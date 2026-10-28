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
type SharedStorage = Mutex<CriticalSectionRawMutex, storage::DeviceStorage>;

#[cfg(feature = "i2s-output")]
#[path = "../ws281x_parallel.rs"]
mod ws281x_parallel;

use alloc::{boxed::Box, vec};
#[cfg(feature = "i2s-output")]
use core::sync::atomic::AtomicBool;
use core::{
    fmt::Write as _,
    sync::atomic::{AtomicU32, Ordering::Relaxed},
};
use donder_device_protocol::{Capabilities, LOADED, OutputCapabilities, TOKEN_HEADER, path};
#[cfg(feature = "i2s-output")]
use donder_device_protocol::{PlaybackStatus, TransportStatus, clock};
#[cfg(feature = "i2s-output")]
use donder_runtime::FrameTiming;
use donder_runtime::{Layout, SequenceFrame, header_bytes};
use donder_runtime_types::SampleTime;
#[cfg(feature = "i2s-output")]
use donder_runtime_types::sample_time_from_frame;
use embassy_net::StackResources;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex};
use embassy_time::{Duration, Timer};
use embedded_io_async::{Read as _, Write as _};
#[cfg(feature = "i2s-output")]
use esp_hal::{
    Async,
    dma::DmaTxBuf,
    gpio::NoPin,
    i2s::parallel::I2sParallel,
    system::{Cpu, Stack},
    time::Rate,
};
use esp_hal::{
    clock::CpuClock,
    rng::Rng,
    time::Instant,
    timer::timg::TimerGroup,
    uart::{Config, Uart},
};
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

#[cfg(all(feature = "i2s-output", not(feature = "dig-quad")))]
use esp_hal::i2s::parallel::TxEightBits;
use esp_radio::wifi;
use picoserve::{
    AppBuilder, AppRouter,
    response::{IntoResponse, Json, StatusCode},
    routing::{PathRouter, RequestHandlerService, get_service, post_service, put_service},
};
#[cfg(feature = "i2s-output")]
use static_cell::StaticCell;

esp_bootloader_esp_idf::esp_app_desc!();

static ALLOCATIONS: AtomicU32 = AtomicU32::new(0);
static EVALUATION_TASK: AtomicU32 = AtomicU32::new(0);
static EVALUATION_ALLOCATIONS: AtomicU32 = AtomicU32::new(0);
#[cfg(feature = "i2s-output")]
static OUTPUT_READY: AtomicBool = AtomicBool::new(false);
#[cfg(feature = "i2s-output")]
// Recursive 32-lane playback measured about 17 KiB below its caller. Leave
// headroom for controller/RTOS frames and interrupts; rebalance the heap below.
static APP_CORE_STACK: StaticCell<Stack<{ 24 * 1024 }>> = StaticCell::new();
#[cfg(feature = "i2s-output")]
static APP_CORE_EXECUTOR: StaticCell<esp_rtos::embassy::Executor> = StaticCell::new();

#[cfg(feature = "i2s-output")]
#[path = "../transport.rs"]
mod transport;

#[cfg(feature = "i2s-output")]
#[path = "../loader_control.rs"]
mod loader_control;

#[cfg(feature = "i2s-output")]
type SharedClock = Mutex<CriticalSectionRawMutex, transport::Clock>;

#[cfg(feature = "i2s-output")]
fn local_micros() -> u64 {
    Instant::now().duration_since_epoch().as_micros()
}

/// A prepared sequence in RAM, freed when dropped.
struct RamImage(*mut [u32]);

impl RamImage {
    fn new(words: Box<[u32]>) -> Self {
        Self(Box::into_raw(words))
    }

    /// The prepared sequence's bytes. Valid until it is dropped.
    fn bytes(&self, len: usize) -> &'static [u8] {
        // Safety: the words hold `len` bytes; the owner drops every borrow
        // before the copy.
        unsafe { core::slice::from_raw_parts((*self.0).as_ptr().cast::<u8>(), len) }
    }
}

impl Drop for RamImage {
    fn drop(&mut self) {
        // Safety: nothing borrows the copy once its owner drops it.
        drop(unsafe { Box::from_raw(self.0) });
    }
}

/// A show playing from RAM. Its playback is laid out once in a
/// heap buffer and kept between frames, so a frame does not lay it out again.
struct LoadedImage {
    playback: core::mem::ManuallyDrop<Box<donder_runtime::Playback<'static, 'static>>>,
    /// The playback's buffer, freed after the playback that borrows it.
    buffer: *mut [u32],
    /// The prepared sequence the playback reads, dropped after it.
    _prepared: RamImage,
}

// Safety: the buffer is owned by this value alone and only reached through
// its playback, which moves between cores with it.
unsafe impl Send for LoadedImage {}

impl LoadedImage {
    fn new(
        image: donder_runtime::Image<'static>,
        layout: &Layout,
        prepared: RamImage,
    ) -> Option<Self> {
        let buffer = Box::into_raw(vec![0u32; layout.words()].into_boxed_slice());
        // Safety: the buffer stays allocated until `drop`, which drops the
        // playback borrowing it first.
        let words: &'static mut [u32] = unsafe { &mut *buffer };
        match donder_runtime::Playback::new(image, layout, words) {
            Some(playback) => Some(Self {
                playback: core::mem::ManuallyDrop::new(Box::new(playback)),
                buffer,
                _prepared: prepared,
            }),
            None => {
                // Safety: nothing borrows the buffer any more.
                drop(unsafe { Box::from_raw(buffer) });
                None
            }
        }
    }

