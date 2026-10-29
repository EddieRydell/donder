#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]
#![feature(asm_experimental_arch)]

extern crate alloc;
use tinyrlibc as _;

#[cfg(feature = "dig-quad")]
#[path = "../dig_quad.rs"]
mod dig_quad;

#[path = "../fast_divide.rs"]
mod fast_divide;

#[path = "../storage.rs"]
mod storage;
use donder_device_storage::{device_config::DeviceConfig, show_slots};
use storage::SharedStorage;

#[path = "../ws281x_parallel.rs"]
mod ws281x_parallel;

use alloc::boxed::Box;
use core::{
    fmt::Write as _,
    sync::atomic::{AtomicBool, Ordering::Relaxed},
};
use donder_device_protocol::{
    Action, LOADED, PlaybackStatus, TOKEN_HEADER, TransportStatus, clock, path,
};
use donder_runtime::{FrameTiming, LoadError as ShowError, SequenceFrame, ShowBytes, ShowPlayback};
use donder_runtime_types::{SampleTime, sample_time_from_frame};
use embassy_net::StackResources;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex};
use embassy_time::{Duration, Timer};
use embedded_io_async::Read as _;
use esp_hal::{
    Async, dma::DmaTxBuf, gpio::NoPin, i2s::parallel::I2sParallel, system::Stack, time::Rate,
};
use esp_hal::{clock::CpuClock, rng::Rng, time::Instant, timer::timg::TimerGroup};
static SERIAL_DIAGNOSTICS: core::sync::atomic::AtomicBool =
    core::sync::atomic::AtomicBool::new(true);

// Dig-Quad LED2/LED3 share UART0 pins. No application diagnostics may write
// UART0 after those pins have been handed to the LED peripheral.
macro_rules! println {
    ($($arg:tt)*) => {
        if $crate::SERIAL_DIAGNOSTICS.load(core::sync::atomic::Ordering::Relaxed) {
            esp_println::println!($($arg)*);
        }
    };
}
#[path = "../network.rs"]
mod network;

#[cfg(not(feature = "dig-quad"))]
use esp_hal::i2s::parallel::TxEightBits;
use esp_radio::wifi;
use picoserve::{
    AppBuilder, AppRouter,
    response::{IntoResponse, Json, StatusCode},
    routing::{PathRouter, RequestHandlerService, get_service, post_service, put_service},
};
use static_cell::StaticCell;

esp_bootloader_esp_idf::esp_app_desc!();

static OUTPUT_READY: AtomicBool = AtomicBool::new(false);
// Recursive 32-lane playback measured about 17 KiB below its caller. Leave
// headroom for controller/RTOS frames and interrupts; rebalance the heap below.
static APP_CORE_STACK: StaticCell<Stack<{ 24 * 1024 }>> = StaticCell::new();
static APP_CORE_EXECUTOR: StaticCell<esp_rtos::embassy::Executor> = StaticCell::new();

#[path = "../transport.rs"]
mod transport;

#[path = "../loader_control.rs"]
mod loader_control;

type SharedClock = Mutex<CriticalSectionRawMutex, transport::Clock>;

fn local_micros() -> u64 {
    Instant::now().duration_since_epoch().as_micros()
}

struct Playback {
    show: ShowPlayback,
    transport: transport::Transport,
    show_id: u32,
    show_bytes: u32,
}

impl Playback {
    fn render(&mut self, display_time: u64) -> Option<SequenceFrame<'_>> {
        let (mode, position) = self
            .transport
            .sample(display_time, self.show.prepared().duration().as_ticks());
        if matches!(mode, transport::Mode::Stopped | transport::Mode::Ended) {
            return None;
        }
        // Scaled timing holds each grid frame; constant timing samples every latch exactly.
        let rate = self.transport.rate(display_time);
        let frame_rate = self.show.prepared().frame_rate();
        let time = match rate.frame_timing() {
            FrameTiming::Scaled => {
                let frame = rate.frame_at(position, frame_rate);
                sample_time_from_frame(frame, frame_rate).unwrap()
            }
            FrameTiming::Constant => SampleTime::from_ticks(position),
        };
        Some(self.show.evaluate(time))
    }
}
type SharedPlayback = Mutex<CriticalSectionRawMutex, Option<Playback>>;
type UploadGate = Mutex<CriticalSectionRawMutex, ()>;

const HTTP_PORT: u16 = 80;
const HTTP_WORKERS: usize = 2;
const OUTPUT_PIXELS: usize = 200;
const OUTPUT_LANES: usize = 4;
const I2S_SAMPLE_RATE: u32 = 2_400_000;
const DATA_SAMPLES: usize = OUTPUT_PIXELS * 3 * 8 * 3;
const RESET_SAMPLES: usize = I2S_SAMPLE_RATE as usize * 300 / 1_000_000;
const DMA_BYTES: usize = DATA_SAMPLES + RESET_SAMPLES;
/// The fastest frame rate the outputs carry: a frame's data and latch must
/// fit in its period.
const MAX_FRAME_RATE: u32 = (I2S_SAMPLE_RATE as usize / (DATA_SAMPLES + RESET_SAMPLES)) as u32;
/// Frame rate of the black frames sent while no show plays.
const IDLE_FRAME_RATE: u32 = 30;
type ParallelOutput = I2sParallel<'static, Async>;

