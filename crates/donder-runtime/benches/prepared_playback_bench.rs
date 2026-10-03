#[allow(dead_code)]
mod fixtures;
#[path = "fixtures/layered.rs"]
mod layered;

use criterion::{Criterion, criterion_group, criterion_main};
use donder_language::values::SampleTime;

#[allow(dead_code)]
#[path = "../tests/support/playback.rs"]
mod playback;
use std::hint::black_box;
use std::time::Duration;

fn bench_prepared_playback(c: &mut Criterion) {
    pin_benchmark_thread();
    let mut playbacks = fixtures::cases().map(|(name, source, params)| {
        let (effect, params) = fixtures::prepared_effect(name, source, params);
        let invocation = playback::sample(&effect, &params);
        playback::show(512, &invocation, 1).into_playback()
    });
    let time = SampleTime::from_ticks(3_250_000);

    c.bench_function("prepared_effect_suite_4x512_pixels", |b| {
        b.iter(|| {
            for playback in &mut playbacks {
                black_box(playback.evaluate(black_box(time)));
            }
        });
    });

    let mut layered = layered::layered_600().into_playback();
    c.bench_function("prepared_600_pixels_4_layers_3_operators", |b| {
        b.iter(|| {
            black_box(layered.evaluate(black_box(time)));
        });
    });
}

#[cfg(windows)]
fn pin_benchmark_thread() {
    use std::ffi::c_void;

    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetCurrentThread() -> *mut c_void;
        fn SetThreadAffinityMask(thread: *mut c_void, affinity_mask: usize) -> usize;
        fn SetThreadPriority(thread: *mut c_void, priority: i32) -> i32;
    }

    // Logical CPU 0 commonly handles extra OS work. A fixed nonzero CPU also prevents
    // migrations between unlike cores; smaller systems fall back to their final CPU.
    let cpu = std::thread::available_parallelism()
        .map(|count| 2.min(count.get().saturating_sub(1)))
        .unwrap_or(0);
    let thread = unsafe { GetCurrentThread() };
    let previous = unsafe { SetThreadAffinityMask(thread, 1usize << cpu) };
    assert_ne!(previous, 0, "benchmark thread affinity should be set");
    assert_ne!(
        unsafe { SetThreadPriority(thread, 2) },
        0,
        "benchmark thread priority should be raised"
    );
}

#[cfg(not(windows))]
fn pin_benchmark_thread() {}

fn criterion_config() -> Criterion {
    Criterion::default()
        .warm_up_time(Duration::from_secs(3))
        .measurement_time(Duration::from_secs(5))
        .noise_threshold(0.05)
}

criterion_group! {
    name = benches;
    config = criterion_config();
    targets = bench_prepared_playback
}
criterion_main!(benches);