    fn view(&self) -> donder_runtime::Image<'static> {
        self.playback.image()
    }

    fn evaluate(&mut self, time: SampleTime) -> SequenceFrame<'_> {
        self.playback.evaluate(time)
    }
}

impl Drop for LoadedImage {
    fn drop(&mut self) {
        // Safety: the playback is dropped before the buffer it borrows, and
        // neither is used again.
        unsafe {
            core::mem::ManuallyDrop::drop(&mut self.playback);
            drop(Box::from_raw(self.buffer));
        }
    }
}

struct Playback {
    image: LoadedImage,
    #[cfg(feature = "i2s-output")]
    transport: transport::Transport,
    #[cfg(feature = "i2s-output")]
    image_id: u32,
    #[cfg(feature = "i2s-output")]
    image_bytes: u32,
}

#[cfg(feature = "i2s-output")]
impl Playback {
    fn render(&mut self, display_time: u64) -> Option<SequenceFrame<'_>> {
        let (mode, position) = self
            .transport
            .sample(display_time, self.image.view().duration().as_ticks());
        if matches!(mode, transport::Mode::Stopped | transport::Mode::Ended) {
            return None;
        }
        // Scaled timing holds each grid frame; constant timing samples every latch exactly.
        let rate = self.transport.rate(display_time);
        let frame_rate = self.image.view().frame_rate();
        let time = match rate.frame_timing() {
            FrameTiming::Scaled => {
                let frame = rate.frame_at(position, frame_rate);
                sample_time_from_frame(frame, frame_rate).unwrap()
            }
            FrameTiming::Constant => SampleTime::from_ticks(position),
        };
        Some(self.image.evaluate(time))
    }
}
type SharedPlayback = Mutex<CriticalSectionRawMutex, Option<Playback>>;
type UploadGate = Mutex<CriticalSectionRawMutex, ()>;

const HTTP_PORT: u16 = 80;
const HTTP_WORKERS: usize = 2;
#[cfg(feature = "i2s-output")]
const OUTPUT_PIXELS: usize = 200;
#[cfg(feature = "i2s-output")]
const OUTPUT_LANES: usize = 4;
#[cfg(feature = "i2s-output")]
const I2S_SAMPLE_RATE: u32 = 2_400_000;
#[cfg(feature = "i2s-output")]
const DATA_SAMPLES: usize = OUTPUT_PIXELS * 3 * 8 * 3;
#[cfg(feature = "i2s-output")]
const RESET_SAMPLES: usize = I2S_SAMPLE_RATE as usize * 300 / 1_000_000;
#[cfg(feature = "i2s-output")]
const DMA_BYTES: usize = DATA_SAMPLES + RESET_SAMPLES;
/// The fastest frame rate the outputs carry: a frame's data and latch must
/// fit in its period.
#[cfg(feature = "i2s-output")]
const MAX_FRAME_RATE: u32 = (I2S_SAMPLE_RATE as usize / (DATA_SAMPLES + RESET_SAMPLES)) as u32;
/// Frame rate of the black frames sent while no show plays.
#[cfg(feature = "i2s-output")]
const IDLE_FRAME_RATE: u32 = 30;
#[cfg(feature = "dig-quad")]
const OUTPUT_DESCRIPTION: &str = dig_quad::OUTPUT_DESCRIPTION;
#[cfg(all(feature = "i2s-output", not(feature = "dig-quad")))]
const OUTPUT_DESCRIPTION: &str = "gpio13,18,21,25";
#[cfg(not(feature = "i2s-output"))]
const OUTPUT_DESCRIPTION: &str = "off";

#[cfg(feature = "i2s-output")]
type ParallelOutput = I2sParallel<'static, Async>;

/// What this controller can play, as it reports to hosts.
struct Limits {
    image_bytes: usize,
    pixels: usize,
    graph_nodes: usize,
    workspace_bytes: usize,
}

const LIMITS: Limits = Limits {
    // A playing show holds its prepared sequence and its playback buffer in
    // the heap beside the network; about 75 KiB of it is free for both.
    image_bytes: 64 * 1024,
    pixels: 1600,
    graph_nodes: 128,
    workspace_bytes: 64 * 1024,
};

/// Why a show does not load.
enum LoadError {
    /// It needs more memory, outputs or frames per second than this
    /// controller has.
    Limit,
    /// It was prepared for another format.
    Version,
    /// Its flash slot could not be read.
    Read,
}

#[unsafe(no_mangle)]
fn _esp_alloc_alloc(
    _: &esp_alloc::EspHeap,
    _: esp_alloc::export::enumset::EnumSet<esp_alloc::MemoryCapability>,
    pointer: usize,
    _: usize,
) {
    if pointer != 0 {
        ALLOCATIONS.fetch_add(1, Relaxed);
        let task = EVALUATION_TASK.load(Relaxed);
        if task != 0 && esp_radio_rtos_driver::current_task().as_ptr() as u32 == task {
            EVALUATION_ALLOCATIONS.fetch_add(1, Relaxed);
        }
    }
}