/// Why a show does not load.
enum LoadError {
    /// The show and its playback buffer need more heap than is free.
    Memory { needed: usize, free: usize },
    /// Its outputs or frame rate exceed what this controller drives.
    Outputs,
    /// It was prepared for another format.
    Version,
    /// Its flash slot could not be read.
    Read,
}

impl From<ShowError> for LoadError {
    fn from(ShowError::Version: ShowError) -> Self {
        Self::Version
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    println!("LOADER PANIC: {}", info);
    esp_hal::system::software_reset()
}

/// Heap left for the network after a show loads. Wi-Fi allocates buffers on
/// demand, and an allocation failure there freezes the controller.
const NETWORK_HEAP_RESERVE: usize = 10 * 1024;

/// Heap a show may use.
fn available_heap() -> usize {
    esp_alloc::HEAP.free().saturating_sub(NETWORK_HEAP_RESERVE)
}

/// Room in RAM for a show of `len` bytes. An allocation failure resets the
/// controller, so the show's copy and then its playback buffer must fit.
fn allocate(len: usize) -> Result<ShowBytes, LoadError> {
    if len > available_heap() {
        return Err(LoadError::Memory {
            needed: len,
            free: available_heap(),
        });
    }
    Ok(ShowBytes::new(len))
}

/// Play `bytes`, a show of `len` bytes, when this controller can.
#[inline(never)]
fn admit(bytes: ShowBytes, len: usize) -> Result<Playback, LoadError> {
    let prepared = bytes.open()?;
    if prepared.outputs().is_empty()
        || prepared.frame_rate() > MAX_FRAME_RATE
        || prepared.outputs().len() > OUTPUT_LANES
        || prepared
            .outputs()
            .iter()
            .any(|output| output.width as usize > OUTPUT_PIXELS * 3 || output.width % 3 != 0)
    {
        return Err(LoadError::Outputs);
    }
    if prepared.workspace_bytes() > available_heap() {
        return Err(LoadError::Memory {
            needed: len + prepared.workspace_bytes(),
            free: len + available_heap(),
        });
    }
    Ok(Playback {
        show_id: prepared.id(),
        show_bytes: len as u32,
        show: prepared.into_playback(),
        transport: transport::Transport::new(),
    })
}

#[derive(Clone, Copy)]
struct LoaderState {
    playback: &'static SharedPlayback,
    upload: &'static UploadGate,
    storage: &'static SharedStorage,
    clock: &'static SharedClock,
    boot_id: u32,
}

/// An unclaimed controller authorizes nothing but `POST /claim`.
fn token_matches(supplied: &[u8]) -> bool {
    network::token().is_some_and(|token| {
        supplied.len() == token.len()
            && supplied
                .iter()
                .zip(token)
                .fold(0, |difference, (&left, right)| difference | (left ^ right))
                == 0
    })
}

/// Serves `S` to requests that carry the claim token.
struct Authorized<S>(S);

impl<S: RequestHandlerService<LoaderState>> RequestHandlerService<LoaderState> for Authorized<S> {
    async fn call_request_handler_service<
        R: picoserve::io::Read,
        W: picoserve::response::ResponseWriter<Error = R::Error>,
    >(
        &self,
        state: &LoaderState,
        (): (),
        request: picoserve::request::Request<'_, R>,
        response_writer: W,
    ) -> Result<picoserve::ResponseSent, W::Error> {
        let supplied = request.parts.headers().get(TOKEN_HEADER);
        if !supplied.is_some_and(|supplied| token_matches(supplied.as_raw())) {
            return (
                StatusCode::UNAUTHORIZED,
                "Missing or invalid X-Donder-Token\n",
            )
                .write_to(request.body_connection.finalize().await?, response_writer)
                .await;
        }
        self.0
            .call_request_handler_service(state, (), request, response_writer)
            .await
    }
}

/// Apply one change to the saved controller configuration. Output pauses
/// while flash is used.
async fn update_config(
    state: &LoaderState,
    change: impl FnOnce(&mut DeviceConfig),
) -> Result<(), &'static str> {
    let _upload = state.upload.lock().await;
    let mut storage = storage::access(state.storage).await;
    let mut config = DeviceConfig::load(&mut *storage)
        .map_err(|_| "Saved configuration is damaged\n")?
        .unwrap_or_else(network::default_config);
    change(&mut config);
    config
        .save(&mut *storage)
        .map_err(|_| "Configuration is invalid or could not be saved\n")
}

