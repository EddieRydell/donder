use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use camino::Utf8PathBuf;
use donder_elaboration::PreparedSequenceOutput;
use donder_language::dsl::Identifier;
use donder_language::sequence::AutomationTarget;
use donder_language::values::sample_time_from_frame;
use donder_project_io::load_package;

struct CountingAllocator;

static COUNTING: AtomicBool = AtomicBool::new(false);
static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
static LIVE_BYTES: AtomicUsize = AtomicUsize::new(0);

#[global_allocator]
static ALLOCATOR: CountingAllocator = CountingAllocator;

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        LIVE_BYTES.fetch_add(layout.size(), Ordering::Relaxed);
        if COUNTING.load(Ordering::Relaxed) {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        LIVE_BYTES.fetch_sub(layout.size(), Ordering::Relaxed);
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        LIVE_BYTES.fetch_add(layout.size(), Ordering::Relaxed);
        if COUNTING.load(Ordering::Relaxed) {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        LIVE_BYTES.fetch_add(size, Ordering::Relaxed);
        LIVE_BYTES.fetch_sub(layout.size(), Ordering::Relaxed);
        if COUNTING.load(Ordering::Relaxed) {
            ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.realloc(pointer, layout, size) }
    }
}

#[test]
fn prepared_controller_sampling_does_not_allocate() {
    let project_path = Utf8PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("examples/starter");
    let session = load_package(&project_path)
        .expect("starter project should load")
        .session;
    for sequence_id in &session.project.root.sequences {
        let output = PreparedSequenceOutput::prepare(
            &session.project,
            &session.project.root.setup,
            sequence_id,
        )
        .expect("starter output should prepare");
        let frames = [
            0,
            output.frame_count() / 2,
            output.frame_count().saturating_sub(1),
        ];
        assert_prepared_sampling_does_not_allocate(&output, &frames, sequence_id.0.object());
        let setup = &session.project.setups[&session.project.root.setup];
        let controller = &setup.controllers[0];
        let port = session.project.controllers[controller].ports[0].id;
        let mut selected = PreparedSequenceOutput::prepare_selected(
            &session.project,
            &session.project.root.setup,
            sequence_id,
            &[(controller.clone(), port)],
        )
        .unwrap();
        let encoded = donder_runtime::wire::encode_sequence(&selected.sequence).unwrap();
        selected.sequence =
            donder_runtime::wire::decode_sequence(&encoded, Default::default()).unwrap();
        assert_prepared_sampling_does_not_allocate(&selected, &frames, "selected output");
        let measure = |output: &PreparedSequenceOutput| {
            let before = LIVE_BYTES.load(Ordering::Relaxed);
            let workspace = output.sequence.workspace();
            let bytes = LIVE_BYTES.load(Ordering::Relaxed) - before;
            drop(workspace);
            bytes
        };
        let full_bytes = measure(&output);
        let selected_bytes = measure(&selected);
        assert!(selected_bytes < full_bytes);
        println!(
            "{} runtime workspace heap: {full_bytes} -> {selected_bytes} bytes",
            sequence_id.0.object()
        );
    }

    let mut project = session.project;
    let sequence_id = project
        .root
        .sequences
        .iter()
        .find(|id| id.0.object() == "layer_test")
        .expect("starter project should include layer_test")
        .clone();
    let sequence = project
        .sequences
        .get_mut(&sequence_id)
        .expect("layer_test should resolve");
    sequence.automation_clips[0].bindings[0].target = AutomationTarget::EffectParam {
        effect_id: sequence.effects[0].id.clone(),
        param: Identifier::new("pulse_overlap".to_string()).expect("static identifier is valid"),
    };
    let output = PreparedSequenceOutput::prepare(&project, &project.root.setup, &sequence_id)
        .expect("automated native output should prepare");
    assert_prepared_sampling_does_not_allocate(
        &output,
        &[7150, 7151, 7152],
        "automated native effect",
    );
    for query in [
        "source.at(seconds() + offset_seconds, pixel_count() - 1 - pixel_index())",
        "source.at_global(seconds() + offset_seconds, 226 + pixel_index())",
    ] {
        let compiled = donder_language::dsl::compile_operators(&format!(
            "operator TimeWarp {{ input Signal source; param float offset_seconds = 0.0; color sample() {{ return {query}; }} }}"
        )).unwrap().remove(0);
        let definition = project
            .definitions
            .operators
            .definitions
            .values_mut()
            .find(|definition| definition.declaration_name == "TimeWarp")
            .unwrap();
        definition.implementation =
            donder_language::operator::OperatorImplementation::Dsl(Box::new(compiled));
        let output =
            PreparedSequenceOutput::prepare(&project, &project.root.setup, &sequence_id).unwrap();
        assert_prepared_sampling_does_not_allocate(&output, &[0, 8494, 7150, 7151, 7152, 0], query);
    }
}

fn assert_prepared_sampling_does_not_allocate(
    output: &PreparedSequenceOutput,
    frames: &[u32],
    name: &str,
) {
    let mut workspace = output.workspace();

    ALLOCATIONS.store(0, Ordering::Relaxed);
    COUNTING.store(true, Ordering::Relaxed);
    let result = frames.iter().try_for_each(|&frame| {
        let time = sample_time_from_frame(frame, output.frame_rate())
            .expect("sample frame should fit the controller clock");
        output.sample_into(time, &mut workspace).map(|_| ())
    });
    COUNTING.store(false, Ordering::Relaxed);

    result.expect("measured samples should render");
    assert_eq!(
        ALLOCATIONS.load(Ordering::Relaxed),
        0,
        "warmed sequence {name} allocated"
    );
}
