use crate::dto::*;
use crate::gui::{GuiMutationError, model::parse_color};
use dawn_language::fixture_profile::*;
use dawn_project_io::{ProjectSession, SourceObjectKind};
use indexmap::IndexMap;

pub(super) fn project_document(
    session: &ProjectSession,
    resolved: &super::ResolvedGuiObject,
) -> GuiDocument {
    let Some(profile) = session
        .project
        .definitions
        .fixture_profiles
        .definitions
        .get(&FixtureProfileId(resolved.identity.clone()))
    else {
        return super::blocked("Fixture profile was not found.", Vec::new());
    };
    GuiDocument::FixtureProfile {
        document: FixtureProfileGuiDocument {
            path: resolved.identity.document().to_string(),
            object_key: resolved.identity.object().to_string(),
            definition: project(profile),
        },
    }
}

// These enums have identical meanings at the DTO boundary. Exhaustive matches
// keep additions on either side from silently losing a profile attribute.
macro_rules! enum_conversion {
    ($gui:ident, $domain:ident, $($variant:ident),+ $(,)?) => {
        impl From<$domain> for $gui {
            fn from(value: $domain) -> Self { match value { $($domain::$variant => Self::$variant),+ } }
        }
        impl From<$gui> for $domain {
            fn from(value: $gui) -> Self { match value { $($gui::$variant => Self::$variant),+ } }
        }
    };
}
enum_conversion!(
    GuiFixtureFunctionTag,
    FixtureFunctionTag,
    Pan,
    Tilt,
    Dimmer,
    Shutter,
    Zoom,
    Gobo,
    Frost,
    Prism,
    ColorWheel,
    ColorMixing
);
enum_conversion!(
    GuiFixtureEntryTag,
    FixtureEntryTag,
    ShutterOpen,
    ShutterClosed,
    Strobe,
    PrismOpen,
    PrismClosed,
    GoboOpen
);
enum_conversion!(GuiFixtureColorModel, ColorMixingModel, Rgb, Rgbw);
enum_conversion!(
    GuiFixtureColorComponent,
    ColorComponent,
    Red,
    Green,
    Blue,
    White
);

pub(super) fn project(profile: &FixtureProfile) -> GuiFixtureDefinition {
    GuiFixtureDefinition {
        functions: profile
            .functions
            .iter()
            .map(|(id, function)| GuiFixtureFunction {
                id: id.0,
                name: function.name.clone(),
                tag: function.tag.map(Into::into),
                curve: super::patch::project_curve(&function.curve),
                kind: match &function.kind {
                    FixtureFunctionKind::Range => GuiFixtureFunctionKind::Range,
                    FixtureFunctionKind::Indexed { entries } => GuiFixtureFunctionKind::Indexed {
                        entries: project_entries(entries),
                    },
                    FixtureFunctionKind::ColorWheel { entries } => {
                        GuiFixtureFunctionKind::ColorWheel {
                            entries: project_entries(entries),
                        }
                    }
                    FixtureFunctionKind::ColorMixing { model } => {
                        GuiFixtureFunctionKind::ColorMixing {
                            model: (*model).into(),
                        }
                    }
                },
            })
            .collect(),
        channels: profile
            .channels
            .iter()
            .map(|channel| GuiFixtureChannel {
                slot: channel.slot,
                curve: super::patch::project_curve(&channel.curve),
                role: match channel.role {
                    FixtureChannelRole::Coarse { function } => GuiFixtureChannelRole::Coarse {
                        function: function.0,
                    },
                    FixtureChannelRole::Fine { function } => GuiFixtureChannelRole::Fine {
                        function: function.0,
                    },
                    FixtureChannelRole::ColorComponent {
                        function,
                        component,
                    } => GuiFixtureChannelRole::ColorComponent {
                        function: function.0,
                        component: component.into(),
                    },
                    FixtureChannelRole::Ignored => GuiFixtureChannelRole::Ignored,
                },
            })
            .collect(),
        behavior_rules: profile
            .behavior_rules
            .iter()
            .map(|rule| match rule {
                FixtureBehaviorRule::Shutter {
                    function,
                    closed,
                    open,
                } => GuiFixtureBehavior::Shutter {
                    function: function.0,
                    closed: closed.0,
                    open: open.0,
                },
                FixtureBehaviorRule::Dimmer { function, off, on } => GuiFixtureBehavior::Dimmer {
                    function: function.0,
                    off: *off,
                    on: *on,
                },
                FixtureBehaviorRule::PrismGate {
                    function,
                    disabled,
                    enabled,
                } => GuiFixtureBehavior::PrismGate {
                    function: function.0,
                    disabled: disabled.0,
                    enabled: enabled.0,
                },
                FixtureBehaviorRule::ColorWheel { function, entries } => {
                    GuiFixtureBehavior::ColorWheel {
                        function: function.0,
                        entries: entries
                            .iter()
                            .map(|entry| GuiFixtureColorMapping {
                                color: entry.color.to_hex(),
                                entry: entry.entry.0,
                            })
                            .collect(),
                    }
                }
            })
            .collect(),
    }
}

fn project_entries(entries: &[FixtureIndexedEntry]) -> Vec<GuiFixtureEntry> {
    entries
        .iter()
        .map(|entry| GuiFixtureEntry {
            id: entry.id.0,
            name: entry.name.clone(),
            dmx_min: entry.dmx_min,
            dmx_max: entry.dmx_max,
            curve_control: entry.curve_control,
            color: entry.color.map(|color| color.to_hex()),
            tag: entry.tag.map(Into::into),
        })
        .collect()
}

