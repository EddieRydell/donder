use donder_elaboration::{PrepareOutputs, prepare};
use donder_runtime_types::SampleTime;
fn main() {
    for (root, name) in [
        ("examples/stanford_room", "main"),
        ("examples/starter", "layer_test"),
    ] {
        let project = donder_project_io::load_project(&camino::Utf8PathBuf::from(root))
            .unwrap()
            .project;
        let id = project
            .root()
            .sequences
            .iter()
            .map(|s| s.id())
            .find(|id| id.0.root_source().object() == name)
            .unwrap();
        let prepared = prepare(&project, id, PrepareOutputs::All).unwrap();
        println!(
            "{root}: effects={} fixtures={}",
            prepared.effect_count(),
            prepared.fixtures().len()
        );
        let mut playback = prepared.into_playback();
        for seconds in [0.5f32, 10.0, 50.0, 60.0, 100.0, 140.0, 200.0, 300.0] {
            let frame = playback.evaluate(SampleTime::from_ticks((seconds * 1e6) as u32));
            let lit: usize = frame
                .fixtures()
                .map(|f| {
                    f.pixels
                        .iter()
                        .filter(|c| (c.red | c.green | c.blue) > 0)
                        .count()
                })
                .sum();
            let out: usize = frame
                .outputs()
                .map(|p| p.bytes.iter().filter(|b| **b != 0).count())
                .sum();
            print!(" t={seconds}:lit_px={lit},out_bytes={out}");
        }
        println!();
    }
}
