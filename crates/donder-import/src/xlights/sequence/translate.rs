//! Each xLights effect of a row as a clip of its likeness in
//! `effects/xlights.donder`, with its settings in the likeness's parameters.

use super::settings::{Palette, PaletteColor, Settings, ValueCurve};
use super::xsq::{EffectName, Placed, Row};
use super::{LeftOut, SequenceImportReport};
use donder_runtime_types::{Color, TargetScope};
use std::time::Duration;

/// A likeness in `effects/xlights.donder`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Port {
    On,
    ColorWash,
    Shockwave,
    SingleStrand,
    Pinwheel,
    Galaxy,
    Wave,
    ShaderContinua,
    ShaderCosmos,
    ShaderVortex,
    ShaderStarfield,
    /// Any other shader: drifting, turning clouds in the palette's colors.
    Shader,
}

impl Port {
    /// The likeness's name in the library.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::On => "On",
            Self::ColorWash => "ColorWash",
            Self::Shockwave => "Shockwave",
            Self::SingleStrand => "SingleStrand",
            Self::Pinwheel => "Pinwheel",
            Self::Galaxy => "Galaxy",
            Self::Wave => "Wave",
            Self::ShaderContinua => "ShaderContinua",
            Self::ShaderCosmos => "ShaderCosmos",
            Self::ShaderVortex => "ShaderVortex",
            Self::ShaderStarfield => "ShaderStarfield",
            Self::Shader => "Shader",
        }
    }

    /// The start of its clips' names.
    pub(crate) fn clip_name(self) -> String {
        match self {
            Self::On => "on".into(),
            Self::ColorWash => "color_wash".into(),
            Self::Shockwave => "shockwave".into(),
            Self::SingleStrand => "single_strand".into(),
            Self::Pinwheel => "pinwheel".into(),
            Self::Galaxy => "galaxy".into(),
            Self::Wave => "wave".into(),
            Self::ShaderContinua
            | Self::ShaderCosmos
            | Self::ShaderVortex
            | Self::ShaderStarfield
            | Self::Shader => "shader".into(),
        }
    }
}

/// A parameter value, before it meets the likeness's declaration.
pub(crate) enum Value {
    Float(f32),
    Int(i32),
    Bool(bool),
    /// An enum option without fields.
    Option(&'static str),
    Gradient(Vec<(f32, Color)>),
    Gradients(Vec<Vec<(f32, Color)>>),
    /// `(position, value)` in the parameter's own units.
    Curve(Vec<(f32, f32)>),
}

/// A likeness's parameters by name.
pub(crate) type Params = Vec<(&'static str, Value)>;

/// A translated effect.
pub(crate) struct Translated {
    pub(crate) port: Port,
    pub(crate) params: Params,
    pub(crate) scope: TargetScope,
    pub(crate) start: Duration,
    pub(crate) end: Duration,
    /// Its row's layer, the top one 0.
    pub(crate) layer: usize,
    /// The xLights effect name.
    pub(crate) source: String,
}

/// How xLights mixes an effect into the layers under it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mix {
    Normal,
    /// `2 is Unmask`: the effect shows only where the layers under it are lit.
    UnmaskedByBelow,
    /// Any other mix, by its xLights name.
    Other,
}

fn mix(settings: &Settings) -> (Mix, &str) {
    let name = settings.choice("T_CHOICE_LayerMethod", "Normal");
    let mix = match name {
        "Normal" => Mix::Normal,
        "2 is Unmask" | "2 is True Unmask" => Mix::UnmaskedByBelow,
        _ => Mix::Other,
    };
    (mix, name)
}

/// How many samples a curve sampled from xLights takes across a clip.
const CURVE_SAMPLES: usize = 24;

/// A slider's value, or its value curve sampled across the effect.
fn slider(settings: &Settings, name: &str, default: f32) -> SliderValue {
    match settings.curve(name) {
        Some(curve) => SliderValue::Curve(curve),
        None => SliderValue::Fixed(
            settings
                .text(&format!("E_SLIDER_{name}"))
                .or_else(|| settings.text(&format!("E_TEXTCTRL_{name}")))
                .and_then(|text| text.trim().parse::<f32>().ok())
                .unwrap_or(default),
        ),
    }
}

enum SliderValue {
    Fixed(f32),
    Curve(ValueCurve),
}

impl SliderValue {
    /// The value at the start of the effect, scaled by `scale`.
    fn first(&self, scale: f32) -> f32 {
        match self {
            Self::Fixed(value) => value * scale,
            Self::Curve(curve) => curve.at(0.0) * scale,
        }
    }