async fn read_body<R: picoserve::io::Read, const N: usize>(
    body: &mut picoserve::request::RequestBodyConnection<'_, R>,
) -> Result<Option<heapless::Vec<u8, N>>, R::Error> {
    let length = body.content_length();
    if length > N {
        return Ok(None);
    }
    let mut bytes = heapless::Vec::<u8, N>::new();
    bytes.resize(length, 0).unwrap();
    let mut reader = body.body().reader();
    let mut offset = 0;
    while offset < length {
        let read = reader.read(&mut bytes[offset..]).await?;
        if read == 0 {
            return Ok(None);
        }
        offset += read;
    }
    Ok(Some(bytes))
}

/// First claim wins. The token authorizes every later request.
struct Claim;

impl RequestHandlerService<LoaderState> for Claim {
    async fn call_request_handler_service<
        R: picoserve::io::Read,
        W: picoserve::response::ResponseWriter<Error = R::Error>,
    >(
        &self,
        state: &LoaderState,
        (): (),
        request: picoserve::request::Request<'_, R>,
        response_writer: W,
    ) -> Result<picoserve::ResponseSent, W::Error> {
        let connection = request.body_connection.finalize().await?;
        if network::token().is_some() {
            return (StatusCode::CONFLICT, "Controller is already claimed\n")
                .write_to(connection, response_writer)
                .await;
        }
        let rng = Rng::new();
        let mut raw = [0; 16];
        for word in raw.chunks_exact_mut(4) {
            word.copy_from_slice(&rng.random().to_le_bytes());
        }
        let token = token_ascii(raw);
        if let Err(error) = update_config(state, |config| config.token = Some(token)).await {
            return (StatusCode::SERVICE_UNAVAILABLE, error)
                .write_to(connection, response_writer)
                .await;
        }
        network::IDENTITY.lock(|identity| identity.borrow_mut().token = Some(token));
        network::ANNOUNCE.signal(());
        (
            StatusCode::OK,
            core::str::from_utf8(&token).unwrap_or_default(),
        )
            .write_to(connection, response_writer)
            .await
    }
}

/// `PUT /name` with the new UTF-8 name as the body.
struct Rename;

impl RequestHandlerService<LoaderState> for Rename {
    async fn call_request_handler_service<
        R: picoserve::io::Read,
        W: picoserve::response::ResponseWriter<Error = R::Error>,
    >(
        &self,
        state: &LoaderState,
        (): (),
        mut request: picoserve::request::Request<'_, R>,
        response_writer: W,
    ) -> Result<picoserve::ResponseSent, W::Error> {
        let body = read_body::<_, { donder_device_storage::device_config::MAX_NAME_BYTES }>(
            &mut request.body_connection,
        )
        .await?;
        let connection = request.body_connection.finalize().await?;
        let Some(name) = body
            .as_deref()
            .and_then(|bytes| core::str::from_utf8(bytes).ok())
            .filter(|name| donder_device_storage::device_config::valid_name(name))
            .and_then(|name| network::Name::try_from(name).ok())
        else {
            return (
                StatusCode::BAD_REQUEST,
                "Name must be 1-32 bytes of UTF-8 without control characters\n",
            )
                .write_to(connection, response_writer)
                .await;
        };
        if let Err(error) = update_config(state, |config| config.name = name.as_str().into()).await
        {
            return (StatusCode::SERVICE_UNAVAILABLE, error)
                .write_to(connection, response_writer)
                .await;
        }
        network::IDENTITY.lock(|identity| identity.borrow_mut().name = name);
        network::ANNOUNCE.signal(());
        (StatusCode::OK, "OK\n")
            .write_to(connection, response_writer)
            .await
    }
}

/// `PUT /network` with `{"ssid": ..., "password": ...}` joins that network
/// after a restart; an empty body returns to the controller's access point.
struct SetNetwork;

impl RequestHandlerService<LoaderState> for SetNetwork {
    async fn call_request_handler_service<
        R: picoserve::io::Read,
        W: picoserve::response::ResponseWriter<Error = R::Error>,
    >(
        &self,
        state: &LoaderState,
        (): (),
        mut request: picoserve::request::Request<'_, R>,
        response_writer: W,
    ) -> Result<picoserve::ResponseSent, W::Error> {
        let body = read_body::<_, 192>(&mut request.body_connection).await?;
        let connection = request.body_connection.finalize().await?;
        let network = match body.as_deref() {
            Some([]) => None,
            Some(bytes) => match donder_device_storage::device_config::Network::from_json(bytes) {
                Ok(network) => Some(network),
                Err(_) => {
                    return (
                        StatusCode::BAD_REQUEST,
                        "Network needs a 1-32 byte name and an 8-64 byte WPA2 password\n",
                    )
                        .write_to(connection, response_writer)
                        .await;
                }
            },
            None => {
                return (
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "Network request is too large\n",
                )
                    .write_to(connection, response_writer)
                    .await;
            }
        };
        if let Err(error) = update_config(state, |config| config.network = network).await {
            return (StatusCode::SERVICE_UNAVAILABLE, error)
                .write_to(connection, response_writer)
                .await;
        }
        network::RESTART.signal(());
        (StatusCode::OK, "OK; restarting\n")
            .write_to(connection, response_writer)
            .await
    }
}