fn domain_entries(
    entries: Vec<GuiFixtureEntry>,
) -> Result<Vec<FixtureIndexedEntry>, GuiMutationError> {
    entries
        .into_iter()
        .map(|entry| {
            Ok(FixtureIndexedEntry {
                id: FixtureEntryId(entry.id),
                name: entry.name,
                dmx_min: entry.dmx_min,
                dmx_max: entry.dmx_max,
                curve_control: entry.curve_control,
                color: entry.color.map(|color| parse_color(&color)).transpose()?,
                tag: entry.tag.map(Into::into),
            })
        })
        .collect()
}

pub(super) fn create(
    session: &mut ProjectSession,
    name: String,
    definition: GuiFixtureDefinition,
) -> Result<(), GuiMutationError> {
    let identity = super::setup::create_object_document(
        session,
        SourceObjectKind::FixtureProfile,
        &name,
        "fixture-profiles",
        "fixture-profile",
    )?;
    write_definition(session, FixtureProfileId(identity), definition)
}

pub(super) fn edit(
    session: &mut ProjectSession,
    resolved: &super::ResolvedGuiObject,
    definition: GuiFixtureDefinition,
) -> Result<(), GuiMutationError> {
    let id = FixtureProfileId(resolved.identity.clone());
    if !session
        .project
        .definitions
        .fixture_profiles
        .definitions
        .contains_key(&id)
    {
        return Err(GuiMutationError::Invalid(
            "Fixture profile was not found.".into(),
        ));
    }
    write_definition(session, id, definition)
}

fn write_definition(
    session: &mut ProjectSession,
    id: FixtureProfileId,
    definition: GuiFixtureDefinition,
) -> Result<(), GuiMutationError> {
    let mut functions = IndexMap::new();
    for function in definition.functions {
        let id = FixtureFunctionId(function.id);
        let value = FixtureFunction {
            name: function.name,
            tag: function.tag.map(Into::into),
            curve: super::patch::domain_curve(function.curve),
            kind: match function.kind {
                GuiFixtureFunctionKind::Range => FixtureFunctionKind::Range,
                GuiFixtureFunctionKind::Indexed { entries } => FixtureFunctionKind::Indexed {
                    entries: domain_entries(entries)?,
                },
                GuiFixtureFunctionKind::ColorWheel { entries } => FixtureFunctionKind::ColorWheel {
                    entries: domain_entries(entries)?,
                },
                GuiFixtureFunctionKind::ColorMixing { model } => FixtureFunctionKind::ColorMixing {
                    model: model.into(),
                },
            },
        };
        if functions.insert(id, value).is_some() {
            return Err(GuiMutationError::Invalid(format!(
                "Function {} is listed more than once. Give each function a unique identifier.",
                id.0
            )));
        }
    }
    let channels = definition
        .channels
        .into_iter()
        .map(|channel| FixtureChannel {
            slot: channel.slot,
            curve: super::patch::domain_curve(channel.curve),
            role: match channel.role {
                GuiFixtureChannelRole::Coarse { function } => FixtureChannelRole::Coarse {
                    function: FixtureFunctionId(function),
                },
                GuiFixtureChannelRole::Fine { function } => FixtureChannelRole::Fine {
                    function: FixtureFunctionId(function),
                },
                GuiFixtureChannelRole::ColorComponent {
                    function,
                    component,
                } => FixtureChannelRole::ColorComponent {
                    function: FixtureFunctionId(function),
                    component: component.into(),
                },
                GuiFixtureChannelRole::Ignored => FixtureChannelRole::Ignored,
            },
        })
        .collect();
    let behavior_rules = definition
        .behavior_rules
        .into_iter()
        .map(|rule| {
            Ok(match rule {
                GuiFixtureBehavior::Shutter {
                    function,
                    closed,
                    open,
                } => FixtureBehaviorRule::Shutter {
                    function: FixtureFunctionId(function),
                    closed: FixtureEntryId(closed),
                    open: FixtureEntryId(open),
                },
                GuiFixtureBehavior::Dimmer { function, off, on } => FixtureBehaviorRule::Dimmer {
                    function: FixtureFunctionId(function),
                    off,
                    on,
                },
                GuiFixtureBehavior::PrismGate {
                    function,
                    disabled,
                    enabled,
                } => FixtureBehaviorRule::PrismGate {
                    function: FixtureFunctionId(function),
                    disabled: FixtureEntryId(disabled),
                    enabled: FixtureEntryId(enabled),
                },
                GuiFixtureBehavior::ColorWheel { function, entries } => {
                    FixtureBehaviorRule::ColorWheel {
                        function: FixtureFunctionId(function),
                        entries: entries
                            .into_iter()
                            .map(|entry| {
                                Ok(ColorWheelColorMapping {
                                    color: parse_color(&entry.color)?,
                                    entry: FixtureEntryId(entry.entry),
                                })
                            })
                            .collect::<Result<_, GuiMutationError>>()?,
                    }
                }
            })
        })
        .collect::<Result<_, GuiMutationError>>()?;
    let profile = FixtureProfile {
        id: id.clone(),
        functions,
        channels,
        behavior_rules,
    };
    profile
        .validate()
        .map_err(|error| GuiMutationError::Invalid(error.to_string()))?;
    session
        .project
        .definitions
        .fixture_profiles
        .definitions
        .insert(id, profile);
    Ok(())
}
