//! Each xLights effect of a row as a clip of its likeness in
//! `effects/xlights.donder`, with its settings in the likeness's parameters.

use super::settings::{Palette, PaletteColor, Settings, ValueCurve};
use super::xsq::{EffectName, Placed, Row};
use super::{LeftOut, SequenceImportReport};
use donder_runtime_types::{Color, TargetScope};
use std::time::Duration;

const WHITE: Color = Color {
    red: 1.0,
    green: 1.0,
    blue: 1.0,
};

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
    Morph,
    Ripple,
    Lightning,
    Spirals,
    Shape,
    VuMeter,
    Meteors,
    Garlands,
    Fan,
    Bars,
    Twinkle,
    Fire,
    Butterfly,
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
            Self::Morph => "Morph",
            Self::Ripple => "Ripple",
            Self::Lightning => "Lightning",
            Self::Spirals => "Spirals",
            Self::Shape => "Shape",
            Self::VuMeter => "VuMeter",
            Self::Meteors => "Meteors",
            Self::Garlands => "Garlands",
            Self::Fan => "Fan",
            Self::Bars => "Bars",
            Self::Twinkle => "Twinkle",
            Self::Fire => "Fire",
            Self::Butterfly => "Butterfly",
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
            Self::Morph => "morph".into(),
            Self::Ripple => "ripple".into(),
            Self::Lightning => "lightning".into(),
            Self::Spirals => "spirals".into(),
            Self::Shape => "shape".into(),
            Self::VuMeter => "vu_meter".into(),
            Self::Meteors => "meteors".into(),
            Self::Garlands => "garlands".into(),
            Self::Fan => "fan".into(),
            Self::Bars => "bars".into(),
            Self::Twinkle => "twinkle".into(),
            Self::Fire => "fire".into(),
            Self::Butterfly => "butterfly".into(),
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
    /// The mark collection made from the timing track of this name.
    Marks(String),
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
    /// `2 is Unmask`: the effect's hue and saturation show only where the
    /// layers under it are lit, at their brightness.
    Unmask,
    /// `2 is True Unmask`: the effect shows whole only where the layers under
    /// it are lit.
    TrueUnmask,
    /// Any other mix, by its xLights name.
    Other,
}

fn mix(settings: &Settings) -> (Mix, &str) {
    let name = settings.choice("T_CHOICE_LayerMethod", "Normal");
    let mix = match name {
        "Normal" => Mix::Normal,
        "2 is Unmask" => Mix::Unmask,
        "2 is True Unmask" => Mix::TrueUnmask,
        _ => Mix::Other,
    };
    (mix, name)
}

/// How many samples a tint takes across a clip: fine enough that a tint
/// ending partway shows its edge within a short step.
const TINT_SAMPLES: usize = 48;

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

/// How an xLights buffer lays out the target: the likenesses' `space`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Space {
    Layout,
    Line,
    MemberColumns,
    StrandColumns,
    MemberRows,
    StrandRows,
}

/// An xLights buffer transform: the likenesses' `turn`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Turn {
    None,
    Half,
    FlipVertical,
    FlipHorizontal,
    Clockwise,
    Counterclockwise,
}

/// Where an effect draws: on each member or the whole target, the buffer's
/// layout and its transform.
struct Buffer {
    scope: TargetScope,
    space: Space,
    turn: Turn,
}