#[unsafe(no_mangle)]
fn _esp_alloc_dealloc(_: &esp_alloc::EspHeap, _: usize, _: usize) {}

/// The last panic's message, kept in RTC memory across the reset that follows
/// it: a Dig-Quad's UART pins drive LEDs, so this is the only place it shows.
#[esp_hal::ram(unstable(rtc_fast, persistent))]
static mut PANIC_MESSAGE: [u8; 256] = [0; 256];
/// `PANIC_MAGIC` when `PANIC_MESSAGE` holds a message, with its length.
#[esp_hal::ram(unstable(rtc_fast, persistent))]
static mut PANIC_RECORD: [u32; 2] = [0; 2];
const PANIC_MAGIC: u32 = 0x5041_4e43;

/// Writes into the panic record, dropping what does not fit.
struct PanicWriter(usize);

impl core::fmt::Write for PanicWriter {
    fn write_str(&mut self, text: &str) -> core::fmt::Result {
        let message = core::ptr::addr_of_mut!(PANIC_MESSAGE);
        for &byte in text.as_bytes() {
            if self.0 < 256 {
                // Safety: only the panicking core writes, once, before reset.
                unsafe { (*message)[self.0] = byte };
                self.0 += 1;
            }
        }
        Ok(())
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    println!("LOADER PANIC: {}", info);
    let mut writer = PanicWriter(0);
    let _ = core::fmt::Write::write_fmt(&mut writer, format_args!("{info}"));
    // Safety: as above.
    unsafe { *core::ptr::addr_of_mut!(PANIC_RECORD) = [PANIC_MAGIC, writer.0 as u32] };
    // Restart rather than hang: the board comes back and serves `/panic`.
    esp_hal::system::software_reset()
}

/// The message the last panic recorded, if any.
fn last_panic() -> Option<&'static str> {
    // Safety: the record is written only by the panic handler, before reset.
    let [magic, len] = unsafe { *core::ptr::addr_of!(PANIC_RECORD) };
    if magic != PANIC_MAGIC {
        return None;
    }
    let message = unsafe { &*core::ptr::addr_of!(PANIC_MESSAGE) };
    Some(core::str::from_utf8(&message[..(len as usize).min(256)]).unwrap_or("(not text)"))
}

struct LastPanic;

impl RequestHandlerService<LoaderState> for LastPanic {
    async fn call_request_handler_service<
        R: picoserve::io::Read,
        W: picoserve::response::ResponseWriter<Error = R::Error>,
    >(
        &self,
        _: &LoaderState,
        (): (),
        request: picoserve::request::Request<'_, R>,
        response_writer: W,
    ) -> Result<picoserve::ResponseSent, W::Error> {
        let connection = request.body_connection.finalize().await?;
        match last_panic() {
            Some(message) => {
                (StatusCode::OK, format_args!("{message}\n"))
                    .write_to(connection, response_writer)
                    .await
            }
            None => {
                (StatusCode::OK, "none\n")
                    .write_to(connection, response_writer)
                    .await
            }
        }
    }
}

async fn uart_reply(
    uart: &mut esp_hal::uart::Uart<'_, esp_hal::Async>,
    args: core::fmt::Arguments<'_>,
) -> Result<(), ()> {
    let mut line = heapless::String::<192>::new();
    line.write_fmt(args).map_err(|_| ())?;
    line.push('\n').map_err(|_| ())?;
    uart.write_all(line.as_bytes()).await.map_err(|_| ())
}

/// Heap left for the network after a show loads. Wi-Fi allocates buffers on
/// demand, and an allocation failure there freezes the controller. The
/// workspace estimate covers the real allocation, so at least this stays free.
const NETWORK_HEAP_RESERVE: usize = 10 * 1024;

#[inline(never)]
fn load(
    storage: &mut storage::DeviceStorage,
    slot: show_slots::Slot,
) -> Result<Playback, LoadError> {
    let free = esp_alloc::HEAP.free();
    println!("LOAD bytes={} heap_free={}", slot.length, free);
    // The prepared image plays from RAM; it and the workspace must leave
    // the network reserve free.
    if slot.length + NETWORK_HEAP_RESERVE > free {
        return Err(LoadError::Limit);
    }
    let copy = RamImage::new(storage.read_image(slot).map_err(|_| LoadError::Read)?);
    let prepared = copy.bytes(slot.length);
    let workspace_bytes = LIMITS
        .workspace_bytes
        .min(esp_alloc::HEAP.free().saturating_sub(NETWORK_HEAP_RESERVE));
    let image = donder_runtime::Image::open(prepared).map_err(|_| LoadError::Version)?;
    #[cfg(feature = "i2s-output")]
    if image.outputs().is_empty()
        || image.frame_rate() > MAX_FRAME_RATE
        || image.outputs().len() > OUTPUT_LANES
        || image
            .outputs()
            .iter()
            .any(|output| output.width as usize > OUTPUT_PIXELS * 3 || output.width % 3 != 0)
    {
        return Err(LoadError::Limit);
    }
    let layout = image.layout();
    if layout.words() * 4 > workspace_bytes {
        return Err(LoadError::Limit);
    }
    let loaded = LoadedImage::new(image, &layout, copy).ok_or(LoadError::Limit)?;
    println!(
        "LOAD workspace_bytes={} heap_free={}",
        layout.words() * 4,
        esp_alloc::HEAP.free()
    );
    Ok(Playback {
        #[cfg(feature = "i2s-output")]
        image_id: donder_runtime::image_id(prepared),
        #[cfg(feature = "i2s-output")]
        image_bytes: prepared.len() as u32,
        image: loaded,
        #[cfg(feature = "i2s-output")]
        transport: transport::Transport::new(),
    })
}

