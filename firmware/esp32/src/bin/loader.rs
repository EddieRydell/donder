#![no_std]
#![no_main]
#![feature(impl_trait_in_assoc_type)]

extern crate alloc;
use tinyrlibc as _;

#[path = "../storage.rs"]
mod storage;
use dawn_device_storage::{Record, credentials::Credentials};
type SharedStorage = Mutex<CriticalSectionRawMutex, storage::DeviceStorage>;

#[cfg(feature = "i2s-output")]
#[path = "../ws281x_parallel.rs"]
mod ws281x_parallel;

use alloc::{boxed::Box, vec, vec::Vec};
#[cfg(feature = "i2s-output")]
use core::sync::atomic::AtomicBool;
use core::{
    fmt::Write as _,
    sync::atomic::{AtomicU32, Ordering::Relaxed},
};
#[cfg(feature = "i2s-output")]
use dawn_runtime::values::{MICROS_PER_SECOND, sample_time_from_frame};
use dawn_runtime::{
    sequence::{PreparedSequence, SequenceWorkspace},
    values::SampleTime,
    wire::{HEADER_BYTES, LoadError, LoadLimits, decode_sequence},
};
use embassy_net::StackResources;
use embassy_sync::{blocking_mutex::raw::CriticalSectionRawMutex, mutex::Mutex};
use embassy_time::{Duration, Timer};
use embedded_io_async::{Read as _, Write as _};
#[cfg(feature = "i2s-output")]
use esp_hal::{
    Async,
    dma::DmaTxBuf,
    gpio::NoPin,
    i2s::parallel::{I2sParallel, TxEightBits},
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
use esp_println::println;
use esp_radio::wifi::{self, AuthenticationMethodConfig, PowerSaveMode, sta::StationConfig};
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
static APP_CORE_STACK: StaticCell<Stack<4096>> = StaticCell::new();
#[cfg(feature = "i2s-output")]
static APP_CORE_EXECUTOR: StaticCell<esp_rtos::embassy::Executor> = StaticCell::new();

#[cfg(feature = "i2s-output")]
#[path = "../transport.rs"]
mod transport;

struct Playback {
    sequence: PreparedSequence,
    workspace: SequenceWorkspace,
    buffers: Vec<Vec<u8>>,
    #[cfg(feature = "i2s-output")]
    transport: transport::Transport,
    #[cfg(feature = "i2s-output")]
    frame_count: u32,
}

#[cfg(feature = "i2s-output")]
impl Playback {
    fn render(&mut self) {
        if self.transport.mode == transport::Mode::Stopped {
            for buffer in &mut self.buffers {
                buffer.fill(0);
            }
            return;
        }
        let frame = self.transport.advance(self.frame_count);
        let time = sample_time_from_frame(frame, OUTPUT_FRAME_RATE).unwrap();
        self.sequence
            .evaluate(time, &mut self.buffers, &mut self.workspace)
            .unwrap();
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
#[cfg(feature = "i2s-output")]
const OUTPUT_FRAME_RATE: u32 = 120;

#[cfg(feature = "i2s-output")]
type ParallelOutput = I2sParallel<'static, Async>;

const LIMITS: LoadLimits = LoadLimits {
    payload_bytes: 32 * 1024,
    pixels: 1600,
    graph_nodes: 128,
    workspace_bytes: 96 * 1024,
};

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

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    println!("LOADER PANIC: {}", info);
    loop {
        core::hint::spin_loop();
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

#[inline(never)]
fn load(bytes: &[u8]) -> Result<Playback, LoadError> {
    let archive_headroom = if cfg!(feature = "i2s-output") {
        Some(0)
    } else {
        bytes.len().checked_mul(8)
    }
    .ok_or(LoadError::Limit)?;
    let workspace_bytes = esp_alloc::HEAP
        .free()
        .saturating_sub(16 * 1024)
        .checked_sub(archive_headroom)
        .ok_or(LoadError::Limit)?;
    let limits = LoadLimits {
        workspace_bytes: LIMITS.workspace_bytes.min(workspace_bytes),
        ..LIMITS
    };
    let sequence = decode_sequence(bytes, limits)?;
    #[cfg(feature = "i2s-output")]
    if sequence.output_widths.is_empty()
        || sequence.output_widths.len() > OUTPUT_LANES
        || sequence
            .output_widths
            .iter()
            .any(|&width| width as usize > OUTPUT_PIXELS * 3 || width % 3 != 0)
    {
        return Err(LoadError::Limit);
    }
    let workspace = sequence.workspace();
    let buffers = sequence
        .output_widths
        .iter()
        .map(|&width| vec![0; width as usize])
        .collect();
    Ok(Playback {
        #[cfg(feature = "i2s-output")]
        frame_count: ((u64::from(sequence.signals.duration.as_ticks())
            * u64::from(OUTPUT_FRAME_RATE))
        .div_ceil(u64::from(MICROS_PER_SECOND)) as u32)
            .max(1),
        sequence,
        workspace,
        buffers,
        #[cfg(feature = "i2s-output")]
        transport: transport::Transport::new(),
    })
}

#[derive(Clone, Copy)]
struct LoaderState {
    playback: &'static SharedPlayback,
    upload: &'static UploadGate,
    storage: &'static SharedStorage,
    token: [u8; 32],
}

fn authorized(state: &LoaderState, request: &picoserve::request::RequestParts<'_>) -> bool {
    let Some(supplied) = request.headers().get("x-dawn-token") else {
        return false;
    };
    let supplied = supplied.as_raw();
    supplied.len() == state.token.len()
        && supplied
            .iter()
            .zip(state.token)
            .fold(0, |difference, (&left, right)| difference | (left ^ right))
            == 0
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct Capabilities {
    sequence_format: u32,
    max_payload_bytes: usize,
    max_pixels: usize,
    max_graph_nodes: usize,
    max_workspace_bytes: usize,
    output: OutputCapabilities,
    sequence_storage: SequenceStorage,
}

#[derive(serde::Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum OutputCapabilities {
    #[cfg(feature = "i2s-output")]
    Ws281x {
        lanes: usize,
        channels_per_lane: usize,
        channel_multiple: usize,
        frame_rate: u32,
    },
    #[cfg(not(feature = "i2s-output"))]
    EvaluationOnly,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
enum SequenceStorage {
    Persistent,
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
                "Missing or invalid X-Dawn-Token\n",
            )
                .write_to(request.body_connection.finalize().await?, response_writer)
                .await;
        }
        let capabilities = Capabilities {
            sequence_format: dawn_runtime::wire::FORMAT_VERSION,
            max_payload_bytes: LIMITS.payload_bytes,
            max_pixels: LIMITS.pixels,
            max_graph_nodes: LIMITS.graph_nodes,
            max_workspace_bytes: LIMITS.workspace_bytes,
            #[cfg(feature = "i2s-output")]
            output: OutputCapabilities::Ws281x {
                lanes: OUTPUT_LANES,
                channels_per_lane: OUTPUT_PIXELS * 3,
                channel_multiple: 3,
                frame_rate: OUTPUT_FRAME_RATE,
            },
            #[cfg(not(feature = "i2s-output"))]
            output: OutputCapabilities::EvaluationOnly,
            sequence_storage: SequenceStorage::Persistent,
        };
        Json(capabilities)
            .into_response()
            .with_header("Cache-Control", "no-store")
            .write_to(request.body_connection.finalize().await?, response_writer)
            .await
    }
}

struct UploadSequence;

impl RequestHandlerService<LoaderState> for UploadSequence {
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
                "Missing or invalid X-Dawn-Token\n",
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
        if !(HEADER_BYTES..=HEADER_BYTES + LIMITS.payload_bytes).contains(&length) {
            return (
                StatusCode::PAYLOAD_TOO_LARGE,
                "Sequence exceeds device limits\n",
            )
                .write_to(request.body_connection.finalize().await?, response_writer)
                .await;
        }

        let mut bytes = Vec::new();
        if bytes.try_reserve_exact(length).is_err() {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                "Insufficient upload memory\n",
            )
                .write_to(request.body_connection.finalize().await?, response_writer)
                .await;
        }
        bytes.resize(length, 0);

        let offset = {
            let mut reader = request
                .body_connection
                .body()
                .reader()
                .with_different_timeout(Duration::from_secs(15));
            let mut offset = 0;
            while offset < bytes.len() {
                let read = reader.read(&mut bytes[offset..]).await?;
                if read == 0 {
                    break;
                }
                offset += read;
            }
            offset
        };
        let connection = request.body_connection.finalize().await?;
        if offset != bytes.len() {
            return (StatusCode::BAD_REQUEST, "Incomplete sequence body\n")
                .write_to(connection, response_writer)
                .await;
        }

        let free = esp_alloc::HEAP.free();
        let start = Instant::now();
        match load(&bytes) {
            Ok(playback) => {
                let pixels = playback.sequence.signals.pixel_count;
                let heap = free.saturating_sub(esp_alloc::HEAP.free());
                let elapsed = start.elapsed().as_micros();
                if dawn_device_storage::write(
                    &mut *state.storage.lock().await,
                    Record::Sequence,
                    &bytes,
                )
                .is_err()
                {
                    return (StatusCode::INTERNAL_SERVER_ERROR, "Could not save sequence to flash; running playback retained. Check device before retrying.\n")
                        .write_to(connection, response_writer).await;
                }
                *state.playback.lock().await = Some(playback);
                (
                    StatusCode::OK,
                    format_args!(
                        "LOADED bytes={} pixels={} heap={} us={}\n",
                        bytes.len(),
                        pixels,
                        heap,
                        elapsed
                    ),
                )
                    .write_to(connection, response_writer)
                    .await
            }
            Err(error) => {
                (
                    StatusCode::UNPROCESSABLE_ENTITY,
                    format_args!("REJECT {:?}\n", error),
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
                "Missing or invalid X-Dawn-Token\n",
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
        let Some(Playback {
            sequence,
            workspace,
            buffers,
            ..
        }) = active.as_mut()
        else {
            drop(active);
            return (StatusCode::CONFLICT, "REJECT NoSequence\n")
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
        let result = sequence.evaluate(SampleTime::from_ticks(ticks), buffers, workspace);
        let elapsed = start.elapsed().as_micros();
        EVALUATION_TASK.store(0, Relaxed);
        let evaluation_allocations = EVALUATION_ALLOCATIONS.load(Relaxed) - evaluation_allocations;
        let allocations = ALLOCATIONS.load(Relaxed) - allocations;

        if result.is_err() {
            drop(active);
            return (StatusCode::INTERNAL_SERVER_ERROR, "REJECT Evaluation\n")
                .write_to(connection, response_writer)
                .await;
        }

        let mut crc = crc32fast::Hasher::new();
        for buffer in buffers {
            crc.update(buffer);
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
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct PlaybackStatus {
    mode: transport::Mode,
    position_micros: u32,
    duration_micros: u32,
}

#[cfg(feature = "i2s-output")]
#[derive(serde::Serialize)]
struct TransportStatus {
    playback: Option<PlaybackStatus>,
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
                "Missing or invalid X-Dawn-Token\n",
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
        let mut active = state.playback.lock().await;
        if let Some(mode) = self.0 {
            let Some(playback) = active.as_mut() else {
                drop(active);
                return (StatusCode::CONFLICT, "No sequence is loaded\n")
                    .write_to(connection, response_writer)
                    .await;
            };
            playback.transport.set_mode(mode);
        }
        let status = TransportStatus {
            playback: active.as_ref().map(|playback| PlaybackStatus {
                mode: playback.transport.mode,
                position_micros: sample_time_from_frame(
                    playback.transport.frame(),
                    OUTPUT_FRAME_RATE,
                )
                .unwrap()
                .as_ticks(),
                duration_micros: playback.sequence.signals.duration.as_ticks(),
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
            .route("/capabilities", get_service(DeviceCapabilities))
            .route("/sequence", put_service(UploadSequence))
            .route("/frame", post_service(EvaluateFrame));
        #[cfg(feature = "i2s-output")]
        let router = router
            .route("/transport", get_service(DeviceTransport(None)))
            .route(
                "/transport/play",
                post_service(DeviceTransport(Some(transport::Mode::Playing))),
            )
            .route(
                "/transport/pause",
                post_service(DeviceTransport(Some(transport::Mode::Paused))),
            )
            .route(
                "/transport/stop",
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
async fn network(mut runner: embassy_net::Runner<'static, wifi::Interface>) {
    runner.run().await;
}

#[embassy_executor::task]
async fn reconnect(mut controller: wifi::WifiController<'static>) {
    loop {
        if controller.is_connected() {
            let _ = controller.wait_for_disconnect_async().await;
            println!("WIFI DISCONNECTED");
        }
        match controller.connect_async().await {
            Ok(_) => println!("WIFI CONNECTED"),
            Err(_) => {
                println!("WIFI RETRY");
                Timer::after_secs(5).await;
            }
        }
    }
}

#[embassy_executor::task(pool_size = HTTP_WORKERS)]
async fn web_server(
    task_id: usize,
    stack: embassy_net::Stack<'static>,
    app: &'static AppRouter<WebApp>,
) -> ! {
    let mut tcp_rx = [0; 4096];
    let mut tcp_tx = [0; 1024];
    let mut http = [0; 2048];
    picoserve::Server::new(app, &SERVER_CONFIG, &mut http)
        .listen_and_serve(task_id, stack, HTTP_PORT, &mut tcp_rx, &mut tcp_tx)
        .await
        .into_never()
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
async fn render_outputs(
    playback: &'static SharedPlayback,
    mut output: ParallelOutput,
    mut ready_buffer: DmaTxBuf,
    mut spare_buffer: DmaTxBuf,
) -> ! {
    loop {
        let mut active = playback.lock().await;
        let Some(playback) = active.as_mut() else {
            drop(active);
            Timer::after_millis(10).await;
            continue;
        };
        playback.render();
        ws281x_parallel::encode(
            &playback.buffers,
            OUTPUT_PIXELS,
            ready_buffer.as_mut_slice(),
        );
        break;
    }

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
    loop {
        let frame_start = Instant::now();
        let mut transfer = match output.send(ready_buffer) {
            Ok(transfer) => transfer,
            Err((error, _, _)) => panic!("I2S DMA start failed: {error:?}"),
        };

        let mut active = playback.lock().await;
        let playback = active.as_mut().unwrap();

        let evaluation_start = Instant::now();
        EVALUATION_TASK.store(
            esp_radio_rtos_driver::current_task().as_ptr() as u32,
            Relaxed,
        );
        playback.render();
        EVALUATION_TASK.store(0, Relaxed);
        let evaluation_us = u32::try_from(evaluation_start.elapsed().as_micros()).unwrap();

        let encoding_start = Instant::now();
        ws281x_parallel::encode(
            &playback.buffers,
            OUTPUT_PIXELS,
            spare_buffer.as_mut_slice(),
        );
        drop(active);
        let encoding_us = u32::try_from(encoding_start.elapsed().as_micros()).unwrap();

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

        let frame_period_us = 1_000_000 / OUTPUT_FRAME_RATE;
        if total_us >= frame_period_us {
            missed += 1;
        } else {
            Timer::after_micros(u64::from(frame_period_us - total_us)).await;
        }

        if frames == OUTPUT_FRAME_RATE {
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

async fn storage_error(uart: &mut Uart<'_, esp_hal::Async>, error: &'static str) -> ! {
    loop {
        let _ = uart_reply(uart, format_args!("DAWN ERROR {error}")).await;
        let mut command = [0];
        let _ = uart.read_exact(&mut command).await;
        if command[0] == b'R' {
            let _ = uart_reply(uart, format_args!("DAWN RESET UNAVAILABLE {error}")).await;
        }
    }
}

async fn erase_storage(
    uart: &mut Uart<'_, esp_hal::Async>,
    storage: &mut storage::DeviceStorage,
) -> ! {
    if dawn_device_storage::erase_all(storage).is_err() {
        storage_error(
            uart,
            "Storage erase failed; reset the controller and retry erasing saved data",
        )
        .await;
    }
    let _ = uart_reply(uart, format_args!("DAWN RESET COMPLETE")).await;
    let _ = embedded_io_async::Write::flush(uart).await;
    esp_hal::system::software_reset();
}

async fn recover_storage(
    uart: &mut Uart<'_, esp_hal::Async>,
    storage: &mut storage::DeviceStorage,
    error: &'static str,
) -> ! {
    let mut erase_requested = false;
    let _ = uart_reply(uart, format_args!("DAWN ERROR {error}")).await;
    loop {
        let mut command = [0];
        if uart.read_exact(&mut command).await.is_err() {
            continue;
        }
        match command[0] {
            b'R' => {
                erase_requested = true;
                let _ = uart_reply(uart, format_args!("DAWN RESET READY")).await;
            }
            b'F' if erase_requested => erase_storage(uart, storage).await,
            _ => {
                erase_requested = false;
                let _ = uart_reply(uart, format_args!("DAWN ERROR {error}")).await;
            }
        }
    }
}

#[esp_rtos::main]
async fn main(spawner: embassy_executor::Spawner) -> ! {
    let p = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::max()));
    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 64 * 1024);
    esp_alloc::heap_allocator!(size: 96 * 1024);
    let timer = TimerGroup::new(p.TIMG0);
    esp_rtos::start(timer.timer0, p.FROM_CPU_INTR0);

    EVALUATION_TASK.store(
        esp_radio_rtos_driver::current_task().as_ptr() as u32,
        Relaxed,
    );
    drop(core::hint::black_box(Box::new(42u32)));
    EVALUATION_TASK.store(0, Relaxed);
    assert_eq!(EVALUATION_ALLOCATIONS.load(Relaxed), 1);

    // USB serial is provisioning and diagnostics only. The host initiates the
    // handshake, so a damaged boot log cannot be mistaken for a failed boot.
    let mut uart = Uart::new(p.UART0, Config::default())
        .unwrap()
        .with_rx(p.GPIO3)
        .with_tx(p.GPIO1)
        .into_async();
    let mut storage = match storage::DeviceStorage::new(p.FLASH) {
        Ok(storage) => storage,
        Err(error) => storage_error(&mut uart, error).await,
    };
    if dawn_device_storage::initialize(&mut storage).is_err() {
        recover_storage(
            &mut uart,
            &mut storage,
            "Cannot mount Dawn storage; erase saved data to recover",
        )
        .await;
    }
    let saved = match Credentials::load(&mut storage) {
        Ok(saved) => saved,
        Err(_) => {
            recover_storage(
                &mut uart,
                &mut storage,
                "Saved credentials are damaged; data was not erased",
            )
            .await
        }
    };
    // A reset gives the USB provisioner a short window to request new credentials.
    // Otherwise a configured controller boots without waiting for a computer.
    let mut command = [0];
    let provision = saved.is_none()
        || matches!(
            embassy_time::with_timeout(Duration::from_secs(3), uart.read_exact(&mut command)).await,
            Ok(Ok(()))
        ) && matches!(command[0], b'P' | b'R');
    let rng = Rng::new();
    let credentials = if provision {
        let mut erase_requested = false;
        loop {
            if command[0] == b'P' {
                erase_requested = false;
                let _ = uart_reply(&mut uart, format_args!("DAWN PROVISION READY")).await;
            }
            if command[0] == b'R' {
                erase_requested = true;
                let _ = uart_reply(&mut uart, format_args!("DAWN RESET READY")).await;
            }
            if command[0] == b'F' && erase_requested {
                erase_storage(&mut uart, &mut storage).await;
            }
            if uart.read_exact(&mut command).await.is_err() {
                continue;
            }
            if command[0] == b'W' {
                break;
            }
        }
        let mut lengths = [0; 2];
        if uart.read_exact(&mut lengths).await.is_err()
            || !(1..=32).contains(&lengths[0])
            || !(8..=64).contains(&lengths[1])
        {
            storage_error(&mut uart, "Invalid Wi-Fi credential lengths").await;
        }
        let mut bytes = [0; 96];
        let split = usize::from(lengths[0]);
        let length = split + usize::from(lengths[1]);
        if uart.read_exact(&mut bytes[..length]).await.is_err() {
            storage_error(&mut uart, "Incomplete Wi-Fi credentials").await;
        }
        let Ok(ssid) = core::str::from_utf8(&bytes[..split]) else {
            storage_error(&mut uart, "Invalid Wi-Fi network encoding").await;
        };
        let Ok(password) = core::str::from_utf8(&bytes[split..length]) else {
            storage_error(&mut uart, "Invalid Wi-Fi password encoding").await;
        };
        let mut raw_token = [0; 16];
        for word in raw_token.chunks_exact_mut(4) {
            word.copy_from_slice(&rng.random().to_le_bytes());
        }
        let credentials = Credentials {
            ssid: ssid.into(),
            password: password.into(),
            token: token_ascii(raw_token),
        };
        raw_token.fill(0);
        bytes.fill(0);
        credentials
    } else {
        saved.unwrap()
    };
    let token = credentials.token;
    let config = StationConfig::default()
        .with_ssid(credentials.ssid.as_str().try_into().unwrap())
        .with_authentication(AuthenticationMethodConfig::Wpa2Personal(
            credentials.password.as_str().try_into().unwrap(),
        ));
    let interface = wifi::Interface::station();
    let mut controller = wifi::WifiController::new(
        p.WIFI,
        wifi::ControllerConfig::default().with_initial_config(wifi::Config::Station(config)),
    )
    .unwrap();
    controller.set_power_saving(PowerSaveMode::None).unwrap();

    let seed = (u64::from(rng.random()) << 32) | u64::from(rng.random());
    uart.write_all(b"TOKEN ").await.unwrap();
    uart.write_all(&token).await.unwrap();
    uart.write_all(b"\n").await.unwrap();

    let resources = Box::leak(Box::new(StackResources::<3>::new()));
    let (stack, runner) = embassy_net::new(
        interface,
        embassy_net::Config::dhcpv4(Default::default()),
        resources,
        seed,
    );
    spawner.spawn(network(runner).unwrap());
    spawner.spawn(reconnect(controller).unwrap());
    let restored = match dawn_device_storage::read(
        &mut storage,
        Record::Sequence,
        HEADER_BYTES + LIMITS.payload_bytes,
    ) {
        Ok(Some(bytes)) => match load(&bytes) {
            Ok(playback) => Some(playback),
            Err(_) => {
                storage_error(
                    &mut uart,
                    "Saved sequence is invalid for this firmware; data was not erased",
                )
                .await
            }
        },
        Ok(None) => None,
        Err(_) => storage_error(&mut uart, "Cannot read saved sequence; data was not erased").await,
    };
    let storage: &'static SharedStorage =
        picoserve::make_static!(SharedStorage, Mutex::new(storage));
    let playback: &'static SharedPlayback =
        picoserve::make_static!(SharedPlayback, Mutex::new(restored));
    let upload = picoserve::make_static!(UploadGate, Mutex::new(()));
    let app = picoserve::make_static!(
        AppRouter<WebApp>,
        WebApp {
            state: LoaderState {
                playback,
                upload,
                storage,
                token
            }
        }
        .build_app()
    );
    for task_id in 0..HTTP_WORKERS {
        spawner.spawn(web_server(task_id, stack, app).unwrap());
    }

    #[cfg(feature = "i2s-output")]
    esp_rtos::start_second_core(
        p.CPU_CTRL,
        p.FROM_CPU_INTR1,
        APP_CORE_STACK.init(Stack::new()),
        move || {
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
                        render_outputs(playback, output, ready_buffer, spare_buffer).unwrap(),
                    );
                });
        },
    );

    #[cfg(feature = "i2s-output")]
    while !OUTPUT_READY.load(Relaxed) {
        Timer::after_millis(1).await;
    }

    stack.wait_config_up().await;
    if provision && credentials.save(&mut *storage.lock().await).is_err() {
        storage_error(
            &mut uart,
            "Wi-Fi connected but credentials could not be saved; retry provisioning",
        )
        .await;
    }
    drop(credentials);

    uart_reply(
        &mut uart,
        format_args!(
            "WIFI READY {} {} i2s={} heap_free={}",
            stack.config_v4().unwrap().address.address(),
            HTTP_PORT,
            if cfg!(feature = "i2s-output") {
                "gpio13,18,21,25"
            } else {
                "off"
            },
            esp_alloc::HEAP.free()
        ),
    )
    .await
    .unwrap();

    loop {
        Timer::after_secs(60).await;
    }
}