impl Buffer {
    fn read(settings: &Settings) -> Self {
        use Space::*;
        let style = settings.choice("B_CHOICE_BufferStyle", "Default");
        // `Per Model <style>` draws `<style>` on each member of the group.
        let (scope, style) = match style.strip_prefix("Per Model ") {
            Some(style) => (TargetScope::PerFixture, style),
            None => (TargetScope::WholeTarget, style),
        };
        let (scope, space) = match style {
            "Single Line" => (scope, Line),
            "Horizontal Per Model" => (scope, MemberColumns),
            "Vertical Per Model" => (scope, MemberRows),
            "Horizontal Per Model/Strand" | "Horizontal Per Strand" => (scope, StrandColumns),
            "Vertical Per Model/Strand" | "Vertical Per Strand" => (scope, StrandRows),
            // Members overlaid or stacked one above another each fill the
            // buffer's width: much as each draws on its own.
            "Overlay - Centered"
            | "Overlay - Scaled"
            | "Vertical Stack"
            | "Vertical Stack - Scaled" => (TargetScope::PerFixture, Layout),
            _ => (scope, Layout),
        };
        let turn = match settings.choice("B_CHOICE_BufferTransform", "None") {
            "Rotate 180" => Turn::Half,
            "Flip Vertical" => Turn::FlipVertical,
            "Flip Horizontal" => Turn::FlipHorizontal,
            "Rotate CW 90" | "Rotate CW 90 Flip Horizontal" => Turn::Clockwise,
            "Rotate CC 90" | "Rotate CC 90 Flip Horizontal" => Turn::Counterclockwise,
            _ => Turn::None,
        };
        Self { scope, space, turn }
    }

    fn params(&self) -> Params {
        let space = match self.space {
            Space::Layout => "Layout",
            Space::Line => "Line",
            Space::MemberColumns => "MemberColumns",
            Space::StrandColumns => "StrandColumns",
            Space::MemberRows => "MemberRows",
            Space::StrandRows => "StrandRows",
        };
        let turn = match self.turn {
            Turn::None => "None",
            Turn::Half => "HalfTurn",
            Turn::FlipVertical => "FlipVertical",
            Turn::FlipHorizontal => "FlipHorizontal",
            Turn::Clockwise => "Clockwise",
            Turn::Counterclockwise => "Counterclockwise",
        };
        vec![
            ("space", Value::Option(space)),
            ("turn", Value::Option(turn)),
        ]
    }

    /// Whether the buffer is one pixel wide: a single line turned on its side.
    fn one_wide(&self) -> bool {
        self.space == Space::Line && matches!(self.turn, Turn::Clockwise | Turn::Counterclockwise)
    }
}

/// The settings every likeness shares.
fn shared(placed: &Placed) -> Params {
    let settings = &placed.settings;
    let mut params = Buffer::read(settings).params();
    params.extend([
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
        ("tint", Value::Gradient(vec![(0.0, WHITE)])),
    ]);
    params
}