struct UploadShow;

impl RequestHandlerService<LoaderState> for UploadShow {
    async fn call_request_handler_service<
        R: picoserve::io::Read,
        W: picoserve::response::ResponseWriter<Error = R::Error>,
    >(
        &self,
        state: &LoaderState,
        (): (),
        mut request: picoserve::request::Request<'_, R>,
        response_writer: W,
    ) -> Result<picoserve::ResponseSent, W::Error> {
        // One show replaces another at a time: each needs the free heap.
        let _upload = state.upload.lock().await;
        let length = request.body_connection.content_length();
        if length > show_slots::CAPACITY {
            return (
                StatusCode::UNPROCESSABLE_ENTITY,
                format_args!(
                    "The show is {length} bytes; this controller stores at most {}.\n",
                    show_slots::CAPACITY
                ),
            )
                .write_to(request.body_connection.finalize().await?, response_writer)
                .await;
        }

        // Replacement stops playback immediately, releasing the show and its
        // buffer for the new one; the previous committed flash slot survives
        // until the new show is admitted.
        state.playback.lock().await.take();
        let mut bytes = match allocate(length) {
            Ok(bytes) => bytes,
            Err(error) => {
                return rejected(
                    error,
                    request.body_connection.finalize().await?,
                    response_writer,
                )
                .await;
            }
        };
        match request
            .body_connection
            .body()
            .reader()
            .with_different_timeout(Duration::from_secs(15))
            .read_exact(&mut bytes.as_mut_bytes()[..length])
            .await
        {
            Ok(()) => {}
            Err(embedded_io_async::ReadExactError::UnexpectedEof) => {
                return (StatusCode::BAD_REQUEST, "Incomplete show body\n")
                    .write_to(request.body_connection.finalize().await?, response_writer)
                    .await;
            }
            Err(embedded_io_async::ReadExactError::Other(error)) => return Err(error),
        }
        let connection = request.body_connection.finalize().await?;
        let mut storage = storage::access(state.storage).await;
        let Ok(slot) = show_slots::begin(&mut storage.shows(), length).and_then(|slot| {
            show_slots::append(&mut storage.shows(), slot, 0, bytes.as_mut_bytes())?;
            Ok(slot)
        }) else {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Cannot save the show in flash\n",
            )
                .write_to(connection, response_writer)
                .await;
        };
        let playback = match admit(bytes, length) {
            Ok(playback) => playback,
            Err(error) => return rejected(error, connection, response_writer).await,
        };
        if show_slots::commit(&mut storage.shows(), slot).is_err() {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "Cannot commit the show; playback stopped and the previous saved show remains\n",
            )
                .write_to(connection, response_writer)
                .await;
        }
        *state.playback.lock().await = Some(playback);
        (StatusCode::OK, format_args!("{LOADED}\n"))
            .write_to(connection, response_writer)
            .await
    }
}

/// Refuse a show, saying why.
async fn rejected<
    R: picoserve::io::Read,
    W: picoserve::response::ResponseWriter<Error = R::Error>,
>(
    error: LoadError,
    connection: picoserve::response::Connection<'_, R>,
    response_writer: W,
) -> Result<picoserve::ResponseSent, W::Error> {
    (
        StatusCode::UNPROCESSABLE_ENTITY,
        format_args!(
            "{}. Playback stopped; the previous saved show remains.\n",
            Rejection(error)
        ),
    )
        .write_to(connection, response_writer)
        .await
}

/// Why a show was refused, as the editor shows it.
struct Rejection(LoadError);

impl core::fmt::Display for Rejection {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self.0 {
            LoadError::Memory { needed, free } => write!(
                formatter,
                "The show needs {needed} bytes of memory and this controller has {free} free"
            ),
            LoadError::Outputs => write!(
                formatter,
                "The show needs more than {OUTPUT_LANES} outputs of {OUTPUT_PIXELS} pixels or more than {MAX_FRAME_RATE} frames per second"
            ),
            LoadError::Version => {
                formatter.write_str("The show was prepared for another firmware format")
            }
            LoadError::Read => formatter.write_str("The staged show could not be read back"),
        }
    }
}

/// `POST /frame` with a little-endian `u32` tick evaluates that frame of the
/// loaded show and replies `FRAME <tick> <crc32 of its output bytes> <µs>`.
struct EvaluateFrame;