#[derive(Clone, Copy)]
struct LoaderState {
    playback: &'static SharedPlayback,
    upload: &'static UploadGate,
    storage: &'static SharedStorage,
    #[cfg(feature = "i2s-output")]
    clock: &'static SharedClock,
    #[cfg(feature = "i2s-output")]
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

fn authorized(_: &LoaderState, request: &picoserve::request::RequestParts<'_>) -> bool {
    request
        .headers()
        .get(TOKEN_HEADER)
        .is_some_and(|supplied| token_matches(supplied.as_raw()))
}

/// Apply one change to the saved controller configuration. Flash writes park
/// the rendering core, so output pauses briefly.
async fn update_config(
    state: &LoaderState,
    change: impl FnOnce(&mut DeviceConfig),
) -> Result<(), &'static str> {
    let Ok(_upload) = state.upload.try_lock() else {
        return Err("Another upload or configuration change is in progress\n");
    };
    let Ok(_output_suspension) = storage::suspend_output().await else {
        return Err("Rendering core did not release flash access\n");
    };
    let mut storage = state.storage.lock().await;
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
        if !authorized(state, &request.parts) {
            return (
                StatusCode::UNAUTHORIZED,
                "Missing or invalid X-Donder-Token\n",
            )
                .write_to(request.body_connection.finalize().await?, response_writer)
                .await;
        }
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
        if !authorized(state, &request.parts) {
            return (
                StatusCode::UNAUTHORIZED,
                "Missing or invalid X-Donder-Token\n",
            )
                .write_to(request.body_connection.finalize().await?, response_writer)
                .await;
        }
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

struct DeviceCapabilities;

impl RequestHandlerService<LoaderState> for DeviceCapabilities {
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
        if !authorized(state, &request.parts) {
            return (
                StatusCode::UNAUTHORIZED,
                "Missing or invalid X-Donder-Token\n",
            )
                .write_to(request.body_connection.finalize().await?, response_writer)
                .await;
        }
        let capabilities = Capabilities {
            image_format: donder_runtime::FORMAT_VERSION,
            max_payload_bytes: LIMITS.image_bytes as u32,
            max_pixels: LIMITS.pixels as u32,
            max_graph_nodes: LIMITS.graph_nodes as u32,
            max_workspace_bytes: LIMITS.workspace_bytes as u32,
            #[cfg(feature = "i2s-output")]
            output: OutputCapabilities::Ws281x {
                lanes: OUTPUT_LANES as u32,
                channels_per_lane: (OUTPUT_PIXELS * 3) as u32,
                channel_multiple: 3,
                max_frame_rate: MAX_FRAME_RATE,
                clock_udp_port: HTTP_PORT,
            },
            #[cfg(not(feature = "i2s-output"))]
            output: OutputCapabilities::EvaluationOnly,
        };
        Json(capabilities)
            .into_response()
            .with_header("Cache-Control", "no-store")
            .write_to(request.body_connection.finalize().await?, response_writer)
            .await
    }
}

struct UploadImage;

impl RequestHandlerService<LoaderState> for UploadImage {
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
        if !authorized(state, &request.parts) {
            return (
                StatusCode::UNAUTHORIZED,
                "Missing or invalid X-Donder-Token\n",
            )
                .write_to(request.body_connection.finalize().await?, response_writer)
                .await;
        }

        let Ok(_upload) = state.upload.try_lock() else {
            return (StatusCode::CONFLICT, "Another upload is in progress\n")
                .write_to(request.body_connection.finalize().await?, response_writer)
                .await;
        };

        let length = request.body_connection.content_length();
        if !(header_bytes()..=LIMITS.image_bytes).contains(&length) {
            return (
                StatusCode::PAYLOAD_TOO_LARGE,
                "Image exceeds device limits\n",
            )
                .write_to(request.body_connection.finalize().await?, response_writer)
                .await;
        }

