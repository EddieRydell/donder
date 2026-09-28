use donder_language::controller::*;
use donder_language::fixture::*;
use donder_language::identity::DocumentId;
use donder_language::layout::*;
use donder_language::patch::*;
use donder_language::setup::Setup;
use yaml_serde::{Mapping, Value};

use super::ProjectSession;
use super::values::*;
use crate::ExportProjectError;
use crate::source::SourceObjectKind;

pub(super) fn setup_value(
    session: &ProjectSession,
    from: &DocumentId,
    setup: &Setup,
) -> Result<Value, ExportProjectError> {
    let mut value = typed_object("setup");
    value.insert(
        string_value("layout"),
        source_value(
            session,
            from,
            SourceObjectKind::Layout,
            &setup.layout,
            |layout| layout_value(session, from, layout),
        )?,
    );
    value.insert(
        string_value("patch"),
        source_value(
            session,
            from,
            SourceObjectKind::Patch,
            &setup.patch,
            |patch| patch_value(session, from, patch),
        )?,
    );
    value.insert(
        string_value("controllers"),
        Value::Sequence(
            setup
                .controllers
                .iter()
                .map(|source| {
                    source_value(
                        session,
                        from,
                        SourceObjectKind::Controller,
                        source,
                        controller_value,
                    )
                })
                .collect::<Result<_, _>>()?,
        ),
    );
    Ok(Value::Mapping(value))
}

pub(super) fn controller_value(controller: &Controller) -> Result<Value, ExportProjectError> {
    let mut value = typed_object("controller");
    insert_owned_collection_id(&mut value, &controller.id.0);
    let mut protocol = Mapping::new();
    match &controller.protocol {
        ControllerProtocol::E131(config) => {
            protocol.insert(string_value("type"), Value::String("e131".to_string()));
            protocol.insert(
                string_value("source_name"),
                Value::String(config.source_name.clone()),
            );
            protocol.insert(
                string_value("bind_address"),
                Value::String(config.bind_address.to_string()),
            );
            protocol.insert(string_value("priority"), serialized_value(config.priority)?);
            match config.mode {
                E131Mode::Multicast => {
                    protocol.insert(string_value("mode"), Value::String("multicast".to_string()));
                }
                E131Mode::Unicast { destination } => {
                    protocol.insert(string_value("mode"), Value::String("unicast".to_string()));
                    protocol.insert(
                        string_value("destination"),
                        Value::String(destination.to_string()),
                    );
                }
            }
        }
        ControllerProtocol::ArtNet(config) => {
            protocol.insert(string_value("type"), Value::String("artnet".to_string()));
            protocol.insert(
                string_value("bind_address"),
                Value::String(config.bind_address.to_string()),
            );
            protocol.insert(
                string_value("destination"),
                Value::String(config.destination.to_string()),
            );
            protocol.insert(
                string_value("mode"),
                Value::String(
                    match config.mode {
                        ArtNetMode::Unicast => "unicast",
                        ArtNetMode::Broadcast => "broadcast",
                    }
                    .to_string(),
                ),
            );
        }
    }
    value.insert(string_value("protocol"), Value::Mapping(protocol));
    value.insert(
        string_value("ports"),
        Value::Sequence(
            controller
                .ports
                .iter()
                .map(|port| {
                    let mut item = Mapping::new();
                    item.insert(string_value("id"), serialized_value(port.id.0)?);
                    match port.address {
                        ControllerPortAddress::E131Universe(universe) => {
                            item.insert(string_value("universe"), serialized_value(universe)?)
                        }
                        ControllerPortAddress::ArtNetPort(address) => {
                            item.insert(string_value("port_address"), serialized_value(address)?)
                        }
                    };
                    item.insert(
                        string_value("slot_count"),
                        serialized_value(port.slot_count)?,
                    );
                    Ok(Value::Mapping(item))
                })
                .collect::<Result<Vec<_>, ExportProjectError>>()?,
        ),
    );
    Ok(Value::Mapping(value))
}

pub(super) fn layout_value(
    session: &ProjectSession,
    from: &DocumentId,
    layout: &Layout,
) -> Result<Value, ExportProjectError> {
    let mut value = typed_object("layout");
    value.insert(
        string_value("fixtures"),
        Value::Sequence(
            layout
                .fixtures
                .iter()
                .map(|fixture| layout_fixture_value(session, from, fixture))
                .collect::<Result<_, _>>()?,
        ),
    );
    Ok(Value::Mapping(value))
}

fn layout_fixture_value(
    session: &ProjectSession,
    from: &DocumentId,
    fixture: &LayoutFixture,
) -> Result<Value, ExportProjectError> {
    let mut value = Mapping::new();
    value.insert(string_value("id"), serialized_value(fixture.id.0)?);
    value.insert(string_value("name"), string_value(&fixture.name));
    match &fixture.kind {
        LayoutFixtureKind::Fixture {
            definition,
            transform,
        } => {
            value.insert(string_value("type"), string_value("fixture"));
            value.insert(
                string_value("definition"),
                match definition {
                    donder_language::fixture::FixtureSource::Inline(value) => {
                        fixture_definition_value(value)?
                    }
                    donder_language::fixture::FixtureSource::Reference(id) => {
                        string_value(&write_source_reference(
                            session,
                            from,
                            SourceObjectKind::FixtureDefinition,
                            &id.0,
                        )?)
                    }
                },
            );
            value.insert(string_value("transform"), transform_value(transform)?);
        }
        LayoutFixtureKind::Group { children } => {
            value.insert(string_value("type"), string_value("group"));
            value.insert(
                string_value("children"),
                Value::Sequence(
                    children
                        .iter()
                        .map(|child| layout_fixture_value(session, from, child))
                        .collect::<Result<_, _>>()?,
                ),
            );
        }
    }
    Ok(Value::Mapping(value))
}