impl RequestHandlerService<LoaderState> for EvaluateFrame {
    async fn call_request_handler_service<
        R: picoserve::io::Read,
        W: picoserve::response::ResponseWriter<Error = R::Error>,
    >(
        &self,
        state: &LoaderState,
        (): (),
        mut request: picoserve::request::Request<'_, R>,
        response_writer: W,
    ) -> Result<picoserve::ResponseSent, W::Error> {
        let body = read_body::<_, 4>(&mut request.body_connection).await?;
        let connection = request.body_connection.finalize().await?;
        let Some(ticks) = body.and_then(|bytes| <[u8; 4]>::try_from(bytes.as_slice()).ok()) else {
            return (StatusCode::BAD_REQUEST, "Frame body must be one u32 tick\n")
                .write_to(connection, response_writer)
                .await;
        };
        let ticks = u32::from_le_bytes(ticks);
        let mut active = state.playback.lock().await;
        let Some(Playback { show, .. }) = active.as_mut() else {
            drop(active);
            return (StatusCode::CONFLICT, "No show is loaded\n")
                .write_to(connection, response_writer)
                .await;
        };
        let start = Instant::now();
        let rendered = show.evaluate(SampleTime::from_ticks(ticks));
        let elapsed = start.elapsed().as_micros();
        let mut crc = crc32fast::Hasher::new();
        for output in rendered.outputs() {
            crc.update(output.bytes);
        }
        let crc = crc.finalize();
        drop(active);
        (
            StatusCode::OK,
            format_args!("FRAME {ticks} {crc} {elapsed}\n"),
        )
            .write_to(connection, response_writer)
            .await
    }
}

struct DeviceTransport(Option<Action>);

impl RequestHandlerService<LoaderState> for DeviceTransport {
    async fn call_request_handler_service<
        R: picoserve::io::Read,
        W: picoserve::response::ResponseWriter<Error = R::Error>,
    >(
        &self,
        state: &LoaderState,
        (): (),
        request: picoserve::request::Request<'_, R>,
        response_writer: W,
    ) -> Result<picoserve::ResponseSent, W::Error> {
        let connection = request.body_connection.finalize().await?;
        let now = state.clock.lock().await.master_at(local_micros());
        let mut active = state.playback.lock().await;
        if let Some(action) = self.0 {
            let Some(playback) = active.as_mut() else {
                drop(active);
                return (StatusCode::CONFLICT, "No show is loaded\n")
                    .write_to(connection, response_writer)
                    .await;
            };
            let mode = transport::Mode::from(action);
            let position = if mode == transport::Mode::Stopped {
                0
            } else {
                playback
                    .transport
                    .sample(now, playback.show.prepared().duration().as_ticks())
                    .1
            };
            playback.transport.apply(
                mode,
                position,
                now,
                true,
                donder_runtime::PlaybackRate::NORMAL,
            );
        }
        let status = TransportStatus {
            playback: active.as_ref().map(|playback| {
                let (mode, position_micros) = playback
                    .transport
                    .sample(now, playback.show.prepared().duration().as_ticks());
                PlaybackStatus {
                    mode,
                    position_micros,
                    show_id: playback.show_id,
                    show_bytes: playback.show_bytes,
                    command_id: playback.transport.command_id,
                }
            }),
        };
        drop(active);
        Json(status)
            .into_response()
            .with_header("Cache-Control", "no-store")
            .write_to(connection, response_writer)
            .await
    }
}

struct WebApp {
    state: LoaderState,
}

impl AppBuilder for WebApp {
    type PathRouter = impl PathRouter;

    fn build_app(self) -> picoserve::Router<Self::PathRouter> {
        let transport_route = |action| Authorized(DeviceTransport(action));
        picoserve::Router::new()
            .route(path::CLAIM, post_service(Claim))
            .route(path::SHOW, put_service(Authorized(UploadShow)))
            .route("/frame", post_service(Authorized(EvaluateFrame)))
            .route(path::NAME, put_service(Authorized(Rename)))
            .route(path::NETWORK, put_service(Authorized(SetNetwork)))
            .route(
                path::CONTROL,
                post_service(Authorized(loader_control::Control)),
            )
            .route(path::TRANSPORT, get_service(transport_route(None)))
            .route(
                path::PLAY,
                post_service(transport_route(Some(Action::Play))),
            )
            .route(
                path::PAUSE,
                post_service(transport_route(Some(Action::Pause))),
            )
            .route(
                path::STOP,
                post_service(transport_route(Some(Action::Stop))),
            )
            .with_state(self.state)
    }
}

static SERVER_CONFIG: picoserve::Config = picoserve::Config::new(picoserve::Timeouts {
    start_read_request: Duration::from_secs(5),
    persistent_start_read_request: Duration::from_secs(5),
    read_request: Duration::from_secs(3),
    write: Duration::from_secs(5),
})
.keep_connection_alive();

#[embassy_executor::task]
async fn network_runner(mut runner: embassy_net::Runner<'static, wifi::Interface>) {
    runner.run().await;
}

