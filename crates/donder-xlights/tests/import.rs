//! Importing the xLights show in `tests/fixtures/oklahoma_house`, the house
//! of `examples/rydell_house`, as it was set up in xLights in December 2025.

use camino::{Utf8Path, Utf8PathBuf};
use donder_model::{FixtureShape, Layout, LayoutFixtureKind, Owned, Patch};
use donder_project_io::ProjectSession;
use donder_xlights::{ImportError, ImportOptions, ImportReport, Scale, SkipReason, UnroutedReason};

const EMPTY_PROJECT: &str = "Project house {
  format: 1,
  description: none,
  setup: Setup {
    description: none,
    layout: Layout { description: none, root: [], items: [] },
    patch: Patch { description: none, routes: [] },
    controllers: [],
    input_devices: [],
  },
  sequences: [],
  rigs: [],
}
";

fn show() -> Utf8PathBuf {
    Utf8Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/oklahoma_house")
}

fn load(root: &Utf8Path) -> ProjectSession {
    donder_project_io::check_project(root, &Default::default(), &mut Default::default())
        .into_session()
        .unwrap_or_else(|diagnostics| panic!("{diagnostics:?}"))
}

/// The house imported into a new project, saved and loaded again.
fn imported(options: &ImportOptions) -> (tempfile::TempDir, ProjectSession, ImportReport) {
    let directory = tempfile::tempdir().unwrap();
    let root = Utf8PathBuf::from_path_buf(directory.path().to_path_buf()).unwrap();
    std::fs::write(root.join("project.data.donder"), EMPTY_PROJECT).unwrap();
    let mut session = load(&root);
    let report = donder_xlights::import_show(&mut session, &show(), options).unwrap();
    donder_project_io::save_project(&session).unwrap();
    (directory, load(&root), report)
}

fn layout(session: &ProjectSession) -> &Layout {
    session
        .project
        .reusable::<Layout>()
        .values()
        .next()
        .unwrap()
}

fn fixture_shapes<'a>(layout: &'a Layout, name: &str) -> &'a [donder_model::FixtureElement] {
    let fixture = layout
        .fixtures
        .iter()
        .find(|fixture| fixture.name.as_str() == name)
        .unwrap_or_else(|| panic!("no fixture {name}"));
    match &fixture.kind {
        LayoutFixtureKind::Fixture {
            definition: Owned::Inline(definition),
            ..
        } => &definition.elements,
        other => panic!("{name} is {other:?}"),
    }
}

#[test]
fn imports_the_house() {
    let (_directory, session, report) = imported(&ImportOptions::default());
    assert_eq!(
        (
            report.fixtures,
            report.pixels,
            report.groups,
            report.controllers,
            report.routes
        ),
        (215, 27053, 43, 31, 346)
    );
    assert_eq!(report.scale, Some(Scale::PixelSpacing(0.075)));
    let reasons = |model: &str| {
        report
            .skipped
            .iter()
            .filter(|skipped| skipped.model == model)
            .map(|skipped| skipped.reason.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(reasons("Wristband85"), [SkipReason::Type("DmxGeneral")]);
    // Two models drive the same lights; the first in the file keeps them.
    assert_eq!(
        reasons("Left meteor1"),
        [SkipReason::SharesChannels("Right Meteor-22".into())]
    );
    assert!(
        report
            .unrouted
            .iter()
            .any(|unrouted| unrouted.model == "QR Code"
                && unrouted.reason
                    == UnroutedReason::UnknownUniverse("#192.168.168.83:80:1".into()))
    );
    let layout = layout(&session);
    assert_eq!(layout.fixtures.len(), 215 + 43);
    let patch = session.project.reusable::<Patch>().values().next().unwrap();
    assert_eq!(patch.routes.len(), 346);
}

#[test]
fn shapes_follow_the_models() {
    let (_directory, session, _) = imported(&ImportOptions::default());
    let layout = layout(&session);
    // A sidewalk arch is one 250-pixel half ellipse, more than twice as tall as
    // it is wide.
    let arch = fixture_shapes(layout, "sw_arch_16");
    assert_eq!(arch.len(), 1);
    let FixtureShape::Arc {
        count,
        sweep_degrees,
        ..
    } = arch[0].shape
    else {
        panic!("{:?}", arch[0].shape)
    };
    assert_eq!((count, sweep_degrees.abs().round()), (250, 180.0));
    assert!(arch[0].transform.scale.y > 2.0);
    // Each of a pixel ball's 50 strands is one arc of longitude.
    let ball = fixture_shapes(layout, "sphere_6");
    assert_eq!(ball.len(), 50);
    assert!(
        ball.iter()
            .all(|shape| matches!(shape.shape, FixtureShape::Arc { count: 10, .. }))
    );
    // A single line of the mega tree is one straight line of 50 pixels.
    let strand = fixture_shapes(layout, "megatreemanual_23");
    assert!(matches!(
        strand,
        [shape] if matches!(shape.shape, FixtureShape::Line { count: 50, .. })
    ));
}

#[test]
fn routes_follow_the_channels() {
    let (_directory, session, _) = imported(&ImportOptions::default());
    let layout = layout(&session);
    let patch = session.project.reusable::<Patch>().values().next().unwrap();
    let penguin = layout
        .fixtures
        .iter()
        .find(|fixture| fixture.name.as_str() == "penguin")
        .unwrap()
        .id;
    // The penguin's 1190 pixels run from the start of universe 232 across
    // seven universes of 170 pixels.
    let routes: Vec<_> = patch
        .routes
        .iter()
        .filter(|route| route.target.fixture == penguin)
        .collect();
    assert_eq!(routes.len(), 7);
    let mut next = 0;
    for route in routes {
        let span = route.pixels.unwrap();
        assert_eq!((span.start, span.count, route.start_slot), (next, 170, 0));
        next += 170;
    }
}

#[test]
fn pixel_spacing_sets_the_size() {
    let (_directory, _, near) = imported(&ImportOptions::default());
    let (_directory, _, far) = imported(&ImportOptions {
        pixel_spacing: Some(0.15),
    });
    assert!((far.meters_per_unit / near.meters_per_unit - 2.0).abs() < 1e-9);
}

#[test]
fn imports_only_into_an_empty_setup() {
    let (directory, mut session, _) = imported(&ImportOptions::default());
    let error = donder_xlights::import_show(&mut session, &show(), &ImportOptions::default());
    assert!(matches!(error, Err(ImportError::NotEmpty)));
    drop(directory);
}