/// The hue and saturation an `On` or `Color Wash` gives the layers under it
/// that unmask it, at full value: `2 is Unmask` keeps those layers'
/// brightness. (`2 is True Unmask` shows the upper effect at its own
/// brightness; it is imported the same way.)
fn tint_color(placed: &Placed, at: Duration) -> Color {
    let span = placed
        .end
        .saturating_sub(placed.start)
        .as_secs_f32()
        .max(1e-6);
    let t = (at.saturating_sub(placed.start).as_secs_f32() / span).clamp(0.0, 1.0);
    let colors = placed.palette.colors_or_white();
    let color = match placed.name {
        EffectName::ColorWash => {
            let cycles = placed
                .settings
                .float("E_TEXTCTRL_ColorWash_Cycles", 1.0)
                .max(0.01);
            blend(&colors, (t * cycles).fract(), t)
        }
        _ => colors[0].at(t),
    };
    let peak = color.red.max(color.green).max(color.blue);
    // Black has no hue: it unmasks as white.
    let scale = if peak > 0.0 { 1.0 / peak } else { 0.0 };
    if peak > 0.0 {
        Color {
            red: color.red * scale,
            green: color.green * scale,
            blue: color.blue * scale,
        }
    } else {
        WHITE
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
    version: (u32, u32),
    report: &mut SequenceImportReport,
) -> Vec<Translated> {
    let mut clips = Vec::new();
    let mut unmasks = Vec::new();
    for (layer, effects) in row.layers.iter().enumerate() {
        for placed in effects {
            for curve in placed
                .settings
                .unknown_curves()
                .chain(placed.palette.unknown_curves())
            {
                *report.unknown_curves.entry(curve).or_default() += 1;
            }
            let (mixing, mix_name) = mix(&placed.settings);
            let source = placed.source_name.clone();
            if matches!(mixing, Mix::Unmask | Mix::TrueUnmask)
                && matches!(placed.name, EffectName::On | EffectName::ColorWash)
            {
                unmasks.push(Unmask { placed, layer });
                continue;
            }
            match translate(placed, frame, version, report) {
                Ok(params) => {
                    let (port, params) = params;
                    if mixing != Mix::Normal {
                        *report.mixed_by_max.entry(mix_name.to_owned()).or_default() += 1;
                    }
                    clips.push(Translated {
                        port,
                        params,
                        scope: Buffer::read(&placed.settings).scope,
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
    // Each clip under unmasking effects takes, at each moment, the tint of
    // the topmost one playing then, as xLights mixes them bottom up; where
    // none plays, the tint is black and the clip keeps its own color.
    let mut used = vec![false; unmasks.len()];
    for clip in &mut clips {
        let span = clip.end.saturating_sub(clip.start);
        let over = unmasks
            .iter()
            .enumerate()
            .filter(|(_, unmask)| {
                unmask.layer < clip.layer
                    && unmask.placed.start < clip.end
                    && unmask.placed.end > clip.start
            })
            .collect::<Vec<_>>();
        if span.is_zero() || over.is_empty() {
            continue;
        }
        let mut covered = false;
        let stops = (0..=TINT_SAMPLES)
            .map(|i| {
                let t = i as f32 / TINT_SAMPLES as f32;
                let at = clip.start + span.mul_f32(t);
                let top = over
                    .iter()
                    .filter(|(_, unmask)| unmask.placed.start <= at && at <= unmask.placed.end)
                    .min_by_key(|(_, unmask)| unmask.layer);
                match top {
                    Some((index, unmask)) => {
                        used[*index] = true;
                        covered = true;
                        (t, tint_color(unmask.placed, at))
                    }
                    None => (t, Color::BLACK),
                }
            })
            .collect::<Vec<_>>();
        if covered {
            clip.params
                .retain(|(name, _)| *name != "tinted" && *name != "tint");
            clip.params.push(("tinted", Value::Bool(true)));
            clip.params.push(("tint", Value::Gradient(stops)));
        }
    }
    for (unmask, used) in unmasks.iter().zip(used) {
        let source = unmask.placed.source_name.clone();
        if used {
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
    version: (u32, u32),
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
            // Before xLights 2024.15 the fade was a 3D fade checkbox, on
            // meaning from the head.
            let legacy_fade = if s.bool("E_CHECKBOX_Chase_3dFade1", false) {
                "From Head"
            } else {
                "None"
            };
            let fade = match s.choice("E_CHOICE_Fade_Type", legacy_fade) {
                "From Head" => "Head",
                "From Tail" => "Tail",
                "Head and Tail" | "Middle" => "HeadAndTail",
                _ => "None",
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
                ("whole", Value::Bool(Buffer::read(s).one_wide())),
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
                    Value::Curve(slider(s, "Pinwheel_ArmSize", 100.0).curve(1.0)),
                ),
                (
                    "twist",
                    Value::Float(slider(s, "Pinwheel_Twist", 0.0).first(1.0)),
                ),
                (
                    "thickness",
                    Value::Curve(slider(s, "Pinwheel_Thickness", 0.0).curve(1.0)),
                ),
                (
                    "speed",
                    Value::Curve(slider(s, "Pinwheel_Speed", 10.0).curve(1.0)),
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
        EffectName::Morph => {
            let n = |name: &str, default: f32| Value::Float(slider(s, name, default).first(1.0));
            params.extend([
                ("colors", palette_gradients(&placed.palette)),
                ("start_x1", n("Morph_Start_X1", 0.0)),
                ("start_y1", n("Morph_Start_Y1", 0.0)),
                ("start_x2", n("Morph_Start_X2", 100.0)),
                ("start_y2", n("Morph_Start_Y2", 0.0)),
                ("end_x1", n("Morph_End_X1", 0.0)),
                ("end_y1", n("Morph_End_Y1", 100.0)),
                ("end_x2", n("Morph_End_X2", 100.0)),
                ("end_y2", n("Morph_End_Y2", 100.0)),
                ("start_length", n("MorphStartLength", 1.0)),
                ("end_length", n("MorphEndLength", 1.0)),
                ("duration_percent", n("MorphDuration", 20.0)),
                ("acceleration", n("MorphAccel", 0.0)),
            ]);
            Port::Morph
        }
        EffectName::Ripple => {
            params.extend([
                ("colors", palette_gradients(&placed.palette)),
                (
                    "thickness",
                    Value::Int(slider(s, "Ripple_Thickness", 3.0).first(1.0) as i32),
                ),
                (
                    "cycles",
                    Value::Float(slider(s, "Ripple_Cycles", 1.0).first(1.0)),
                ),
                (
                    "center_x",
                    Value::Float(slider(s, "Ripple_XC", 0.0).first(1.0)),
                ),
                (
                    "center_y",
                    Value::Float(slider(s, "Ripple_YC", 0.0).first(1.0)),
                ),
                (
                    "implode",
                    Value::Bool(s.choice("E_CHOICE_Ripple_Movement", "Explode") == "Implode"),
                ),
                ("three_d", Value::Bool(s.bool("E_CHECKBOX_Ripple3D", false))),
            ]);
            Port::Ripple
        }
        EffectName::Lightning => {
            params.extend([
                ("colors", palette_gradients(&placed.palette)),
                (
                    "bolts",
                    Value::Int(slider(s, "Number_Bolts", 10.0).first(1.0) as i32),
                ),
                (
                    "segments",
                    Value::Int(slider(s, "Number_Segments", 5.0).first(1.0) as i32),
                ),
                (
                    "forked",
                    Value::Bool(s.bool("E_CHECKBOX_ForkedLightning", false)),
                ),
                (
                    "top_x",
                    Value::Float(slider(s, "Lightning_TopX", 0.0).first(1.0)),
                ),
                (
                    "top_y",
                    Value::Float(slider(s, "Lightning_TopY", 0.0).first(1.0)),
                ),
                (
                    "drift",
                    Value::Float(s.float("E_SLIDER_Lightning_BOTX", 0.0)),
                ),
                (
                    "width",
                    Value::Int(s.float("E_SLIDER_Lightning_WIDTH", 1.0) as i32),
                ),
                (
                    "down",
                    Value::Bool(s.choice("E_CHOICE_Lightning_Direction", "Up") == "Down"),
                ),
            ]);
            Port::Lightning
        }
        EffectName::Spirals => {
            params.extend([
                ("colors", palette_gradients(&placed.palette)),
                (
                    "repeats",
                    Value::Int(slider(s, "Spirals_Count", 1.0).first(1.0) as i32),
                ),
                // Rotation is used as the slider's raw number, 20 for 2.0.
                (
                    "rotation",
                    Value::Curve(slider(s, "Spirals_Rotation", 20.0).curve(1.0)),
                ),
                (
                    "thickness",
                    Value::Curve(slider(s, "Spirals_Thickness", 50.0).curve(1.0)),
                ),
                (
                    "movement",
                    Value::Float(slider(s, "Spirals_Movement", 1.0).first(1.0)),
                ),
                (
                    "three_d",
                    Value::Bool(s.bool("E_CHECKBOX_Spirals_3D", false)),
                ),
            ]);
            Port::Spirals
        }
        EffectName::Shape => {
            let shapes = slider(s, "Shape_Count", 5.0).curve(1.0);
            let most = shapes
                .iter()
                .map(|(_, count)| *count)
                .fold(1.0_f32, f32::max);
            params.extend([
                ("colors", palette_gradients(&placed.palette)),
                (
                    "star",
                    Value::Bool(s.choice("E_CHOICE_Shape_ObjectToDraw", "Circle") == "Star"),
                ),
                (
                    "points",
                    Value::Int(s.float("E_SLIDER_Shape_Points", 5.0) as i32),
                ),
                (
                    "thickness",
                    Value::Float(slider(s, "Shape_Thickness", 1.0).first(1.0)),
                ),
                ("shapes", Value::Curve(shapes)),
                ("most", Value::Int(most.ceil() as i32)),
                (
                    "start_size",
                    Value::Float(slider(s, "Shape_StartSize", 1.0).first(1.0)),
                ),
                (
                    "lifetime",
                    Value::Float(slider(s, "Shape_Lifetime", 5.0).first(1.0)),
                ),
                (
                    "growth",
                    Value::Float(slider(s, "Shape_Growth", 10.0).first(1.0)),
                ),
                (
                    "random_initial",
                    Value::Bool(s.bool("E_CHECKBOX_Shape_RandomInitial", true)),
                ),
                (
                    "fade_away",
                    Value::Bool(s.bool("E_CHECKBOX_Shape_FadeAway", true)),
                ),
            ]);
            Port::Shape
        }
        EffectName::VuMeter => {
            let kind = match s.choice("E_CHOICE_VUMeter_Type", "Waveform") {
                "Timing Event Pulse Color" => "Pulse",
                "Timing Event Bar" => "Bar",
                _ => return Err(LeftOut::NoLikeness),
            };
            params.extend([
                ("colors", palette_gradients(&placed.palette)),
                (
                    "beats",
                    Value::Marks(s.choice("E_CHOICE_VUMeter_TimingTrack", "").to_owned()),
                ),
                ("kind", Value::Option(kind)),
                (
                    "bars",
                    Value::Int(s.float("E_SLIDER_VUMeter_Bars", 6.0) as i32),
                ),
            ]);
            Port::VuMeter
        }
        EffectName::Meteors => {
            let motion = match s.choice("E_CHOICE_Meteors_Effect", "Down") {
                "Up" => "Up",
                "Left" => "Left",
                "Right" => "Right",
                "Implode" => "Implode",
                "Explode" => "Explode",
                _ => "Down",
            };
            params.extend([
                ("colors", palette_gradients(&placed.palette)),
                ("motion", Value::Option(motion)),
                (
                    "amount",
                    Value::Float(slider(s, "Meteors_Count", 10.0).first(1.0)),
                ),
                (
                    "length",
                    Value::Float(slider(s, "Meteors_Length", 25.0).first(1.0)),
                ),
                (
                    "speed",
                    Value::Curve(slider(s, "Meteors_Speed", 10.0).curve(1.0)),
                ),
                (
                    "x_offset",
                    Value::Float(slider(s, "Meteors_XOffset", 0.0).first(1.0)),
                ),
                (
                    "y_offset",
                    Value::Float(slider(s, "Meteors_YOffset", 0.0).first(1.0)),
                ),
            ]);
            Port::Meteors
        }
        EffectName::Garlands => {
            let direction = match s.choice("E_CHOICE_Garlands_Direction", "Up") {
                "Down" | "Down then Up" => "Down",
                "Left" | "Left then Right" => "Left",
                "Right" | "Right then Left" => "Right",
                _ => "Up",
            };
            params.extend([
                ("colors", palette_gradients(&placed.palette)),
                (
                    "spacing",
                    Value::Float(slider(s, "Garlands_Spacing", 10.0).first(1.0)),
                ),
                (
                    "cycles",
                    Value::Float(slider(s, "Garlands_Cycles", 1.0).first(1.0)),
                ),
                (
                    "garland_type",
                    Value::Int(s.float("E_SLIDER_Garlands_Type", 0.0) as i32),
                ),
                ("direction", Value::Option(direction)),
            ]);
            Port::Garlands
        }
        EffectName::Fan => {
            let curve =
                |name: &str, default: f32| Value::Curve(slider(s, name, default).curve(1.0));
            let n = |name: &str, default: f32| Value::Float(slider(s, name, default).first(1.0));
            params.extend([
                ("colors", palette_gradients(&placed.palette)),
                ("center_x", curve("Fan_CenterX", 50.0)),
                ("center_y", curve("Fan_CenterY", 50.0)),
                ("start_radius", curve("Fan_Start_Radius", 1.0)),
                ("end_radius", curve("Fan_End_Radius", 50.0)),
                ("start_angle", n("Fan_Start_Angle", 0.0)),
                ("revolutions", n("Fan_Revolutions", 720.0)),
                (
                    "blades",
                    Value::Int(slider(s, "Fan_Num_Blades", 3.0).first(1.0) as i32),
                ),
                ("blade_width", n("Fan_Blade_Width", 50.0)),
                ("blade_angle", n("Fan_Blade_Angle", 90.0)),
                (
                    "elements",
                    Value::Int(slider(s, "Fan_Num_Elements", 1.0).first(1.0) as i32),
                ),
                ("element_width", n("Fan_Element_Width", 100.0)),
                ("duration_percent", n("Fan_Duration", 80.0)),
                ("acceleration", n("Fan_Accel", 0.0)),
                (
                    "reverse",
                    Value::Bool(s.bool("E_CHECKBOX_Fan_Reverse", false)),
                ),
                (
                    "blend_edges",
                    Value::Bool(s.bool("E_CHECKBOX_Fan_Blend_Edges", true)),
                ),
                // Before 2025.04 a fan's radii were pixels, and xLights loads
                // a file from then without the setting as unscaled.
                (
                    "scale",
                    Value::Bool(s.bool("E_CHECKBOX_Fan_Scale", version >= (2025, 4))),
                ),
            ]);
            Port::Fan
        }
        EffectName::Bars => {
            params.extend([
                ("colors", palette_gradients(&placed.palette)),
                (
                    "repeats",
                    Value::Int(slider(s, "Bars_BarCount", 1.0).first(1.0) as i32),
                ),
                (
                    "cycles",
                    Value::Float(slider(s, "Bars_Cycles", 1.0).first(1.0)),
                ),
                (
                    "gradient_bars",
                    Value::Bool(s.bool("E_CHECKBOX_Bars_Gradient", false)),
                ),
                ("three_d", Value::Bool(s.bool("E_CHECKBOX_Bars_3D", false))),
                (
                    "down",
                    Value::Bool(s.choice("E_CHOICE_Bars_Direction", "up") == "down"),
                ),
            ]);
            Port::Bars
        }
        EffectName::Twinkle => {
            params.extend([
                ("colors", palette_gradients(&placed.palette)),
                (
                    "density",
                    Value::Curve(slider(s, "Twinkle_Count", 3.0).curve(1.0)),
                ),
                (
                    "steps",
                    Value::Int(slider(s, "Twinkle_Steps", 30.0).first(1.0) as i32),
                ),
                (
                    "strobe",
                    Value::Bool(s.bool("E_CHECKBOX_Twinkle_Strobe", false)),
                ),
            ]);
            Port::Twinkle
        }
        EffectName::Fire => {
            let location = match s.choice("E_CHOICE_Fire_Location", "Bottom") {
                "Top" => "Top",
                "Left" => "Left",
                "Right" => "Right",
                _ => "Bottom",
            };
            params.extend([
                ("colors", palette_gradients(&placed.palette)),
                (
                    "height",
                    Value::Float(slider(s, "Fire_Height", 50.0).first(1.0)),
                ),
                (
                    "hue_shift",
                    Value::Float(slider(s, "Fire_HueShift", 0.0).first(1.0)),
                ),
                (
                    "growth_cycles",
                    Value::Float(slider(s, "Fire_GrowthCycles", 0.0).first(1.0)),
                ),
                ("location", Value::Option(location)),
            ]);
            Port::Fire
        }
        EffectName::Butterfly => {
            params.extend([
                ("colors", palette_gradients(&placed.palette)),
                (
                    "chunks",
                    Value::Int(slider(s, "Butterfly_Chunks", 1.0).first(1.0) as i32),
                ),
                (
                    "skip",
                    Value::Int(slider(s, "Butterfly_Skip", 2.0).first(1.0) as i32),
                ),
                (
                    "speed",
                    Value::Float(slider(s, "Butterfly_Speed", 10.0).first(1.0)),
                ),
                (
                    "reverse",
                    Value::Bool(s.choice("E_CHOICE_Butterfly_Direction", "Normal") == "Reverse"),
                ),
            ]);
            Port::Butterfly
        }
        EffectName::Shader => {
            let file = s.text("E_0FILEPICKERCTRL_IFS").unwrap_or_default();
            // Windows writes the path with backslashes.
            let file_name = file.rsplit(['/', '\\']).next().unwrap_or(file).to_owned();
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