#[embassy_executor::task(pool_size = HTTP_WORKERS)]
async fn web_server(
    task_id: usize,
    stack: embassy_net::Stack<'static>,
    app: &'static AppRouter<WebApp>,
) -> ! {
    let mut tcp_rx = [0; 2048];
    let mut tcp_tx = [0; 1024];
    let mut http = [0; 2048];
    loop {
        let mut socket = embassy_net::tcp::TcpSocket::new(stack, &mut tcp_rx, &mut tcp_tx);
        // Small clock/control responses must not wait for a delayed TCP ACK
        // between the response headers and body.
        socket.set_nagle_enabled(false);
        socket.set_keep_alive(Some(Duration::from_secs(30)));
        socket.set_timeout(Some(Duration::from_secs(45)));
        if let Err(error) = socket.accept(HTTP_PORT).await {
            println!("HTTP {} accept failed: {:?}", task_id, error);
            continue;
        }
        if let Err(error) = picoserve::Server::new(app, &SERVER_CONFIG, &mut http)
            .serve(socket)
            .await
        {
            println!("HTTP {} request failed: {:?}", task_id, error);
        }
    }
}

fn token_ascii(token: [u8; 16]) -> [u8; 32] {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut result = [0; 32];
    for (index, byte) in token.into_iter().enumerate() {
        result[index * 2] = HEX[(byte >> 4) as usize];
        result[index * 2 + 1] = HEX[(byte & 0xf) as usize];
    }
    result
}

#[embassy_executor::task]
async fn clock_server(stack: embassy_net::Stack<'static>, boot_id: u32) -> ! {
    use embassy_net::udp::{PacketMetadata, UdpSocket};
    let mut rx_meta = [PacketMetadata::EMPTY; 4];
    let mut tx_meta = [PacketMetadata::EMPTY; 4];
    let mut rx = [0; 176];
    let mut tx = [0; 128];
    let mut socket = UdpSocket::new(stack, &mut rx_meta, &mut rx, &mut tx_meta, &mut tx);
    socket.bind(HTTP_PORT).unwrap();
    let mut request = [0; clock::Request::BYTES];
    loop {
        let Ok((length, remote)) = socket.recv_from(&mut request).await else {
            continue;
        };
        let received = local_micros();
        let Some(request) = clock::Request::decode(&request[..length])
            .filter(|request| token_matches(&request.token))
        else {
            continue;
        };
        let reply = clock::Reply {
            sent_micros: request.sent_micros,
            boot_id,
            received_micros: received,
            replied_micros: local_micros(),
        };
        if let Err(error) = socket.send_to(&reply.encode(), remote).await {
            println!("Clock reply failed: {:?}", error);
        }
    }
}

/// Start sending `buffer` to the LEDs.
fn transmit(
    output: ParallelOutput,
    buffer: DmaTxBuf,
) -> esp_hal::i2s::parallel::I2sParallelTransfer<'static, DmaTxBuf, Async> {
    match output.send(buffer) {
        Ok(transfer) => transfer,
        Err((error, _, _)) => panic!("I2S DMA start failed: {error:?}"),
    }
}

