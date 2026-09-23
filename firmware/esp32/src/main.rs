#![no_std]
#![no_main]

extern crate alloc;

use alloc::vec;
use core::hint::black_box;
use core::sync::atomic::{AtomicU32, Ordering::Relaxed};
use donder_runtime::dsl::VmWorkspace;
use esp_hal::{clock::CpuClock, time::Instant};
use esp_println::println;

mod workload;
#[allow(unused_imports)]
mod fixtures {
    include!(concat!(env!("OUT_DIR"), "/fixtures.rs"));
}

esp_bootloader_esp_idf::esp_app_desc!();

static ALLOCATIONS: AtomicU32 = AtomicU32::new(0);
static REQUESTED_LIVE: AtomicU32 = AtomicU32::new(0);
static REQUESTED_PEAK: AtomicU32 = AtomicU32::new(0);

// The allocator's supported hook counts successful allocation calls. No custom
// allocator and no logging in the measured path. The other core is not started.
#[unsafe(no_mangle)]
fn _esp_alloc_alloc(
    _: &esp_alloc::EspHeap,
    _: esp_alloc::export::enumset::EnumSet<esp_alloc::MemoryCapability>,
    pointer: usize,
    size: usize,
) {
    if pointer != 0 {
        ALLOCATIONS.fetch_add(1, Relaxed);
        let live = REQUESTED_LIVE.fetch_add(size as u32, Relaxed) + size as u32;
        REQUESTED_PEAK.fetch_max(live, Relaxed);
    }
}