        let Ok(_output_suspension) = storage::suspend_output().await else {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                "Rendering core did not release flash access\n",
            )
                .write_to(request.body_connection.finalize().await?, response_writer)
                .await;
        };
        // Replacement stops playback immediately. Release the decoded show and
        // workspace before staging or validating the candidate; the previous
        // committed flash slot survives until the new show is admitted.
        state.playback.lock().await.take();
        let mut storage = state.storage.lock().await;
        #[cfg(not(feature = "dig-quad"))]
        println!(
            "UPLOAD begin bytes={} heap_free={}",
            length,
            esp_alloc::HEAP.free()
        );
        let slot = match show_slots::begin(&mut storage.shows(), length) {
            Ok(slot) => slot,
            Err(_) => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Cannot stage image in flash\n",
                )
                    .write_to(request.body_connection.finalize().await?, response_writer)
                    .await;
            }
        };
        #[cfg(not(feature = "dig-quad"))]
        println!("UPLOAD erased slot={}", slot.index);
        let mut offset = 0;
        let mut scratch = vec![0xff; 1024];
        {
            let mut reader = request
                .body_connection
                .body()
                .reader()
                .with_different_timeout(Duration::from_secs(15));
            while offset < length {
                let count = scratch.len().min(length - offset);
                scratch.fill(0xff);
                let mut filled = 0;
                while filled < count {
                    let read = reader.read(&mut scratch[filled..count]).await?;
                    if read == 0 {
                        break;
                    }
                    filled += read;
                }
                if filled != count {
                    break;
                }
                if show_slots::append(
                    &mut storage.shows(),
                    slot,
                    offset,
                    &scratch[..count.next_multiple_of(4)],
                )
                .is_err()
                {
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Cannot write staged image\n",
                    )
                        .write_to(request.body_connection.finalize().await?, response_writer)
                        .await;
                }
                offset += count;
                #[cfg(not(feature = "dig-quad"))]
                if offset.is_multiple_of(16384) || offset == length {
                    println!("UPLOAD written bytes={}", offset);
                }
            }
        }
        let connection = request.body_connection.finalize().await?;
        drop(scratch);
        if offset != length {
            return (StatusCode::BAD_REQUEST, "Incomplete image body\n")
                .write_to(connection, response_writer)
                .await;
        }
        #[cfg(not(feature = "dig-quad"))]
        println!("UPLOAD loading bytes={}", length);
        let free = esp_alloc::HEAP.free();
        let start = Instant::now();
        match load(&mut storage, slot) {
            Ok(playback) => {
                let pixels = playback.image.view().pixel_count();
                let heap = free.saturating_sub(esp_alloc::HEAP.free());
                let elapsed = start.elapsed().as_micros();
                if show_slots::commit(&mut storage.shows(), slot).is_err() {
                    return (StatusCode::INTERNAL_SERVER_ERROR, "Cannot commit image; playback stopped and previous saved image retained\n")
                        .write_to(connection, response_writer).await;
                }
                *state.playback.lock().await = Some(playback);
                (
                    StatusCode::OK,
                    format_args!(
                        "{LOADED}bytes={} pixels={} heap={} us={}\n",
                        length, pixels, heap, elapsed
                    ),
                )
                    .write_to(connection, response_writer)
                    .await
            }
            Err(error) => {
                let reason = match error {
                    LoadError::Limit => {
                        "the image needs more memory, outputs or frames per second than this controller has"
                    }
                    LoadError::Version => "the image's format does not match this firmware",
                    LoadError::Read => "the staged image could not be read back",
                };
                (
                    StatusCode::UNPROCESSABLE_ENTITY,
                    format_args!("{reason}. Playback stopped; the previous saved image remains.\n"),
                )
                    .write_to(connection, response_writer)
                    .await
            }
        }
    }
}

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
        if !authorized(state, &request.parts) {
            return (
                StatusCode::UNAUTHORIZED,
                "Missing or invalid X-Donder-Token\n",
            )
                .write_to(request.body_connection.finalize().await?, response_writer)
                .await;
        }
        if request.body_connection.content_length() != 4 {
            return (StatusCode::BAD_REQUEST, "Frame body must be one u32 tick\n")
                .write_to(request.body_connection.finalize().await?, response_writer)
                .await;
        }

        let mut ticks = [0; 4];
        let offset = {
            let mut reader = request.body_connection.body().reader();
            let mut offset = 0;
            while offset < ticks.len() {
                let read = reader.read(&mut ticks[offset..]).await?;
                if read == 0 {
                    break;
                }
                offset += read;
            }
            offset
        };
        let connection = request.body_connection.finalize().await?;
        if offset != ticks.len() {
            return (StatusCode::BAD_REQUEST, "Incomplete frame body\n")
                .write_to(connection, response_writer)
                .await;
        }

        let ticks = u32::from_le_bytes(ticks);
        let mut active = state.playback.lock().await;
        let Some(Playback { image, .. }) = active.as_mut() else {
            drop(active);
            return (StatusCode::CONFLICT, "REJECT NoImage\n")
                .write_to(connection, response_writer)
                .await;
        };

        let allocations = ALLOCATIONS.load(Relaxed);
        let evaluation_allocations = EVALUATION_ALLOCATIONS.load(Relaxed);
        EVALUATION_TASK.store(
            esp_radio_rtos_driver::current_task().as_ptr() as u32,
            Relaxed,
        );
        let start = Instant::now();
        let rendered = image.evaluate(SampleTime::from_ticks(ticks));
        let elapsed = start.elapsed().as_micros();
        EVALUATION_TASK.store(0, Relaxed);
        let evaluation_allocations = EVALUATION_ALLOCATIONS.load(Relaxed) - evaluation_allocations;
        let allocations = ALLOCATIONS.load(Relaxed) - allocations;

        let mut crc = crc32fast::Hasher::new();
        for output in rendered.outputs() {
            crc.update(output.bytes);
        }
        let crc = crc.finalize();
        drop(active);
        (
            StatusCode::OK,
            format_args!(
                "FRAME {} {} {} {} {}\n",
                ticks, crc, elapsed, evaluation_allocations, allocations
            ),
        )
            .write_to(connection, response_writer)
            .await
    }
}

