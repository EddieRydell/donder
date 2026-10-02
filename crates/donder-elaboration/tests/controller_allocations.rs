mod support;

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use camino::Utf8PathBuf;
use donder_elaboration::{PrepareOutputs, PreparedSequence, prepare};
use donder_language::dsl::Identifier;
use donder_language::sequence::AutomationTarget;
use donder_language::values::sample_time_from_frame;
use donder_project_io::load_project;

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
    let session = load_project(&project_path).expect("starter project should load");
    for sequence_id in session
        .project
        .root()
        .sequences
        .iter()
        .map(|source| source.id())
    {
        let output = prepare(&session.project, sequence_id, PrepareOutputs::All)
            .expect("starter output should prepare");
        let frames = [
            0,
            output.frame_count() / 2,
            output.frame_count().saturating_sub(1),
        ];
        assert_prepared_sampling_does_not_allocate(
            output.clone(),
            &frames,
            sequence_id.0.root_source().object(),
        );
        let setup = &session.project.reusable_setups()[session.project.root().setup.id()];
        let controller = setup.controllers[0].id();
        let port = session.project.reusable_controllers()[controller].ports[0].id;
        let mut selected = prepare(
            &session.project,
            sequence_id,
            PrepareOutputs::Ports(&[(controller.clone(), port)]),
        )
        .unwrap();
        let encoded = donder_runtime::encode_sequence(&selected).unwrap();
        selected = donder_runtime::decode_sequence(&encoded, Default::default()).unwrap();
        assert_prepared_sampling_does_not_allocate(selected.clone(), &frames, "selected output");
        let measure = |accepted: PreparedSequence| {
            let before = LIVE_BYTES.load(Ordering::Relaxed);
            let workspace = accepted.into_playback();
            let bytes = LIVE_BYTES.load(Ordering::Relaxed) - before;
            drop(workspace);
            bytes
        };
        let full_bytes = measure(output);
        let selected_bytes = measure(selected);
        assert!(selected_bytes < full_bytes);
        println!(
            "{} runtime workspace heap: {full_bytes} -> {selected_bytes} bytes",
            sequence_id.0.root_source().object()
        );
    }

    let mut project = session.project;
    let sequence_id = project
        .root()
        .sequences
        .iter()
        .map(|source| source.id())
        .find(|id| id.0.root_source().object() == "layer_test")
        .expect("starter project should include layer_test")
        .clone();
    let mut sequence = project
        .sequence(&sequence_id)
        .expect("layer_test should resolve")
        .clone();
    sequence.automation_clips[0].bindings[0].target = AutomationTarget::EffectParam {
        effect_id: sequence.effects[0].id.clone(),
        param: Identifier::new("pulse_overlap".to_string()).expect("static identifier is valid"),
    };
    project.replace_sequence(&sequence_id, sequence).unwrap();
    let output = prepare(&project, &sequence_id, PrepareOutputs::All)
        .expect("automated DSL output should prepare");
    assert_prepared_sampling_does_not_allocate(output, &[7150, 7151, 7152], "automated DSL effect");
    for query in [
        "source.at(seconds() + offset_seconds, pixel_count() - 1 - pixel_index())",
        "source.at_global(seconds() + offset_seconds, 226 + pixel_index())",
    ] {
        let compiled = donder_language::dsl::compile_operators(&format!(
            "operator TimeWarp {{ input Signal source; param float offset_seconds = 0.0; color sample() {{ return {query}; }} }}"
        )).unwrap().remove(0);
        let definition_id = project
            .definitions()
            .operators
            .definitions
            .iter()
            .find(|(_, definition)| definition.declaration_name == "TimeWarp")
            .unwrap()
            .0
            .clone();
        let definition =
            donder_language::operator::custom_operator_definition(definition_id.clone(), compiled);
        project
            .apply_edits(
                [donder_language::model::ProjectEdit::SetOperatorDefinition {
                    id: definition_id,
                    value: definition,
                }],
            )
            .unwrap();
        let output = prepare(&project, &sequence_id, PrepareOutputs::All).unwrap();
        assert_prepared_sampling_does_not_allocate(output, &[0, 8494, 7150, 7151, 7152, 0], query);
    }

    // Exercise the project-owned bounded Echo loop through the same prepared
    // controller path, including temporal queries and backward seeks.
    let echo = donder_language::dsl::compile_operators(include_str!(
        "../../../examples/starter/operators/standard.operator.donder"
    ))
    .unwrap()
    .into_iter()
    .find(|operator| operator.name().as_str() == "Echo")
    .unwrap();
    support::append_operator(&mut project, &sequence_id, echo);
    let output = prepare(&project, &sequence_id, PrepareOutputs::All).unwrap();
    assert_prepared_sampling_does_not_allocate(output, &[0, 8494, 7150, 7151, 0], "DSL Echo");
}

fn assert_prepared_sampling_does_not_allocate(
    output: PreparedSequence,
    frames: &[u32],
    name: &str,
) {
    let frame_rate = output.frame_rate();
    let mut workspace = output.into_playback();
    ALLOCATIONS.store(0, Ordering::Relaxed);
    COUNTING.store(true, Ordering::Relaxed);
    for &frame in frames {
        let time = sample_time_from_frame(frame, frame_rate)
            .expect("sample frame should fit the controller clock");
        std::hint::black_box(workspace.evaluate(time));
    }
    COUNTING.store(false, Ordering::Relaxed);
    assert_eq!(
        ALLOCATIONS.load(Ordering::Relaxed),
        0,
        "warmed sequence {name} allocated"
    );
}