#[unsafe(no_mangle)]
fn _esp_alloc_dealloc(_: &esp_alloc::EspHeap, pointer: usize, size: usize) {
    if pointer != 0 {
        REQUESTED_LIVE.fetch_sub(size as u32, Relaxed);
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
    let _peripherals = esp_hal::init(esp_hal::Config::default().with_cpu_clock(CpuClock::max()));
    esp_alloc::heap_allocator!(#[esp_hal::ram(reclaimed)] size: 64 * 1024);
    esp_alloc::heap_allocator!(size: 96 * 1024);
    // Let the serial monitor attach after the flash/reset operation.
    let start = Instant::now();
    while start.elapsed().as_millis() < 3000 {}
    println!(
        "DONDER PROFILE BEGIN cpu_mhz={} cores=1 wifi=off gpio=untouched frames={}",
        esp_hal::clock::cpu_clock().as_mhz(),
        workload::FRAMES
    );
    println!(
        "heap_total={} heap_free={}",
        esp_alloc::HEAP.used() + esp_alloc::HEAP.free(),
        esp_alloc::HEAP.free()
    );
    for (case, name) in fixtures::NAMES.iter().enumerate() {
        for (count_index, count) in workload::COUNTS.into_iter().enumerate() {
            let heap_before = esp_alloc::HEAP.used();
            REQUESTED_PEAK.store(REQUESTED_LIVE.load(Relaxed), Relaxed);
            let setup_start = Instant::now();
            let (program, params) = fixtures::case(case);
            let mut vm = VmWorkspace::default();
            let mut bytes = vec![0; count * 3];
            let setup_us = setup_start.elapsed().as_micros() as u32;
            let mut render = |frame| {
                for pixel in 0..count {
                    let color = program
                        .sample_effect(&params, &workload::context(count, pixel, frame), &mut vm)
                        .unwrap();
                    bytes[pixel * 3..pixel * 3 + 3].copy_from_slice(&[
                        color.green,
                        color.red,
                        color.blue,
                    ]);
                }
                black_box(&bytes);
            };
            let first_start = Instant::now();
            let first_allocations = ALLOCATIONS.load(Relaxed);
            render(0);
            let first_us = first_start.elapsed().as_micros() as u32;
            let first_allocations = ALLOCATIONS.load(Relaxed) - first_allocations;
            let mut times = [0_u32; workload::FRAMES];
            let allocations_before = ALLOCATIONS.load(Relaxed);
            for (frame, elapsed) in times.iter_mut().enumerate() {
                let start = Instant::now();
                render(frame);
                *elapsed = start.elapsed().as_micros() as u32;
            }
            let allocations = ALLOCATIONS.load(Relaxed) - allocations_before;
            // Verification is a separate untimed pass over all measured frames.
            let mut mismatches = 0;
            for frame in 0..workload::FRAMES {
                for pixel in 0..count {
                    let color = program
                        .sample_effect(&params, &workload::context(count, pixel, frame), &mut vm)
                        .unwrap();
                    bytes[pixel * 3..pixel * 3 + 3].copy_from_slice(&[
                        color.green,
                        color.red,
                        color.blue,
                    ]);
                }
                mismatches += usize::from(
                    workload::checksum(&bytes) != fixtures::GOLDEN[case][count_index][frame],
                );
            }
            report(
                "vm",
                name,
                count,
                setup_us,
                esp_alloc::HEAP.used() - heap_before,
                allocations,
                mismatches,
                &mut times,
            );
            println!(
                "first_frame_us={} first_alloc_calls={} peak_requested_bytes={} heap_free={}",
                first_us,
                first_allocations,
                REQUESTED_PEAK.load(Relaxed),
                esp_alloc::HEAP.free()
            );
            drop((program, params, vm, bytes));
            assert_eq!(esp_alloc::HEAP.used(), heap_before, "VM case leaked heap");

            #[derive(Clone, Copy)]
            enum OperatorCase {
                Full,
                Reuse,
                Grouped,
                Alternating,
                Nested(usize, bool),
                Mixed,
                Uniform(bool),
            }
            use OperatorCase::{Alternating, Full, Grouped, Mixed, Nested, Reuse, Uniform};
            for (stage, layers, gamma, operator, native) in [
                ("show", 1, None, None, None),
                ("layers4", 4, None, None, None),
                ("layers16", 16, None, None, None),
                ("gamma_lookup", 1, Some(true), None, None),
                ("operator_full", 1, None, Some(Full), None),
                ("operator_reuse", 1, None, Some(Reuse), None),
                ("temporal_grouped", 1, None, Some(Grouped), None),
                ("temporal_alternating", 1, None, Some(Alternating), None),
                ("native_automation", 1, None, None, Some(false)),
                ("empty_automation", 1, None, None, Some(true)),
                ("mixed_native", 1, None, Some(Mixed), None),
                ("nested2", 1, None, Some(Nested(0, true)), None),
                ("nested4", 1, None, Some(Nested(1, true)), None),
                ("nested8", 1, None, Some(Nested(2, true)), None),
                ("nested4_full", 1, None, Some(Nested(1, false)), None),
                ("nested8_full", 1, None, Some(Nested(2, false)), None),
                ("uniform_full", 1, None, Some(Uniform(false)), None),
                ("uniform_reuse", 1, None, Some(Uniform(true)), None),
            ] {
                if matches!(operator, Some(Uniform(_))) {
                    if case != 4 {
                        continue;
                    }
                } else if (gamma.is_some() || operator.is_some() || native.is_some())
                    && case != workload::GAMMA_CASE
                {
                    continue;
                }
                if !(4..8).contains(&case) && layers != 1 {
                    continue;
                }
                REQUESTED_PEAK.store(REQUESTED_LIVE.load(Relaxed), Relaxed);
                let setup_start = Instant::now();
                let (program, params) = fixtures::case(case);
                let mut show = if case < 4 {
                    workload::show(count, program, params)
                } else {
                    workload::layered_show(count, program, params, layers)
                };
                if gamma.is_some() {
                    workload::apply_gamma(&mut show, fixtures::GAMMA_LOOKUP);
                }
                if let Some(kind) = operator {
                    let program = match kind {
                        Full | Reuse | Nested(_, _) => fixtures::operator_program(),
                        Grouped => fixtures::grouped_program(),
                        Alternating => fixtures::alternating_program(),
                        Mixed | Uniform(_) => fixtures::identity_program(),
                    };
                    if let Uniform(reuse) = kind {
                        assert!(!show.signals.programs[0].uses_pixel_context);
                        show.signals.programs[0].uses_pixel_context = !reuse;
                    }
                    workload::apply_operator(
                        &mut show,
                        program,
                        !matches!(kind, Full | Nested(_, false)),
                    );
                    if let Nested(index, _) = kind {
                        workload::nest_operator(&mut show, workload::OPERATOR_DEPTHS[index]);
                    } else if matches!(kind, Mixed) {
                        workload::insert_native_invert(&mut show);
                    }
                }
                if let Some(empty) = native {
                    workload::apply_native_automation(&mut show, empty);
                }
                let golden = if let Some(empty) = native {
                    if empty {
                        &fixtures::EMPTY_GOLDEN[count_index]
                    } else {
                        &fixtures::NATIVE_GOLDEN[count_index]
                    }
                } else if let Some(kind) = operator {
                    match kind {
                        Full | Reuse => &fixtures::OPERATOR_GOLDEN[count_index],
                        Grouped | Alternating => &fixtures::TEMPORAL_GOLDEN[count_index],
                        Nested(index, _) => &fixtures::NESTED_GOLDEN[count_index][index],
                        Mixed => &fixtures::MIXED_GOLDEN[count_index],
                        Uniform(_) => &fixtures::GOLDEN[case][count_index],
                    }
                } else if gamma.is_some() {
                    &fixtures::GAMMA_GOLDEN[count_index]
                } else {
                    &fixtures::GOLDEN[case][count_index]
                };
                let mut workspace = show.workspace();
                let mut buffers = [vec![0; count * 3]];
                let setup_us = setup_start.elapsed().as_micros() as u32;
                let first_start = Instant::now();
                let first_allocations = ALLOCATIONS.load(Relaxed);
                show.evaluate(workload::time(0), &mut buffers, &mut workspace)
                    .unwrap();
                let first_us = first_start.elapsed().as_micros() as u32;
                let first_allocations = ALLOCATIONS.load(Relaxed) - first_allocations;
                let mut mismatches = 0;
                let allocations_before = ALLOCATIONS.load(Relaxed);
                for (frame, elapsed) in times.iter_mut().enumerate() {
                    let start = Instant::now();
                    show.evaluate(workload::time(frame), &mut buffers, &mut workspace)
                        .unwrap();
                    black_box(&buffers);
                    *elapsed = start.elapsed().as_micros() as u32;
                    mismatches += usize::from(workload::checksum(&buffers[0]) != golden[frame]);
                }
                report(
                    stage,
                    name,
                    count,
                    setup_us,
                    esp_alloc::HEAP.used() - heap_before,
                    ALLOCATIONS.load(Relaxed) - allocations_before,
                    mismatches,
                    &mut times,
                );
                println!(
                    "first_frame_us={} first_alloc_calls={} peak_requested_bytes={} heap_free={}",
                    first_us,
                    first_allocations,
                    REQUESTED_PEAK.load(Relaxed),
                    esp_alloc::HEAP.free()
                );
                drop((show, workspace, buffers));
                assert_eq!(esp_alloc::HEAP.used(), heap_before, "show case leaked heap");
            }
        }
    }
    for (index, name) in fixtures::GENERATOR_NAMES.iter().enumerate() {
        let heap_before = esp_alloc::HEAP.used();
        REQUESTED_PEAK.store(REQUESTED_LIVE.load(Relaxed), Relaxed);
        let start = Instant::now();
        let show = donder_runtime::wire::decode_sequence(
            fixtures::GENERATOR_SEQUENCES[index],
            Default::default(),
        )
        .unwrap();
        let mut workspace = show.workspace();
        let mut output = [vec![0; 600]];
        let setup_us = start.elapsed().as_micros() as u32;
        let first_allocations = ALLOCATIONS.load(Relaxed);
        let start = Instant::now();
        show.evaluate(workload::time(0), &mut output, &mut workspace)
            .unwrap();
        let first_us = start.elapsed().as_micros() as u32;
        let first_allocations = ALLOCATIONS.load(Relaxed) - first_allocations;
        let mut times = [0u32; workload::FRAMES];
        let allocations_before = ALLOCATIONS.load(Relaxed);
        let mut mismatches = 0;
        for (frame, elapsed) in times.iter_mut().enumerate() {
            let start = Instant::now();
            show.evaluate(workload::time(frame), &mut output, &mut workspace)
                .unwrap();
            black_box(&output);
            *elapsed = start.elapsed().as_micros() as u32;
            mismatches += usize::from(
                workload::checksum(&output[0]) != fixtures::GENERATOR_GOLDEN[index][frame],
            );
        }
        report(
            "generator",
            name,
            200,
            setup_us,
            esp_alloc::HEAP.used() - heap_before,
            ALLOCATIONS.load(Relaxed) - allocations_before,
            mismatches,
            &mut times,
        );
        println!(
            "first_frame_us={} first_alloc_calls={} peak_requested_bytes={} heap_free={}",
            first_us,
            first_allocations,
            REQUESTED_PEAK.load(Relaxed),
            esp_alloc::HEAP.free()
        );
        drop((show, workspace, output));
        assert_eq!(
            esp_alloc::HEAP.used(),
            heap_before,
            "generator case leaked heap"
        );
    }
    println!("DONDER PROFILE END heap_free={}", esp_alloc::HEAP.free());
    loop {
        core::hint::spin_loop();
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "One flat serial measurement record"
)]
fn report(
    stage: &str,
    effect: &str,
    pixels: usize,
    setup_us: u32,
    retained: usize,
    allocations: u32,
    mismatches: usize,
    times: &mut [u32; workload::FRAMES],
) {
    let mean = times.iter().sum::<u32>() / workload::FRAMES as u32;
    times.sort_unstable();
    println!(
        "stage={} effect={} pixels={} setup_us={} retained_bytes={} alloc_calls={} mismatched_frames={} min_us={} median_us={} mean_us={} p95_us={} max_us={} fps={}",
        stage,
        effect,
        pixels,
        setup_us,
        retained,
        allocations,
        mismatches,
        times[0],
        times[workload::FRAMES / 2],
        mean,
        times[30],
        times[workload::FRAMES - 1],
        1_000_000 / mean.max(1)
    );
}