    /// The value across the effect, scaled by `scale`.
    fn curve(&self, scale: f32) -> Vec<(f32, f32)> {
        match self {
            Self::Fixed(value) => vec![(0.0, value * scale), (1.0, value * scale)],
            Self::Curve(curve) => (0..=CURVE_SAMPLES)
                .map(|i| {
                    let t = i as f32 / CURVE_SAMPLES as f32;
                    (t, curve.at(t) * scale)
                })
                .collect(),
        }
    }
}

/// A palette color as a gradient over the effect.
fn gradient(color: &PaletteColor) -> Vec<(f32, Color)> {
    match color {
        PaletteColor::Solid(color) => vec![(0.0, *color)],
        PaletteColor::Curve(stops) => stops.clone(),
    }
}

fn palette_gradients(palette: &Palette) -> Value {
    Value::Gradients(palette.colors_or_white().iter().map(gradient).collect())
}

/// Effects drawn on each model of a group on its own, by buffer style.
fn scope(settings: &Settings) -> TargetScope {
    if settings
        .choice("B_CHOICE_BufferStyle", "Default")
        .starts_with("Per Model")
    {
        TargetScope::PerFixture
    } else {
        TargetScope::WholeTarget
    }
}

/// Whether the buffer lays the models' pixels end to end in one line.
fn single_line(settings: &Settings) -> bool {
    settings
        .choice("B_CHOICE_BufferStyle", "Default")
        .ends_with("Single Line")
}

/// The settings every likeness shares.
fn shared(placed: &Placed) -> Params {
    let settings = &placed.settings;
    vec![
        ("brightness", Value::Float(placed.palette.brightness())),
        (
            "fade_in",
            Value::Float(settings.float("T_TEXTCTRL_Fadein", 0.0)),
        ),
        (
            "fade_out",
            Value::Float(settings.float("T_TEXTCTRL_Fadeout", 0.0)),
        ),
        // Untinted; an unmasking effect above replaces it.
        (
            "tint",
            Value::Gradient(vec![(
                0.0,
                Color {
                    red: 1.0,
                    green: 1.0,
                    blue: 1.0,
                },
            )]),
        ),
    ]
}

/// The effect's own color over its span, as an `On` or `Color Wash` unmasked
/// by the layers under it shows it, at `samples` times from `from` to `to`.
fn tint_color(placed: &Placed, at: Duration) -> Color {
    let span = placed
        .end
        .saturating_sub(placed.start)
        .as_secs_f32()
        .max(1e-6);
    let t = (at.saturating_sub(placed.start).as_secs_f32() / span).clamp(0.0, 1.0);
    let colors = placed.palette.colors_or_white();
    let settings = &placed.settings;
    let (color, level) = match placed.name {
        EffectName::ColorWash => {
            let cycles = settings.float("E_TEXTCTRL_ColorWash_Cycles", 1.0).max(0.01);
            let position = (t * cycles).fract();
            (blend(&colors, position, t), 1.0)
        }
        _ => {
            let start = settings.float("E_TEXTCTRL_Eff_On_Start", 100.0) / 100.0;
            let end = settings.float("E_TEXTCTRL_Eff_On_End", 100.0) / 100.0;
            (colors[0].at(t), start + (end - start) * t)
        }
    };
    let fade_in = settings.float("T_TEXTCTRL_Fadein", 0.0);
    let fade_out = settings.float("T_TEXTCTRL_Fadeout", 0.0);
    let seconds = t * span;
    let rise = if fade_in > 0.0 {
        (seconds / fade_in).min(1.0)
    } else {
        1.0
    };
    let fall = if fade_out > 0.0 {
        ((span - seconds) / fade_out).min(1.0)
    } else {
        1.0
    };
    // `2 is Unmask` keeps the brightness of the layers under it and takes
    // only the hue and saturation of this effect; `2 is True Unmask` takes
    // its color whole.
    let (_, mix_name) = mix(settings);
    let peak = color.red.max(color.green).max(color.blue);
    let level = if mix_name == "2 is True Unmask" {
        (level * rise * fall * placed.palette.brightness()).max(0.0)
    } else if peak > 0.0 {
        1.0 / peak
    } else {
        0.0
    };
    Color {
        red: (color.red * level).min(1.0),
        green: (color.green * level).min(1.0),
        blue: (color.blue * level).min(1.0),
    }
}

/// xLights' `GetMultiColorBlend`: the palette's colors spread evenly over
/// 0..1, blended between neighbors, each color curve read at `t`.
fn blend(colors: &[PaletteColor], position: f32, t: f32) -> Color {
    if colors.len() <= 1 {
        return colors.first().map_or(Color::BLACK, |color| color.at(t));
    }
    let scaled = position.clamp(0.0, 1.0) * (colors.len() - 1) as f32;
    let index = (scaled.floor() as usize).min(colors.len() - 2);
    let k = scaled - index as f32;
    let (a, b) = (colors[index].at(t), colors[index + 1].at(t));
    Color {
        red: a.red + (b.red - a.red) * k,
        green: a.green + (b.green - a.green) * k,
        blue: a.blue + (b.blue - a.blue) * k,
    }
}

/// An unmasked effect waiting for the clips under it.
struct Unmask<'a> {
    placed: &'a Placed,
    layer: usize,
}

