use donder_runtime::sampling::deterministic_random_seed;

#[test]
fn random_is_repeatable_bounded_and_distributed() {
    let mut buckets = [0_u32; 16];
    for seed in -8192..8192 {
        let value = deterministic_random_seed(seed as f32);
        assert_eq!(value, deterministic_random_seed(seed as f32));
        assert!((0.0..1.0).contains(&value));
        buckets[(value * 16.0) as usize] += 1;
    }
    assert_eq!(
        deterministic_random_seed(0.0),
        deterministic_random_seed(-0.0)
    );
    assert!(
        buckets.iter().all(|&count| (850..1200).contains(&count)),
        "{buckets:?}"
    );
}

#[test]
fn trigonometry_accuracy_over_show_phases() {
    // Cover about four minutes at 16 Hz, including negative phases. f32 range
    // reduction loses accuracy at large phases; retain the tighter near bound.
    let mut maximum_error = 0.0_f32;
    for step in -2_500_000..=2_500_000 {
        let phase = step as f32 * 0.01;
        let sine = micromath::F32Ext::sin(phase);
        let cosine = micromath::F32Ext::cos(phase);
        let error = (sine - libm::sinf(phase))
            .abs()
            .max((cosine - libm::cosf(phase)).abs());
        maximum_error = maximum_error.max(error);
        if phase.abs() <= 1000.0 {
            assert!(error < 0.002, "phase {phase}: error {error}");
        }
    }
    assert!(
        maximum_error < 0.004,
        "maximum absolute error: {maximum_error}"
    );
    eprintln!("maximum absolute error over +/-25000 radians: {maximum_error}");
}
