#[allow(dead_code)]
mod fixtures;

use criterion::{Criterion, criterion_group, criterion_main};
use donder_language::dsl::VmWorkspace;
use std::hint::black_box;
use std::time::Duration;

fn bench_effect_vm(c: &mut Criterion) {
    pin_benchmark_thread();
    let effects = fixtures::cases()
        .map(|(name, source, params)| fixtures::prepared_effect(name, source, params));
    let contexts = fixtures::sample_contexts();
    let mut workspacees = std::array::from_fn::<_, 4, _>(|_| VmWorkspace::default());

    c.bench_function("dsl_effect_suite_4x512_pixels", |b| {
        b.iter(|| {
            for ((effect, bound), workspace) in effects.iter().zip(&mut workspacees) {
                for context in &contexts {
                    black_box(
                        effect
                            .sample_bound(black_box(bound), black_box(context), workspace)
                            .expect("sample benchmark effect should run"),
                    );
                }
            }
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
    targets = bench_effect_vm
}
criterion_main!(benches);