/// The row's effects as clips, top layer first; an `On` or `Color Wash` that
/// the layers under it unmask tints their clips instead of playing.
pub(crate) fn row(
    row: &Row,
    frame: Duration,
    report: &mut SequenceImportReport,
) -> Vec<Translated> {
    let mut clips = Vec::new();
    let mut unmasks = Vec::new();
    for (layer, effects) in row.layers.iter().enumerate() {
        for placed in effects {
            let (mixing, mix_name) = mix(&placed.settings);
            let source = placed.source_name.clone();
            if mixing == Mix::UnmaskedByBelow
                && matches!(placed.name, EffectName::On | EffectName::ColorWash)
            {
                unmasks.push(Unmask { placed, layer });
                continue;
            }
            match translate(placed, frame, report) {
                Ok(params) => {
                    let (port, params) = params;
                    if mixing == Mix::Other || mixing == Mix::UnmaskedByBelow {
                        *report.mixed_by_max.entry(mix_name.to_owned()).or_default() += 1;
                    }
                    *report.played.entry(source.clone()).or_default() += 1;
                    clips.push(Translated {
                        port,
                        params,
                        scope: scope(&placed.settings),
                        start: placed.start,
                        end: placed.end,
                        layer,
                        source,
                    });
                }
                Err(reason) => {
                    *report.left_out.entry((source, reason)).or_default() += 1;
                }
            }
        }
    }
    for unmask in unmasks {
        let (from, to) = (unmask.placed.start, unmask.placed.end);
        let mut tinted_any = false;
        for clip in clips.iter_mut().filter(|clip| clip.layer > unmask.layer) {
            let overlap = clip.end.min(to).saturating_sub(clip.start.max(from));
            let span = clip.end.saturating_sub(clip.start);
            if span.is_zero() || overlap.as_secs_f32() < span.as_secs_f32() * 0.5 {
                continue;
            }
            let stops = (0..=CURVE_SAMPLES)
                .map(|i| {
                    let t = i as f32 / CURVE_SAMPLES as f32;
                    let at = clip.start + span.mul_f32(t);
                    (t, tint_color(unmask.placed, at.clamp(from, to)))
                })
                .collect::<Vec<_>>();
            clip.params
                .retain(|(name, _)| *name != "tinted" && *name != "tint");
            clip.params.push(("tinted", Value::Bool(true)));
            clip.params.push(("tint", Value::Gradient(stops)));
            tinted_any = true;
        }
        let source = unmask.placed.source_name.clone();
        if tinted_any {
            *report.tinting.entry(source).or_default() += 1;
        } else {
            *report
                .left_out
                .entry((source, LeftOut::NothingToUnmask))
                .or_default() += 1;
        }
    }
    clips
}

