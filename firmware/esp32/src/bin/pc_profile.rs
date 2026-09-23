//! Statistical interrupted-PC sampling. No shared-runtime instrumentation.
#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec;
use core::sync::atomic::{AtomicU32, Ordering::Relaxed};
use esp_hal::{
    clock::CpuClock,
    time::{Duration, Instant},
    timer::{Timer as _, timg::TimerGroup},
    trapframe::TrapFrame,
};
use esp_println::println;

#[path = "../workload.rs"]
#[allow(dead_code)]
mod workload;
#[allow(unused_imports, dead_code)] // Shared generated fixtures include other harness cases.
mod fixtures {
    include!(concat!(env!("OUT_DIR"), "/fixtures.rs"));
}

esp_bootloader_esp_idf::esp_app_desc!();

static COUNT: AtomicU32 = AtomicU32::new(0);
static ALLOCATIONS: AtomicU32 = AtomicU32::new(0);

#[unsafe(no_mangle)]
fn _esp_alloc_alloc(
    _: &esp_alloc::EspHeap,
    _: esp_alloc::export::enumset::EnumSet<esp_alloc::MemoryCapability>,
    pointer: usize,
    _: usize,
) {
    if pointer != 0 {
        ALLOCATIONS.fetch_add(1, Relaxed);
    }
}

#[unsafe(no_mangle)]
fn _esp_alloc_dealloc(_: &esp_alloc::EspHeap, _: usize, _: usize) {}

static PCS: [AtomicU32; 4096] = [const { AtomicU32::new(0) }; 4096];
// Installed before interrupts are enabled, never replaced, single-core firmware.
// Both contexts only borrow Timer immutably; register methods use shared access.
static mut TIMER: Option<esp_hal::timer::timg::Timer<'static>> = None;

fn timer() -> &'static esp_hal::timer::timg::Timer<'static> {
    // SAFETY: main initializes TIMER once before calling this or enabling its ISR.
    unsafe { (&*core::ptr::addr_of!(TIMER)).as_ref().unwrap() }
}

#[esp_hal::ram]
extern "C" fn sample(context: &mut TrapFrame) {
    timer().clear_interrupt();
    let index = COUNT.fetch_add(1, Relaxed) as usize;
    if let Some(slot) = PCS.get(index) {
        slot.store(context.PC, Relaxed);
    }
}

#[panic_handler]
fn panic(info: &core::panic::PanicInfo) -> ! {
    println!("PROFILE PANIC: {}", info);
    loop {
        core::hint::spin_loop();
    }
}

#[esp_hal::main]
fn main() -> ! {
    let peripherals = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::max()));
    esp_alloc::heap_allocator!(size: 96 * 1024);
    let group = TimerGroup::new(peripherals.TIMG0);
    // SAFETY: no handler is enabled yet and this is the sole initialization.
    unsafe { TIMER = Some(group.timer0) };
    // SAFETY: the pinned esp-hal Xtensa dispatcher calls peripheral handlers with
    // &mut TrapFrame, although InterruptHandler stores a zero-argument pointer.
    // Its handler macro currently fails to erase this signature itself.
    let handler =
        unsafe { core::mem::transmute::<extern "C" fn(&mut TrapFrame), extern "C" fn()>(sample) };
    timer().set_interrupt_handler(esp_hal::interrupt::InterruptHandler::new(
        handler,
        esp_hal::interrupt::Priority::Priority1,
    ));
    timer().enable_auto_reload(true);
    let attach = Instant::now();
    while attach.elapsed().as_millis() < 3000 {}

    println!("DONDER PC BEGIN cpu_mhz=240 core=1 stack_samples=false");
    for case in 0..fixtures::NAMES.len()
        + workload::CHASE_PULSE_CASES.len()
        + workload::MARK_CASES.len()
        + fixtures::GENERATOR_NAMES.len()
    {
        let (name, show, golden) = if case < fixtures::NAMES.len() {
            let (program, params) = fixtures::case(case);
            (
                fixtures::NAMES[case],
                workload::show(200, program, params),
                &fixtures::GOLDEN[case][0],
            )
        } else if case < fixtures::NAMES.len() + workload::CHASE_PULSE_CASES.len() {
            let index = case - fixtures::NAMES.len();
            let (name, layers) = workload::CHASE_PULSE_CASES[index];
            let (program, _) = fixtures::case(workload::GAMMA_CASE);
            (
                name,
                workload::chase_pulse_show(200, layers, program),
                &fixtures::CHASE_PULSE_GOLDEN[index],
            )
        } else if case
            < fixtures::NAMES.len() + workload::CHASE_PULSE_CASES.len() + workload::MARK_CASES.len()
        {
            let index = case - fixtures::NAMES.len() - workload::CHASE_PULSE_CASES.len();
            let (name, _, _) = workload::MARK_CASES[index];
            (
                name,
                donder_runtime::wire::decode_sequence(
                    fixtures::MARK_SEQUENCES[index],
                    Default::default(),
                )
                .unwrap(),
                &fixtures::MARK_GOLDEN[index],
            )
        } else {
            let index = case
                - fixtures::NAMES.len()
                - workload::CHASE_PULSE_CASES.len()
                - workload::MARK_CASES.len();
            (
                fixtures::GENERATOR_NAMES[index],
                donder_runtime::wire::decode_sequence(
                    fixtures::GENERATOR_SEQUENCES[index],
                    Default::default(),
                )
                .unwrap(),
                &fixtures::GENERATOR_GOLDEN[index],
            )
        };
        let mut workspace = show.workspace();
        let mut output = [vec![0; 600]];
        show.evaluate(workload::time(0), &mut output, &mut workspace)
            .unwrap();
        // Baseline and two sampling periods in one image. Report cycles of work,
        // not only sample counts; periodic aliasing and ISR overhead remain visible.
        for period_us in [0, 997, 1999, 0] {
            COUNT.store(0, Relaxed);
            let allocations = ALLOCATIONS.load(Relaxed);
            if period_us != 0 {
                timer().reset();
                timer()
                    .load_value(Duration::from_micros(period_us))
                    .unwrap();
                timer().clear_interrupt();
                timer().enable_interrupt(true);
                timer().start();
            }
            let mut frames = 0u32;
            let start = Instant::now();
            while start.elapsed().as_millis() < 2000 {
                // Every window has the same mixture of show times, including
                // slow mark frames. Read the wall clock only between full cycles.
                for frame in 0..workload::FRAMES {
                    show.evaluate(workload::time(frame), &mut output, &mut workspace)
                        .unwrap();
                    core::hint::black_box(&output);
                    frames += 1;
                }
            }
            let elapsed_us = start.elapsed().as_micros();
            timer().enable_interrupt(false);
            timer().stop();
            timer().clear_interrupt();
            assert_eq!(
                ALLOCATIONS.load(Relaxed),
                allocations,
                "profiled frame allocation"
            );
            // Outside timing/sampling: check the frame actually produced while
            // the ISR was active, rather than merely replaying with it disabled.
            assert_eq!(
                workload::checksum(&output[0]),
                golden[(frames as usize - 1) % workload::FRAMES],
                "profiled frame differs from host output"
            );
            let samples = COUNT.load(Relaxed) as usize;
            assert!(samples <= PCS.len(), "PC sample storage exhausted");
            println!(
                "PC CASE effect={} period_us={} frames={} elapsed_us={} samples={}",
                name, period_us, frames, elapsed_us, samples
            );
            for slot in &PCS[..samples] {
                println!("PC {:08x}", slot.load(Relaxed));
            }
        }
    }
    println!("DONDER PC END");
    loop {
        core::hint::spin_loop();
    }
}