#[embassy_executor::task]
async fn render_outputs(
    playback: &'static SharedPlayback,
    clock: &'static SharedClock,
    mut output: ParallelOutput,
    mut ready_buffer: DmaTxBuf,
    mut spare_buffer: DmaTxBuf,
) -> ! {
    encode_frame(None, ready_buffer.as_mut_slice());
    let data_micros = (DATA_SAMPLES as u64 * 1_000_000).div_ceil(u64::from(I2S_SAMPLE_RATE));
    let mut next_transmit = local_micros();
    let mut ready_frame = 0;
    let mut render_budget = 1_000;
    let mut previous_clock = 0;
    let mut previous_rate = 0;
    let mut ready_signature = None;
    let mut ready_display_time = 0;
    loop {
        storage::mark(1, 10);
        if storage::pause_requested() {
            storage::mark(1, 30);
            storage::pause();
            storage::mark(1, 31);
            ready_signature = None;
        }
        if let Some(wait) = next_transmit.checked_sub(local_micros()) {
            Timer::after_micros(wait).await;
        }
        storage::mark(1, 11);
        let mut current = playback.lock().await;
        storage::mark(1, 12);
        let frame_rate = current
            .as_ref()
            .map_or(IDLE_FRAME_RATE, |p| p.show.prepared().frame_rate());
        let signature = current
            .as_ref()
            .map(|p| (p.show_id, p.show_bytes, p.transport.generation));
        if signature != ready_signature {
            let rendered = current.as_mut().and_then(|p| p.render(ready_display_time));
            encode_frame(rendered, ready_buffer.as_mut_slice());
        }
        storage::mark(1, 13);
        let mut transfer = transmit(output, ready_buffer);
        drop(current);
        storage::mark(1, 14);

        let model = *clock.lock().await;
        storage::mark(1, 15);
        let master_now = model.master_at(local_micros());
        // Frame numbers count periods of one clock and one rate.
        if (model.id, frame_rate) != (previous_clock, previous_rate) {
            ready_frame = master_now * u64::from(frame_rate) / 1_000_000;
            previous_clock = model.id;
            previous_rate = frame_rate;
        }
        let next_frame = (ready_frame + 1).max(
            ((master_now + data_micros + render_budget) * u64::from(frame_rate))
                .div_ceil(1_000_000),
        );
        let next_latch = (next_frame * 1_000_000).div_ceil(u64::from(frame_rate));
        next_transmit = model.local_at(next_latch.saturating_sub(data_micros));
        ready_frame = next_frame;
        let render_start = Instant::now();
        storage::mark(1, 16);
        let mut active = playback.lock().await;
        storage::mark(1, 17);
        let rendered = active.as_mut().and_then(|playback| {
            playback
                .transport
                .refresh(master_now, playback.show.prepared().duration().as_ticks());
            playback.render(next_latch)
        });
        storage::mark(1, 18);
        encode_frame(rendered, spare_buffer.as_mut_slice());
        storage::mark(1, 19);
        ready_signature = active
            .as_ref()
            .map(|p| (p.show_id, p.show_bytes, p.transport.generation));
        ready_display_time = next_latch;
        drop(active);
        render_budget = render_start.elapsed().as_micros() + 500;

        storage::mark(1, 20);
        transfer.wait_for_done().await.unwrap();
        storage::mark(1, 21);
        let (next_output, finished_buffer) = transfer.wait();
        output = next_output;
        ready_buffer = spare_buffer;
        spare_buffer = finished_buffer;
    }
}

/// Load the newest saved show, stopped. This runs after the network has
/// allocated its buffers, so releasing the show for an upload frees one
/// contiguous region instead of gaps between network buffers.
async fn restore_show(storage: &SharedStorage, playback: &SharedPlayback) {
    let mut storage = storage::access(storage).await;
    let restored = match show_slots::latest(&mut storage.shows()) {
        Ok(Some(slot)) => allocate(slot.length)
            .and_then(|mut bytes| {
                storage
                    .read_show(slot, bytes.as_mut_bytes())
                    .map_err(|_| LoadError::Read)?;
                admit(bytes, slot.length)
            })
            .ok(),
        Ok(None) => return,
        Err(_) => None,
    };
    // The editor uploads a current show on its next Play; the slot is
    // replaced then, not erased now.
    if restored.is_none() {
        println!(
            "DONDER saved show is unreadable or invalid for this firmware; starting without it"
        );
    }
    *playback.lock().await = restored;
}