#[cfg(feature = "i2s-output")]
struct DeviceTransport(Option<transport::Mode>);

#[cfg(feature = "i2s-output")]
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
        if !authorized(state, &request.parts) {
            return (
                StatusCode::UNAUTHORIZED,
                "Missing or invalid X-Donder-Token\n",
            )
                .write_to(request.body_connection.finalize().await?, response_writer)
                .await;
        }
        if request.body_connection.content_length() != 0 {
            return (StatusCode::BAD_REQUEST, "Transport requests have no body\n")
                .write_to(request.body_connection.finalize().await?, response_writer)
                .await;
        }
        let connection = request.body_connection.finalize().await?;
        let now = state.clock.lock().await.master_at(local_micros());
        let mut active = state.playback.lock().await;
        if let Some(mode) = self.0 {
            let Some(playback) = active.as_mut() else {
                drop(active);
                return (StatusCode::CONFLICT, "No image is loaded\n")
                    .write_to(connection, response_writer)
                    .await;
            };
            let position = if mode == transport::Mode::Stopped {
                0
            } else {
                playback
                    .transport
                    .sample(now, playback.image.view().duration().as_ticks())
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
            playback: active.as_ref().map(|playback| PlaybackStatus {
                mode: playback
                    .transport
                    .sample(now, playback.image.view().duration().as_ticks())
                    .0,
                position_micros: playback
                    .transport
                    .sample(now, playback.image.view().duration().as_ticks())
                    .1,
                duration_micros: playback.image.view().duration().as_ticks(),
                image_id: playback.image_id,
                image_bytes: playback.image_bytes,
                pending_command: playback.transport.pending.map(|command| command.id),
                command_id: playback.transport.command_id,
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
        let router = picoserve::Router::new()
            .route(path::CAPABILITIES, get_service(DeviceCapabilities))
            .route("/panic", get_service(LastPanic))
            .route(path::IMAGE, put_service(UploadImage))
            .route("/frame", post_service(EvaluateFrame))
            .route(path::CLAIM, post_service(Claim))
            .route(path::NAME, put_service(Rename))
            .route(path::NETWORK, put_service(SetNetwork));
        #[cfg(feature = "i2s-output")]
        let router = router
            .route(path::CONTROL, post_service(loader_control::Control))
            .route(path::TRANSPORT, get_service(DeviceTransport(None)))
            .route(
                path::PLAY,
                post_service(DeviceTransport(Some(transport::Mode::Playing))),
            )
            .route(
                path::PAUSE,
                post_service(DeviceTransport(Some(transport::Mode::Paused))),
            )
            .route(
                path::STOP,
                post_service(DeviceTransport(Some(transport::Mode::Stopped))),
            );
        router.with_state(self.state)
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

#[cfg(feature = "i2s-output")]
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

#[cfg(feature = "i2s-output")]
#[embassy_executor::task]
async fn render_outputs(
    playback: &'static SharedPlayback,
    clock: &'static SharedClock,
    mut output: ParallelOutput,
    mut ready_buffer: DmaTxBuf,
    mut spare_buffer: DmaTxBuf,
) -> ! {
    encode_frame(None, ready_buffer.as_mut_slice());
    let mut frames = 0;
    let mut missed = 0;
    let mut evaluation_sum = 0;
    let mut evaluation_max = 0;
    let mut encoding_sum = 0;
    let mut encoding_max = 0;
    let mut wait_sum = 0;
    let mut wait_max = 0;
    let mut total_sum = 0;
    let mut total_max = 0;
    let data_micros = (DATA_SAMPLES as u64 * 1_000_000).div_ceil(u64::from(I2S_SAMPLE_RATE));
    let mut next_transmit = local_micros();
    let mut ready_frame = 0;
    let mut render_budget = 1_000;
    let mut previous_clock = 0;
    let mut previous_rate = 0;
    let mut ready_signature = None;
    let mut ready_display_time = 0;
    loop {
        if storage::flash_requested() {
            encode_frame(None, ready_buffer.as_mut_slice());
            let mut transfer = match output.send(ready_buffer) {
                Ok(transfer) => transfer,
                Err((error, _, _)) => panic!("I2S DMA start failed: {error:?}"),
            };
            transfer.wait_for_done().await.unwrap();
            (output, ready_buffer) = transfer.wait();
            storage::flash_checkpoint();
            ready_signature = None;
        }
        if let Some(wait) = next_transmit.checked_sub(local_micros()) {
            Timer::after_micros(wait).await;
        }
        let frame_start = Instant::now();
        let mut current = playback.lock().await;
        let frame_rate = current
            .as_ref()
            .map_or(IDLE_FRAME_RATE, |p| p.image.view().frame_rate());
        let signature = current
            .as_ref()
            .map(|p| (p.image_id, p.image_bytes, p.transport.generation));
        if signature != ready_signature {
            if let Some(p) = current.as_mut() {
                let rendered = p.render(ready_display_time);
                encode_frame(rendered, ready_buffer.as_mut_slice());
            } else {
                encode_frame(None, ready_buffer.as_mut_slice());
            }
        }
        let mut transfer = match output.send(ready_buffer) {
            Ok(transfer) => transfer,
            Err((error, _, _)) => panic!("I2S DMA start failed: {error:?}"),
        };
        drop(current);

        let model = *clock.lock().await;
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
        let mut active = playback.lock().await;
        let evaluation_start = Instant::now();
        EVALUATION_TASK.store(
            esp_radio_rtos_driver::current_task().as_ptr() as u32,
            Relaxed,
        );
        let rendered = if let Some(playback) = active.as_mut() {
            playback
                .transport
                .refresh(master_now, playback.image.view().duration().as_ticks());
            playback.render(next_latch)
        } else {
            None
        };
        EVALUATION_TASK.store(0, Relaxed);
        let evaluation_us = u32::try_from(evaluation_start.elapsed().as_micros()).unwrap();

        let encoding_start = Instant::now();
        encode_frame(rendered, spare_buffer.as_mut_slice());
        ready_signature = active
            .as_ref()
            .map(|p| (p.image_id, p.image_bytes, p.transport.generation));
        ready_display_time = next_latch;
        drop(active);
        let encoding_us = u32::try_from(encoding_start.elapsed().as_micros()).unwrap();
        render_budget = u64::from(evaluation_us + encoding_us) + 500;

        let wait_start = Instant::now();
        transfer.wait_for_done().await.unwrap();
        let (next_output, finished_buffer) = transfer.wait();
        output = next_output;
        ready_buffer = spare_buffer;
        spare_buffer = finished_buffer;
        let wait_us = u32::try_from(wait_start.elapsed().as_micros()).unwrap();

        let total_us = u32::try_from(frame_start.elapsed().as_micros()).unwrap();
        evaluation_sum += evaluation_us;
        evaluation_max = evaluation_max.max(evaluation_us);
        encoding_sum += encoding_us;
        encoding_max = encoding_max.max(encoding_us);
        wait_sum += wait_us;
        wait_max = wait_max.max(wait_us);
        total_sum += total_us;
        total_max = total_max.max(total_us);
        frames += 1;

        let frame_period_us = 1_000_000 / frame_rate;
        if total_us >= frame_period_us {
            missed += 1;
        }

        if frames >= frame_rate {
            println!(
                "PLAYBACK core={} frames={} missed={} eval_avg_us={} eval_max_us={} encode_avg_us={} encode_max_us={} dma_wait_avg_us={} dma_wait_max_us={} total_avg_us={} total_max_us={} heap_free={}",
                Cpu::current() as usize,
                frames,
                missed,
                evaluation_sum / frames,
                evaluation_max,
                encoding_sum / frames,
                encoding_max,
                wait_sum / frames,
                wait_max,
                total_sum / frames,
                total_max,
                esp_alloc::HEAP.free()
            );
            frames = 0;
            missed = 0;
            evaluation_sum = 0;
            evaluation_max = 0;
            encoding_sum = 0;
            encoding_max = 0;
            wait_sum = 0;
            wait_max = 0;
            total_sum = 0;
            total_max = 0;
        }
    }
}

/// Load the newest saved show, stopped. This runs after the network has
/// allocated its buffers, so releasing the show for an upload frees one
/// contiguous region instead of gaps between network buffers.
async fn restore_image(storage: &SharedStorage, playback: &SharedPlayback) {
    let Ok(_output_suspension) = storage::suspend_output().await else {
        println!("DONDER rendering core did not release flash access; starting without an image");
        return;
    };
    let mut storage = storage.lock().await;
    let restored = match show_slots::latest(&mut storage.shows()) {
        Ok(Some(slot)) => load(&mut storage, slot).ok(),
        Ok(None) => return,
        Err(_) => None,
    };
    // The editor uploads a current show on its next Play; the slot is
    // replaced then, not erased now.
    if restored.is_none() {
        println!(
            "DONDER saved image is unreadable or invalid for this firmware; starting without it"
        );
    }
    *playback.lock().await = restored;
}

async fn storage_error(uart: &mut Uart<'_, esp_hal::Async>, error: &'static str) -> ! {
    loop {
        let _ = uart_reply(uart, format_args!("DONDER ERROR {error}")).await;
        let mut command = [0];
        let _ = uart.read_exact(&mut command).await;
        if command[0] == b'R' {
            let _ = uart_reply(uart, format_args!("DONDER RESET UNAVAILABLE {error}")).await;
        }
    }
}

async fn erase_storage(
    uart: &mut Uart<'_, esp_hal::Async>,
    storage: &mut storage::DeviceStorage,
) -> ! {
    if donder_device_storage::erase_all(&mut storage.shows()).is_err()
        || donder_device_storage::erase_all(storage).is_err()
    {
        storage_error(
            uart,
            "Storage erase failed; reset the controller and retry erasing saved data",
        )
        .await;
    }
    let _ = uart_reply(uart, format_args!("DONDER RESET COMPLETE")).await;
    let _ = embedded_io_async::Write::flush(uart).await;
    esp_hal::system::software_reset();
}

async fn recover_storage(
    uart: &mut Uart<'_, esp_hal::Async>,
    storage: &mut storage::DeviceStorage,
    error: &'static str,
) -> ! {
    let mut erase_requested = false;
    let _ = uart_reply(uart, format_args!("DONDER ERROR {error}")).await;
    loop {
        let mut command = [0];
        if uart.read_exact(&mut command).await.is_err() {
            continue;
        }
        match command[0] {
            b'R' => {
                erase_requested = true;
                let _ = uart_reply(uart, format_args!("DONDER RESET READY")).await;
            }
            b'F' if erase_requested => erase_storage(uart, storage).await,
            _ => {
                erase_requested = false;
                let _ = uart_reply(uart, format_args!("DONDER ERROR {error}")).await;
            }
        }
    }
}

#[esp_rtos::main]
async fn main(spawner: embassy_executor::Spawner) -> ! {
    let mut p = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::max()));
    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 64 * 1024);
    // The heap holds the playing show's prepared sequence and playback
    // buffer beside the network's buffers; the rest of DRAM is the network
    // core's stack, which loading a show and
    // serving requests use deeply. Total heap with output enabled is 126 KiB.
    #[cfg(feature = "i2s-output")]
    esp_alloc::heap_allocator!(size: 62 * 1024);
    #[cfg(not(feature = "i2s-output"))]
    esp_alloc::heap_allocator!(size: 92 * 1024);
    let timer = TimerGroup::new(p.TIMG0);
    esp_rtos::start(timer.timer0, p.FROM_CPU_INTR0);

    EVALUATION_TASK.store(
        esp_radio_rtos_driver::current_task().as_ptr() as u32,
        Relaxed,
    );
    drop(core::hint::black_box(Box::new(42u32)));
    EVALUATION_TASK.store(0, Relaxed);
    assert_eq!(EVALUATION_ALLOCATIONS.load(Relaxed), 1);

    // USB serial is factory reset and diagnostics only. The host initiates the
    // handshake, so a damaged boot log cannot be mistaken for a failed boot.
    let mut uart = Uart::new(p.UART0.reborrow(), Config::default())
        .unwrap()
        .with_rx(p.GPIO3.reborrow())
        .with_tx(p.GPIO1.reborrow())
        .into_async();
    let mut storage = match storage::DeviceStorage::new(p.FLASH) {
        Ok(storage) => storage,
        Err(error) => storage_error(&mut uart, error).await,
    };
    if donder_device_storage::initialize(&mut storage).is_err() {
        recover_storage(
            &mut uart,
            &mut storage,
            "Cannot mount Donder storage; erase saved data to recover",
        )
        .await;
    }
    let config = match DeviceConfig::load(&mut storage) {
        Ok(saved) => saved.unwrap_or_else(network::default_config),
        Err(_) => {
            recover_storage(
                &mut uart,
                &mut storage,
                "Saved configuration is damaged; data was not erased",
            )
            .await
        }
    };
    // A reset gives the USB host a short window to request a factory reset.
    let mut command = [0];
    if matches!(
        embassy_time::with_timeout(Duration::from_secs(1), uart.read_exact(&mut command)).await,
        Ok(Ok(()))
    ) && command[0] == b'R'
    {
        let _ = uart_reply(&mut uart, format_args!("DONDER RESET READY")).await;
        // The host repeats R until it sees READY, then confirms with F.
        loop {
            match embassy_time::with_timeout(Duration::from_secs(2), uart.read_exact(&mut command))
                .await
            {
                Ok(Ok(())) if command[0] == b'F' => erase_storage(&mut uart, &mut storage).await,
                Ok(Ok(())) if command[0] == b'R' => {}
                _ => break,
            }
        }
    }
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
    #[cfg(feature = "i2s-output")]
    let boot_id = rng.random();
    #[cfg(feature = "i2s-output")]
    let clock: &'static SharedClock =
        picoserve::make_static!(SharedClock, Mutex::new(transport::Clock::new()));

    embedded_io_async::Write::flush(&mut uart).await.unwrap();
    #[cfg(feature = "dig-quad")]
    SERIAL_DIAGNOSTICS.store(false, Relaxed);
    // Disable the async UART interrupt before the LED peripheral takes its pins,
    // while retaining UART0's clock for ROM routines that use its transmitter.
    core::mem::forget(uart.into_blocking());

    // Start output before the network: joining a missing network must not
    // delay a restored show.
    #[cfg(feature = "i2s-output")]
    storage::output_started();

    #[cfg(feature = "i2s-output")]
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

    #[cfg(feature = "i2s-output")]
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
    restore_image(storage, playback).await;
    let app = picoserve::make_static!(
        AppRouter<WebApp>,
        WebApp {
            state: LoaderState {
                playback,
                upload,
                storage,
                #[cfg(feature = "i2s-output")]
                clock,
                #[cfg(feature = "i2s-output")]
                boot_id,
            }
        }
        .build_app()
    );
    for task_id in 0..HTTP_WORKERS {
        spawner.spawn(web_server(task_id, stack, app).unwrap());
    }
    #[cfg(feature = "i2s-output")]
    spawner.spawn(clock_server(stack, boot_id).unwrap());
    println!(
        "NETWORK mode={} id={} output={} heap_free={}",
        mode.label(),
        id,
        OUTPUT_DESCRIPTION,
        esp_alloc::HEAP.free()
    );

    loop {
        Timer::after_secs(60).await;
    }
}

#[cfg(feature = "i2s-output")]
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
