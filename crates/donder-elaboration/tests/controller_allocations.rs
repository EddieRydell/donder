use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use camino::Utf8PathBuf;
use donder_elaboration::{PreparedShow, Retain, prepare};
use donder_project_io::check_project;
use donder_runtime_types::sample_time_from_frame;

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
    let session = check_project(&project_path, &Default::default(), &mut Default::default())
        .into_session()
        .expect("starter project should load");
    for sequence_id in session
        .project()
        .reusable::<donder_model::Sequence>()
        .keys()
    {
        let output = prepare(session.project(), sequence_id, Retain::All)
            .expect("starter output should prepare");
        let frames = [
            0,
            output.frame_count() / 2,
            output.frame_count().saturating_sub(1),
        ];
        assert_prepared_sampling_does_not_allocate(
            output.clone(),
            &frames,
            session
                .declaration((*sequence_id).into())
                .unwrap()
                .1
                .as_str(),
        );
        let controllers_store = session.project().reusable::<donder_model::Controller>();
        let controller = controllers_store.get_index(0).unwrap().0;
        let first_output =
            session.project().reusable::<donder_model::Controller>()[controller].outputs()[0].0;
        let mut selected = prepare(
            session.project(),
            sequence_id,
            Retain::Outputs(&[(*controller, first_output)]),
        )
        .unwrap();
        let encoded = selected.encode();
        selected = donder_runtime::PreparedShow::decode(&encoded).unwrap();
        assert_prepared_sampling_does_not_allocate(selected.clone(), &frames, "selected output");
        let measure = |accepted: PreparedShow| {
            let before = LIVE_BYTES.load(Ordering::Relaxed);
            let workspace = accepted.into_playback();
            let bytes = LIVE_BYTES.load(Ordering::Relaxed) - before;
            drop(workspace);
            bytes
        };
        let full_bytes = measure(output);
        let selected_bytes = measure(selected);
        // The starter's one controller sends every fixture over its universes,
        // so its output is the whole show.
        assert!(selected_bytes <= full_bytes);
        println!(
            "{} runtime workspace heap: {full_bytes} -> {selected_bytes} bytes",
            session
                .declaration((*sequence_id).into())
                .unwrap()
                .1
                .as_str()
        );
    }
}

fn assert_prepared_sampling_does_not_allocate(output: PreparedShow, frames: &[u32], name: &str) {
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