pub(super) fn fixture_definition_value(
    definition: &FixtureDefinition,
) -> Result<Value, ExportProjectError> {
    let mut value = typed_object("fixture");
    value.insert(
        string_value("elements"),
        Value::Sequence(
            definition
                .elements
                .iter()
                .map(|element| {
                    let mut value = Mapping::new();
                    value.insert(string_value("id"), serialized_value(element.id.0)?);
                    value.insert(string_value("name"), string_value(&element.name));
                    value.insert(
                        string_value("diameter"),
                        serialized_value(f64::from(element.diameter.micrometers) / 1_000_000.0)?,
                    );
                    value.insert(
                        string_value("transform"),
                        transform_value(&element.transform)?,
                    );
                    value.insert(string_value("reverse"), serialized_value(element.reverse)?);
                    let mut shape = Mapping::new();
                    match &element.shape {
                        FixtureShape::Pixel => {
                            shape.insert(string_value("type"), string_value("pixel"));
                        }
                        FixtureShape::Line { length, count } => {
                            shape.insert(string_value("type"), string_value("line"));
                            shape.insert(string_value("length"), serialized_value(length)?);
                            shape.insert(string_value("count"), serialized_value(count)?);
                        }
                        FixtureShape::Polyline { points, count } => {
                            shape.insert(string_value("type"), string_value("polyline"));
                            shape.insert(
                                string_value("points"),
                                Value::Sequence(
                                    points.iter().map(point_value).collect::<Result<_, _>>()?,
                                ),
                            );
                            shape.insert(string_value("count"), serialized_value(count)?);
                        }
                        FixtureShape::Arc {
                            radius,
                            start_degrees,
                            sweep_degrees,
                            count,
                            closed,
                        } => {
                            shape.insert(string_value("type"), string_value("arc"));
                            shape.insert(string_value("radius"), serialized_value(radius)?);
                            shape.insert(
                                string_value("start_degrees"),
                                serialized_value(start_degrees)?,
                            );
                            shape.insert(
                                string_value("sweep_degrees"),
                                serialized_value(sweep_degrees)?,
                            );
                            shape.insert(string_value("count"), serialized_value(count)?);
                            shape.insert(string_value("closed"), serialized_value(closed)?);
                        }
                        FixtureShape::Grid {
                            columns,
                            rows,
                            width,
                            height,
                            axis,
                            corner,
                            serpentine,
                        } => {
                            shape.insert(string_value("type"), string_value("grid"));
                            shape.insert(string_value("columns"), serialized_value(columns)?);
                            shape.insert(string_value("rows"), serialized_value(rows)?);
                            shape.insert(string_value("width"), serialized_value(width)?);
                            shape.insert(string_value("height"), serialized_value(height)?);
                            shape.insert(
                                string_value("axis"),
                                string_value(match axis {
                                    GridAxis::Rows => "rows",
                                    GridAxis::Columns => "columns",
                                }),
                            );
                            shape.insert(
                                string_value("corner"),
                                string_value(match corner {
                                    GridCorner::BottomLeft => "bottom_left",
                                    GridCorner::BottomRight => "bottom_right",
                                    GridCorner::TopLeft => "top_left",
                                    GridCorner::TopRight => "top_right",
                                }),
                            );
                            shape.insert(string_value("serpentine"), serialized_value(serpentine)?);
                        }
                    }
                    value.insert(string_value("shape"), Value::Mapping(shape));
                    Ok(Value::Mapping(value))
                })
                .collect::<Result<_, ExportProjectError>>()?,
        ),
    );
    Ok(Value::Mapping(value))
}

pub(super) fn patch_value(
    session: &ProjectSession,
    from: &DocumentId,
    patch: &Patch,
) -> Result<Value, ExportProjectError> {
    let mut value = typed_object("patch");
    value.insert(
        string_value("routes"),
        Value::Sequence(
            patch
                .routes
                .iter()
                .map(|route| {
                    let mut value = Mapping::new();
                    value.insert(string_value("id"), serialized_value(route.id.0)?);
                    value.insert(
                        string_value("target"),
                        fixture_target_value(session, from, &route.target)?,
                    );
                    if let Some(span) = route.pixels {
                        let mut pixels = Mapping::new();
                        pixels.insert(string_value("start"), serialized_value(span.start)?);
                        pixels.insert(string_value("count"), serialized_value(span.count)?);
                        value.insert(string_value("pixels"), Value::Mapping(pixels));
                    }
                    value.insert(
                        string_value("controller"),
                        write_object_reference(
                            session,
                            from,
                            SourceObjectKind::Controller,
                            &route.controller.0,
                        )?,
                    );
                    value.insert(string_value("port"), serialized_value(route.port.0)?);
                    value.insert(
                        string_value("start_slot"),
                        serialized_value(route.start_slot)?,
                    );
                    let mut encoding = typed_object(match route.encoding {
                        PixelEncoding::Rgb { .. } => "rgb",
                        PixelEncoding::Rgbw { .. } => "rgbw",
                    });
                    encoding.insert(
                        string_value("order"),
                        serialized_value(route.encoding.channel_order())?,
                    );
                    value.insert(string_value("encoding"), Value::Mapping(encoding));
                    value.insert(string_value("gamma"), serialized_value(route.gamma)?);
                    value.insert(
                        string_value("brightness"),
                        serialized_value(route.brightness)?,
                    );
                    Ok(Value::Mapping(value))
                })
                .collect::<Result<_, ExportProjectError>>()?,
        ),
    );
    Ok(Value::Mapping(value))
}