#[esp_rtos::main]
async fn main(spawner: embassy_executor::Spawner) -> ! {
    let p = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::max()));
    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 64 * 1024);
    // The heap holds the playing show's prepared sequence and playback
    // buffer beside the network's buffers; the rest of DRAM is the network
    // core's stack, which loading a show and
    // serving requests use deeply. Total heap with output enabled is 126 KiB.
    esp_alloc::heap_allocator!(size: 62 * 1024);
    let timer = TimerGroup::new(p.TIMG0);
    esp_rtos::start(timer.timer0, p.FROM_CPU_INTR0);
    let marks = unsafe { storage::MARKS };
    esp_println::println!("BOOT reason={:?} marks={:?}", esp_hal::system::reset_reason(), marks);
    for index in 0..4 {
        storage::mark(index, 0);
    }
    let mut rtc = esp_hal::rtc_cntl::Rtc::new(p.RTC_TIMER);
    rtc.rwdt.set_timeout(esp_hal::rtc_cntl::RwdtStage::Stage0, esp_hal::time::Duration::from_millis(8000));
    rtc.rwdt.enable();

    let mut storage =
        storage::DeviceStorage::new(p.FLASH).unwrap_or_else(|error| panic!("{error}"));
    donder_device_storage::initialize(&mut storage).expect("Donder storage cannot be formatted");
    // A damaged record is replaced by the next claim or settings change.
    let config = DeviceConfig::load(&mut storage)
        .ok()
        .flatten()
        .unwrap_or_else(network::default_config);
    network::IDENTITY.lock(|identity| {
        let mut identity = identity.borrow_mut();
        identity.name = network::Name::try_from(config.name.as_str()).unwrap();
        identity.token = config.token;
    });
    let rng = Rng::new();
    let storage: &'static SharedStorage =
        picoserve::make_static!(SharedStorage, Mutex::new(storage));
    let playback: &'static SharedPlayback =
        picoserve::make_static!(SharedPlayback, Mutex::new(None));
    let upload = picoserve::make_static!(UploadGate, Mutex::new(()));
    let boot_id = rng.random();
    let clock: &'static SharedClock =
        picoserve::make_static!(SharedClock, Mutex::new(transport::Clock::new()));

    #[cfg(feature = "dig-quad")]
    SERIAL_DIAGNOSTICS.store(false, Relaxed);

    // Start output before the network: joining a missing network must not
    // delay a restored show.
    esp_rtos::start_second_core(
        p.CPU_CTRL,
        p.FROM_CPU_INTR1,
        APP_CORE_STACK.init(Stack::new()),
        move || {
            #[cfg(feature = "dig-quad")]
            let pins = dig_quad::output_pins(p.GPIO16, p.GPIO3, p.GPIO1, p.GPIO4);
            #[cfg(not(feature = "dig-quad"))]
            let pins = TxEightBits::new(
                p.GPIO13, p.GPIO18, p.GPIO21, p.GPIO25, NoPin, NoPin, NoPin, NoPin,
            );
            let output = I2sParallel::new(
                p.I2S1,
                p.DMA_I2S1,
                Rate::from_hz(I2S_SAMPLE_RATE),
                pins,
                NoPin,
            )
            .into_async();
            let mut ready_buffer = esp_hal::dma_tx_buffer!(DMA_BYTES).unwrap();
            ready_buffer.as_mut_slice().fill(0);
            ready_buffer.set_length(DMA_BYTES);
            let mut spare_buffer = esp_hal::dma_tx_buffer!(DMA_BYTES).unwrap();
            spare_buffer.as_mut_slice().fill(0);
            spare_buffer.set_length(DMA_BYTES);
            OUTPUT_READY.store(true, Relaxed);
            APP_CORE_EXECUTOR
                .init(esp_rtos::embassy::Executor::new())
                .run(|spawner| {
                    spawner.spawn(
                        render_outputs(playback, clock, output, ready_buffer, spare_buffer)
                            .unwrap(),
                    );
                });
        },
    );

    while !OUTPUT_READY.load(Relaxed) {
        Timer::after_millis(1).await;
    }

    let (controller, interface, mode) = network::start(p.WIFI, &config).await;
    drop(config);
    let id: &'static str = picoserve::make_static!(heapless::String<12>, network::device_id());
    let hostname: &'static str = picoserve::make_static!(heapless::String<19>, {
        let mut hostname = heapless::String::new();
        write!(hostname, "donder-{id}").unwrap();
        hostname
    });
    let seed = (u64::from(rng.random()) << 32) | u64::from(rng.random());
    // HTTP workers, clock, mDNS, and the DHCP client or server.
    let resources = Box::leak(Box::new(StackResources::<{ HTTP_WORKERS + 3 }>::new()));
    let (stack, runner) = embassy_net::new(interface, network::stack_config(mode), resources, seed);
    let udp_pool = network::udp_pool();
    spawner.spawn(network_runner(runner).unwrap());
    spawner.spawn(network::reconnect(controller, mode).unwrap());
    spawner.spawn(network::restart_after_change().unwrap());
    if mode == network::Mode::AccessPoint {
        spawner.spawn(network::dhcp_server(stack, udp_pool).unwrap());
    }
    spawner.spawn(network::mdns_responder(stack, udp_pool, id, hostname, mode).unwrap());
    restore_show(storage, playback).await;
    let app = picoserve::make_static!(
        AppRouter<WebApp>,
        WebApp {
            state: LoaderState {
                playback,
                upload,
                storage,
                clock,
                boot_id,
            }
        }
        .build_app()
    );
    for task_id in 0..HTTP_WORKERS {
        spawner.spawn(web_server(task_id, stack, app).unwrap());
    }
    spawner.spawn(clock_server(stack, boot_id).unwrap());
    println!(
        "NETWORK mode={} id={} heap_free={}",
        mode.label(),
        id,
        esp_alloc::HEAP.free()
    );

    loop {
        Timer::after_secs(1).await;
        rtc.rwdt.feed();
        let marks = unsafe { core::ptr::read_volatile(&raw const storage::MARKS) };
        esp_println::println!("H {:?} free={}", marks, esp_alloc::HEAP.free());
    }
}

fn encode_frame(frame: Option<SequenceFrame<'_>>, buffer: &mut [u8]) {
    let mut lanes: [&[u8]; OUTPUT_LANES] = [&[]; OUTPUT_LANES];
    let count = match frame {
        Some(frame) => {
            let count = frame.outputs().len();
            for (lane, output) in lanes.iter_mut().zip(frame.outputs()) {
                *lane = output.bytes;
            }
            count
        }
        None => OUTPUT_LANES,
    };
    #[cfg(feature = "dig-quad")]
    let brightness = dig_quad::MAX_CHANNEL_VALUE;
    #[cfg(not(feature = "dig-quad"))]
    let brightness = u8::MAX;
    ws281x_parallel::encode(&lanes[..count], OUTPUT_PIXELS, buffer, brightness);
}