/// The likeness and its parameters, or why the effect is left out.
fn translate(
    placed: &Placed,
    frame: Duration,
    report: &mut SequenceImportReport,
) -> Result<(Port, Params), LeftOut> {
    let s = &placed.settings;
    let mut params = shared(placed);
    let port = match placed.name {
        EffectName::Off => return Err(LeftOut::Off),
        EffectName::MovingHead | EffectName::Faces | EffectName::Other => {
            return Err(LeftOut::NoLikeness);
        }
        EffectName::Warp => return Err(LeftOut::Distortion),
        EffectName::On => {
            params.extend([
                ("colors", palette_gradients(&placed.palette)),
                (
                    "start_level",
                    Value::Float(s.float("E_TEXTCTRL_Eff_On_Start", 100.0) / 100.0),
                ),
                (
                    "end_level",
                    Value::Float(s.float("E_TEXTCTRL_Eff_On_End", 100.0) / 100.0),
                ),
                (
                    "shimmer",
                    Value::Bool(s.bool("E_CHECKBOX_On_Shimmer", false)),
                ),
                ("cycles", Value::Float(s.float("E_TEXTCTRL_On_Cycles", 1.0))),
            ]);
            Port::On
        }
        EffectName::ColorWash => {
            params.extend([
                ("colors", palette_gradients(&placed.palette)),
                (
                    "cycles",
                    Value::Float(s.float("E_TEXTCTRL_ColorWash_Cycles", 1.0)),
                ),
                (
                    "horizontal_fade",
                    Value::Bool(s.bool("E_CHECKBOX_ColorWash_HFade", false)),
                ),
                (
                    "vertical_fade",
                    Value::Bool(s.bool("E_CHECKBOX_ColorWash_VFade", false)),
                ),
                (
                    "reverse_fades",
                    Value::Bool(s.bool("E_CHECKBOX_ColorWash_ReverseFades", false)),
                ),
                (
                    "shimmer",
                    Value::Bool(s.bool("E_CHECKBOX_ColorWash_Shimmer", false)),
                ),
                (
                    "circular",
                    Value::Bool(s.bool("E_CHECKBOX_ColorWash_CircularPalette", false)),
                ),
            ]);
            Port::ColorWash
        }
        EffectName::Shockwave => {
            params.extend([
                ("colors", palette_gradients(&placed.palette)),
                (
                    "center_x",
                    Value::Curve(slider(s, "Shockwave_CenterX", 50.0).curve(1.0)),
                ),
                (
                    "center_y",
                    Value::Curve(slider(s, "Shockwave_CenterY", 50.0).curve(1.0)),
                ),
                (
                    "start_radius",
                    Value::Float(slider(s, "Shockwave_Start_Radius", 1.0).first(1.0)),
                ),
                (
                    "end_radius",
                    Value::Float(slider(s, "Shockwave_End_Radius", 10.0).first(1.0)),
                ),
                (
                    "start_width",
                    Value::Float(slider(s, "Shockwave_Start_Width", 5.0).first(1.0)),
                ),
                (
                    "end_width",
                    Value::Float(slider(s, "Shockwave_End_Width", 10.0).first(1.0)),
                ),
                (
                    "acceleration",
                    Value::Float(s.float("E_SLIDER_Shockwave_Accel", 0.0)),
                ),
                (
                    "cycles",
                    Value::Float(s.float("E_SLIDER_Shockwave_Cycles", 1.0)),
                ),
                (
                    "blend_edges",
                    Value::Bool(s.bool("E_CHECKBOX_Shockwave_Blend_Edges", true)),
                ),
                (
                    "scale",
                    Value::Bool(s.bool("E_CHECKBOX_Shockwave_Scale", true)),
                ),
            ]);
            Port::Shockwave
        }
        EffectName::SingleStrand => {
            let mode = match s.choice("E_NOTEBOOK_SSEFFECT_TYPE", "Chase") {
                "Skips" => "Skips",
                "FX" => return Err(LeftOut::NoLikeness),
                _ => "Chase",
            };
            let chase = match s.choice("E_CHOICE_Chase_Type1", "Left-Right") {
                "Right-Left" | "Static Right-Left" => "RightLeft",
                "Bounce from Left" | "Bounce to Middle" => "BounceFromLeft",
                "Bounce from Right" | "Bounce from Middle" => "BounceFromRight",
                "Dual Chase" | "Dual Bounce" | "Static Dual" | "Static Double-Ended" => "DualChase",
                "From Middle" | "Static From Middle" => "FromMiddle",
                "To Middle" | "Static To Middle" => "ToMiddle",
                _ => "LeftRight",
            };
            let fade = match s.choice("E_CHOICE_Fade_Type", "None") {
                "From Head" => "Head",
                "From Tail" => "Tail",
                "Head and Tail" | "Middle" => "HeadAndTail",
                _ => "None",
            };
            // A single line turned on its side is one pixel wide, so the
            // chase lights all of it; a turned buffer chases up or down it.
            let transform = s.choice("B_CHOICE_BufferTransform", "None");
            let turned_cw = transform.starts_with("Rotate CW 90");
            let turned_cc = transform.starts_with("Rotate CC 90");
            let axis = match (single_line(s), turned_cw, turned_cc) {
                (true, true, _) | (true, _, true) => "Whole",
                (true, false, false) => "Line",
                (false, true, _) => "Down",
                (false, _, true) => "Up",
                (false, false, false) => "Across",
            };
            params.extend([
                ("colors", palette_gradients(&placed.palette)),
                ("mode", Value::Option(mode)),
                ("chase_type", Value::Option(chase)),
                (
                    "rainbow",
                    Value::Bool(s.choice("E_CHOICE_SingleStrand_Colors", "Palette") == "Rainbow"),
                ),
                (
                    "chases",
                    Value::Int(slider(s, "Number_Chases", 1.0).first(1.0) as i32),
                ),
                (
                    "chase_size",
                    Value::Float(slider(s, "Color_Mix1", 10.0).first(1.0)),
                ),
                (
                    "rotations",
                    Value::Float(slider(s, "Chase_Rotations", 1.0).first(1.0)),
                ),
                (
                    "offset",
                    Value::Float(slider(s, "Chase_Offset", 0.0).first(1.0)),
                ),
                ("fade", Value::Option(fade)),
                (
                    "band_size",
                    Value::Int(s.float("E_SLIDER_Skips_BandSize", 1.0) as i32),
                ),
                (
                    "skip_size",
                    Value::Int(s.float("E_SLIDER_Skips_SkipSize", 1.0) as i32),
                ),
                (
                    "start_position",
                    Value::Int(s.float("E_SLIDER_Skips_StartPos", 1.0) as i32),
                ),
                (
                    "advance",
                    Value::Int(s.float("E_SLIDER_Skips_Advance", 0.0) as i32),
                ),
                (
                    "skip_right",
                    Value::Bool(s.choice("E_CHOICE_Skips_Direction", "Left") == "Right"),
                ),
                ("axis", Value::Option(axis)),
            ]);
            Port::SingleStrand
        }
        EffectName::Pinwheel => {
            params.extend([
                ("colors", palette_gradients(&placed.palette)),
                (
                    "arms",
                    Value::Int(s.float("E_SLIDER_Pinwheel_Arms", 3.0) as i32),
                ),
                (
                    "arm_size",
                    Value::Float(slider(s, "Pinwheel_ArmSize", 100.0).first(1.0)),
                ),
                (
                    "twist",
                    Value::Float(slider(s, "Pinwheel_Twist", 0.0).first(1.0)),
                ),
                (
                    "thickness",
                    Value::Float(slider(s, "Pinwheel_Thickness", 0.0).first(1.0)),
                ),
                (
                    "speed",
                    Value::Float(slider(s, "Pinwheel_Speed", 10.0).first(1.0)),
                ),
                (
                    "offset",
                    Value::Float(slider(s, "Pinwheel_Offset", 0.0).first(1.0)),
                ),
                (
                    "clockwise",
                    Value::Bool(s.bool("E_CHECKBOX_Pinwheel_Rotation", true)),
                ),
                (
                    "center_x",
                    Value::Float(slider(s, "PinwheelXC", 0.0).first(1.0)),
                ),
                (
                    "center_y",
                    Value::Float(slider(s, "PinwheelYC", 0.0).first(1.0)),
                ),
                (
                    "flip_vertical",
                    Value::Bool(matches!(
                        s.choice("B_CHOICE_BufferTransform", "None"),
                        "Flip Vertical" | "Rotate 180"
                    )),
                ),
                (
                    "blade",
                    Value::Option(match s.choice("E_CHOICE_Pinwheel_3D", "None") {
                        "3D" => "ThreeD",
                        "3D Inverted" => "InvertedThreeD",
                        "Sweep" => "Sweep",
                        _ => "Flat",
                    }),
                ),
            ]);
            Port::Pinwheel
        }
        EffectName::Galaxy => {
            params.extend([
                ("colors", palette_gradients(&placed.palette)),
                (
                    "center_x",
                    Value::Float(slider(s, "Galaxy_CenterX", 50.0).first(1.0)),
                ),
                (
                    "center_y",
                    Value::Float(slider(s, "Galaxy_CenterY", 50.0).first(1.0)),
                ),
                (
                    "start_radius",
                    Value::Float(slider(s, "Galaxy_Start_Radius", 1.0).first(1.0)),
                ),
                (
                    "end_radius",
                    Value::Float(slider(s, "Galaxy_End_Radius", 10.0).first(1.0)),
                ),
                (
                    "start_angle",
                    Value::Float(slider(s, "Galaxy_Start_Angle", 0.0).first(1.0)),
                ),
                (
                    "revolutions",
                    Value::Float(slider(s, "Galaxy_Revolutions", 1440.0).first(1.0)),
                ),
                (
                    "start_width",
                    Value::Float(slider(s, "Galaxy_Start_Width", 5.0).first(1.0)),
                ),
                (
                    "end_width",
                    Value::Float(slider(s, "Galaxy_End_Width", 5.0).first(1.0)),
                ),
                (
                    "duration_percent",
                    Value::Float(slider(s, "Galaxy_Duration", 20.0).first(1.0)),
                ),
                (
                    "acceleration",
                    Value::Float(slider(s, "Galaxy_Accel", 0.0).first(1.0)),
                ),
                (
                    "reverse",
                    Value::Bool(s.bool("E_CHECKBOX_Galaxy_Reverse", false)),
                ),
                (
                    "blend_edges",
                    Value::Bool(s.bool("E_CHECKBOX_Galaxy_Blend_Edges", true)),
                ),
                (
                    "scale",
                    Value::Bool(s.bool("E_CHECKBOX_Galaxy_Scale", true)),
                ),
            ]);
            Port::Galaxy
        }
        EffectName::Wave => {
            params.extend([
                ("colors", palette_gradients(&placed.palette)),
                (
                    "wave_type",
                    Value::Option(match s.choice("E_CHOICE_Wave_Type", "Sine") {
                        "Triangle" => "Triangle",
                        "Square" => "Square",
                        "Decaying Sine" => "DecayingSine",
                        _ => "Sine",
                    }),
                ),
                (
                    "fill",
                    Value::Option(match s.choice("E_CHOICE_Fill_Colors", "None") {
                        "Rainbow" => "Rainbow",
                        "Palette" => "Palette",
                        _ => "None",
                    }),
                ),
                (
                    "mirror",
                    Value::Bool(s.bool("E_CHECKBOX_Mirror_Wave", false)),
                ),
                (
                    "waves",
                    Value::Float(s.float("E_TEXTCTRL_Number_Waves", 2.5)),
                ),
                (
                    "thickness",
                    Value::Float(slider(s, "Thickness_Percentage", 5.0).first(1.0)),
                ),
                (
                    "height",
                    Value::Float(slider(s, "Wave_Height", 50.0).first(1.0)),
                ),
                (
                    "speed",
                    Value::Float(s.float("E_TEXTCTRL_Wave_Speed", 10.0)),
                ),
                (
                    "y_offset",
                    Value::Float(slider(s, "Wave_YOffset", 0.0).first(1.0)),
                ),
                (
                    "right_to_left",
                    Value::Bool(
                        s.choice("E_CHOICE_Wave_Direction", "Right to Left") == "Right to Left",
                    ),
                ),
            ]);
            Port::Wave
        }
        EffectName::Shader => {
            let file = s.text("E_0FILEPICKERCTRL_IFS").unwrap_or_default();
            let file_name = file.rsplit('/').next().unwrap_or(file).to_owned();
            let port = match file_name.as_str() {
                "Continua Variation.fs" => Port::ShaderContinua,
                "Black Cherry Cosmos.fs" | "BlackCherryCosmos2+.fs" => Port::ShaderCosmos,
                "Vortex V2.fs" => Port::ShaderVortex,
                "Euro Vision Triangle Starfield.fs" => Port::ShaderStarfield,
                _ => {
                    *report.general_shaders.entry(file_name).or_default() += 1;
                    Port::Shader
                }
            };
            params.extend([
                ("colors", palette_gradients(&placed.palette)),
                (
                    "speed",
                    Value::Curve(slider(s, "Shader_Speed", 100.0).curve(1.0)),
                ),
                (
                    "zoom",
                    Value::Float(slider(s, "Shader_Zoom", 0.0).first(1.0)),
                ),
                (
                    "offset_x",
                    Value::Curve(slider(s, "Shader_Offset_X", 0.0).curve(1.0)),
                ),
                (
                    "offset_y",
                    Value::Curve(slider(s, "Shader_Offset_Y", 0.0).curve(1.0)),
                ),
                // Lead-in counts frames.
                (
                    "lead_in",
                    Value::Float(s.float("E_TEXTCTRL_Shader_LeadIn", 0.0) * frame.as_secs_f32()),
                ),
            ]);
            if port == Port::ShaderVortex {
                let uniform = |name: &str, default: f32| {
                    s.float(&format!("E_SLIDER_SHADERXYZZY_{name}"), default * 100.0) / 100.0
                };
                params.extend([
                    ("stripes", Value::Float(uniform("Stripe", 29.0))),
                    ("rotation", Value::Float(uniform("Rotation", 0.5))),
                    ("color_g", Value::Float(uniform("ColorG", 0.0))),
                    ("color_b", Value::Float(uniform("ColorB", 0.0))),
                ]);
            }
            port
        }
    };
    Ok((port, params))
}
