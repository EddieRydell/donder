//! The effect language's builtin names: functions, context values, methods,
//! reducers and type names. Each is listed once here; name resolution, type
//! checking, editor analysis and `docs/effect_builtins.md` all read these
//! tables.
use super::check::Checker;
use super::ir::{Binary, Context, Node, Op, Quaternary, Reducer as Tabulation, Ternary, Unary};
use super::syntax::ast::ReducerKind;
use core::fmt::Write;
use donder_runtime_types::{Identifier, Type};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BuiltinGroup {
    Math,
    Rounding,
    MissingValues,
    Color,
    Resources,
    Events,
    Sections,
    Random,
    Collections,
    Audio,
    Text,
    Images,
}

impl BuiltinGroup {
    const ALL: [Self; 12] = [
        Self::Math,
        Self::Rounding,
        Self::MissingValues,
        Self::Color,
        Self::Resources,
        Self::Events,
        Self::Sections,
        Self::Random,
        Self::Collections,
        Self::Audio,
        Self::Text,
        Self::Images,
    ];

    fn title(self) -> &'static str {
        match self {
            Self::Math => "Math",
            Self::Rounding => "Rounding and conversion",
            Self::MissingValues => "Missing values",
            Self::Color => "Color",
            Self::Resources => "Curves and gradients",
            Self::Events => "Marks and crossings",
            Self::Sections => "Sections",
            Self::Random => "Randomness",
            Self::Collections => "Collections",
            Self::Audio => "Audio",
            Self::Text => "Text",
            Self::Images => "Images",
        }
    }
}

/// What an argument of a builtin takes.
#[derive(Debug)]
pub enum ArgType {
    /// A value of this type; a float also takes an int, widened.
    Is(Type),
    /// An array of any items.
    Array,
    /// A text: a `text` value or a table of character codes.
    Text,
}

/// How an argument fits an [`ArgType`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Fit {
    Exact,
    /// An int where a float is taken.
    Widened,
}

impl ArgType {
    pub(crate) fn fit(&self, actual: &Type) -> Option<Fit> {
        match (self, actual) {
            (Self::Is(expected), actual) if expected == actual => Some(Fit::Exact),
            (Self::Is(Type::Float), Type::Int) => Some(Fit::Widened),
            (Self::Array, Type::Array(_)) => Some(Fit::Exact),
            (Self::Text, Type::Text) => Some(Fit::Exact),
            (Self::Text, Type::Array(item)) if **item == Type::Int => Some(Fit::Exact),
            _ => None,
        }
    }

    /// The type as scripts spell it.
    pub(crate) fn text(&self) -> String {
        match self {
            Self::Is(ty) => type_text(ty),
            Self::Array => format!("{ARRAY_WORD}<T>"),
            Self::Text => "text".into(),
        }
    }

    /// The type as diagnostics name it.
    pub(crate) fn describe(&self) -> String {
        match self {
            Self::Is(ty) => describe(ty),
            Self::Array => format!("an {ARRAY_WORD}"),
            Self::Text => "a text".into(),
        }
    }
}

/// What a call of one signature becomes in the IR.
#[derive(Debug)]
pub(crate) enum Lowering {
    Unary(Unary),
    Binary(Binary),
    Ternary(Ternary),
    /// Several operations, built from the arguments.
    Expand(fn(&mut Checker, &[Node]) -> Node),
    /// The integral of the float parameter the argument names.
    Integral,
    /// A read of an analysis of the sequence's audio.
    Audio(super::audio::AudioKind),
}

/// One way to call a builtin: argument names with types, the result type,
/// and the operation it lowers to.
#[derive(Debug)]
pub struct Signature {
    pub(crate) args: &'static [(&'static str, ArgType)],
    result: Type,
    pub(crate) lowering: Lowering,
}

impl Signature {
    /// `name(a: float, b: int) -> float`.
    pub(crate) fn label(&self, name: &str) -> String {
        label(name, self.args, &self.result)
    }
}

fn label(name: &str, args: &[(&str, ArgType)], result: &Type) -> String {
    format!(
        "{name}({}) -> {}",
        args.iter()
            .map(|(arg, ty)| format!("{arg}: {}", ty.text()))
            .collect::<Vec<_>>()
            .join(", "),
        type_text(result)
    )
}

#[derive(Debug)]
pub struct Builtin {
    pub name: &'static str,
    group: BuiltinGroup,
    /// Overloads: a call takes one its arguments fit exactly, else one they
    /// fit with ints widened to floats.
    pub(crate) signatures: &'static [Signature],
    pub(crate) summary: &'static str,
    /// Edge cases: NaN, ranges, units and conversions.
    pub(crate) details: &'static str,
    pub(crate) example: &'static str,
}

impl Builtin {
    pub(crate) fn arity(&self) -> usize {
        self.signatures[0].args.len()
    }
}

const fn sig(
    args: &'static [(&'static str, ArgType)],
    result: Type,
    lowering: Lowering,
) -> Signature {
    Signature {
        args,
        result,
        lowering,
    }
}

const INT: ArgType = ArgType::Is(Type::Int);
const FLOAT: ArgType = ArgType::Is(Type::Float);
const COLOR: ArgType = ArgType::Is(Type::Color);
const CURVE: ArgType = ArgType::Is(Type::Curve);
const GRADIENT: ArgType = ArgType::Is(Type::Gradient);
const MARKS: ArgType = ArgType::Is(Type::Marks);
const ANY_ARRAY: ArgType = ArgType::Array;
const TEXT: ArgType = ArgType::Text;
const FONT: ArgType = ArgType::Is(Type::Font);
const IMAGE: ArgType = ArgType::Is(Type::Image);
const X: &[(&str, ArgType)] = &[("x", FLOAT)];

/// The most characters a text holds, as tables count them.
const TEXT_LENGTH: i32 = donder_runtime_types::TEXT_LENGTH as i32;

use BuiltinGroup as G;
use Lowering::{Binary as B, Expand as E, Ternary as T, Unary as U};

pub const BUILTINS: &[Builtin] = &[
    Builtin {
        name: "sin",
        group: G::Math,
        signatures: &[sig(X, Type::Float, U(Unary::Sin))],
        summary: "Sine of an angle in radians.",
        details: "NaN gives NaN. Never folded at compile time, so host and device agree.",
        example: "let wave = sin(time * TAU);",
    },
    Builtin {
        name: "cos",
        group: G::Math,
        signatures: &[sig(X, Type::Float, U(Unary::Cos))],
        summary: "Cosine of an angle in radians.",
        details: "NaN gives NaN.",
        example: "let wave = cos(pixel.fraction * PI);",
    },
    Builtin {
        name: "tan",
        group: G::Math,
        signatures: &[sig(X, Type::Float, U(Unary::Tan))],
        summary: "Tangent of an angle in radians.",
        details: "Grows without bound near odd multiples of PI / 2. NaN gives NaN.",
        example: "let slope = tan(angle);",
    },
    Builtin {
        name: "atan2",
        group: G::Math,
        signatures: &[sig(
            &[("y", FLOAT), ("x", FLOAT)],
            Type::Float,
            B(Binary::Atan2),
        )],
        summary: "Angle of the point (x, y) from the positive x axis, in radians from -PI to PI.",
        details: "Note the argument order: y first.",
        example: "let angle = atan2(pixel.y - 1.0, pixel.x - 1.0);",
    },
    Builtin {
        name: "sqrt",
        group: G::Math,
        signatures: &[sig(X, Type::Float, U(Unary::Sqrt))],
        summary: "Square root.",
        details: "Negative numbers and NaN give NaN.",
        example: "let distance = sqrt(dx * dx + dy * dy);",
    },
    Builtin {
        name: "exp",
        group: G::Math,
        signatures: &[sig(X, Type::Float, U(Unary::Exp))],
        summary: "e raised to `x`.",
        details: "Large inputs give infinity. NaN gives NaN.",
        example: "let decay = exp(-age * 4.0);",
    },
    Builtin {
        name: "log",
        group: G::Math,
        signatures: &[sig(X, Type::Float, U(Unary::Log))],
        summary: "Natural logarithm.",
        details: "Zero gives negative infinity; negative numbers and NaN give NaN.",
        example: "let octave = log(frequency / 440.0) / log(2.0);",
    },
    Builtin {
        name: "pow",
        group: G::Math,
        signatures: &[
            sig(&[("base", FLOAT), ("exponent", INT)], Type::Float, E(power)),
            sig(
                &[("base", FLOAT), ("exponent", FLOAT)],
                Type::Float,
                B(Binary::PowerFloat),
            ),
        ],
        summary: "`base` raised to `exponent`.",
        details: "An int exponent that the compiler can bound to 0..10,000 multiplies `base` into one that many times, exactly. Any other exponent, including negative ints, uses the general power function: `pow(2, -1)` is 0.5. A negative base with a fractional exponent is NaN.",
        example: "let falloff = pow(1.0 - progress, 3);",
    },
    Builtin {
        name: "abs",
        group: G::Math,
        signatures: &[
            sig(&[("x", INT)], Type::Int, U(Unary::IntAbs)),
            sig(X, Type::Float, U(Unary::Abs)),
        ],
        summary: "Absolute value; an int stays an int.",
        details: "The int minimum wraps to itself, like negation.",
        example: "let distance = abs(pixel.index - center);",
    },
    Builtin {
        name: "sign",
        group: G::Math,
        signatures: &[
            sig(&[("x", INT)], Type::Int, E(int_sign)),
            sig(X, Type::Float, E(float_sign)),
        ],
        summary: "-1, 0 or 1 by the sign of `x`; an int stays an int.",
        details: "A float zero keeps its sign and NaN stays NaN.",
        example: "let direction = sign(velocity);",
    },
    Builtin {
        name: "min",
        group: G::Math,
        signatures: &[
            sig(&[("a", INT), ("b", INT)], Type::Int, B(Binary::IntMin)),
            sig(&[("a", FLOAT), ("b", FLOAT)], Type::Float, B(Binary::Min)),
        ],
        summary: "The smaller argument. Two ints give an int; otherwise both are floats.",
        details: "NaN in either argument gives NaN.",
        example: "let last = min(radius, target.count - 1 - pixel.index);",
    },
    Builtin {
        name: "max",
        group: G::Math,
        signatures: &[
            sig(&[("a", INT), ("b", INT)], Type::Int, B(Binary::IntMax)),
            sig(&[("a", FLOAT), ("b", FLOAT)], Type::Float, B(Binary::Max)),
            sig(
                &[("a", COLOR), ("b", COLOR)],
                Type::Color,
                B(Binary::ColorMax),
            ),
        ],
        summary: "The larger argument. Two ints give an int; two colors take the larger of each channel.",
        details: "NaN in either float argument gives NaN.",
        example: "let first = max(-radius, -pixel.index);",
    },
    Builtin {
        name: "clamp",
        group: G::Math,
        signatures: &[
            sig(
                &[("x", INT), ("low", INT), ("high", INT)],
                Type::Int,
                E(int_clamp),
            ),
            sig(
                &[("x", FLOAT), ("low", FLOAT), ("high", FLOAT)],
                Type::Float,
                T(Ternary::Clamp),
            ),
        ],
        summary: "`x` limited to `low..=high`. Three ints give an int.",
        details: "Float clamping with `low > high`, or NaN in any argument, gives NaN. Int clamping applies `low` first, so `low > high` gives `high`.",
        example: "let level = clamp(age / fade, 0.0, 1.0);",
    },
    Builtin {
        name: "mix",
        group: G::Math,
        signatures: &[
            sig(
                &[("a", FLOAT), ("b", FLOAT), ("t", FLOAT)],
                Type::Float,
                T(Ternary::Mix),
            ),
            sig(
                &[("a", COLOR), ("b", COLOR), ("t", FLOAT)],
                Type::Color,
                T(Ternary::MixColor),
            ),
        ],
        summary: "Linear interpolation from `a` at `t = 0` to `b` at `t = 1`.",
        details: "`t` outside 0..1 extrapolates. A NaN `t` gives NaN.",
        example: "let color = mix(#ff0000, #0000ff, pixel.fraction);",
    },
    Builtin {
        name: "smoothstep",
        group: G::Math,
        signatures: &[sig(
            &[("low", FLOAT), ("high", FLOAT), ("x", FLOAT)],
            Type::Float,
            E(smoothstep),
        )],
        summary: "0 below `low`, 1 above `high`, and a smooth cubic between.",
        details: "`low == high` divides by zero and gives NaN.",
        example: "let edge = smoothstep(0.4, 0.6, pixel.fraction);",
    },
    Builtin {
        name: "step",
        group: G::Math,
        signatures: &[sig(&[("edge", FLOAT), ("x", FLOAT)], Type::Float, E(step))],
        summary: "0 when `x < edge`, otherwise 1.",
        details: "NaN in either argument gives NaN.",
        example: "let on = step(0.5, progress);",
    },
    Builtin {
        name: "integral",
        group: G::Math,
        signatures: &[sig(
            &[("parameter", FLOAT)],
            Type::Float,
            Lowering::Integral,
        )],
        summary: "A float parameter integrated over time, in its units times seconds.",
        details: "Integrates from the clip's start in an effect, or the sequence's start in an operator. A fixed value gives `parameter * time`; automation is integrated exactly, so a changing speed or rate never jumps. The argument must name a float parameter.",
        example: "let phase = integral(speed) * TAU;",
    },
    Builtin {
        name: "floor",
        group: G::Rounding,
        signatures: &[sig(X, Type::Float, U(Unary::Floor))],
        summary: "The largest whole number at most `x`, as a float.",
        details: "Use `x // 1` for the same value as an int when `x` is an int expression, or `int(floor(x))` for a float.",
        example: "let step = floor(time * 4.0);",
    },
    Builtin {
        name: "ceil",
        group: G::Rounding,
        signatures: &[sig(X, Type::Float, U(Unary::Ceil))],
        summary: "The smallest whole number at least `x`, as a float.",
        details: "NaN gives NaN.",
        example: "let bars = ceil(height / 3.0);",
    },
    Builtin {
        name: "round",
        group: G::Rounding,
        signatures: &[sig(X, Type::Float, E(round))],
        summary: "The nearest whole number, halves away from zero, as a float.",
        details: "`round(2.5)` is 3 and `round(-2.5)` is -3. NaN gives NaN.",
        example: "let step = round(time * 40.0);",
    },
    Builtin {
        name: "fract",
        group: G::Rounding,
        signatures: &[sig(X, Type::Float, E(fract))],
        summary: "The fractional part `x - floor(x)`, in `[0, 1)`.",
        details: "Negative numbers wrap: `fract(-0.25)` is 0.75. Infinity and NaN give NaN.",
        example: "let phase = fract(time * 0.5 + pixel.fraction);",
    },
    Builtin {
        name: "int",
        group: G::Rounding,
        signatures: &[sig(X, Type::Int, U(Unary::FloatToInt))],
        summary: "A float converted to an int, truncating toward zero.",
        details: "Saturates at the int range and turns NaN into zero. Write `int(floor(x))` to floor negative values.",
        example: "let index = int(progress * len(colors));",
    },
    Builtin {
        name: "is_nan",
        group: G::MissingValues,
        signatures: &[sig(X, Type::Bool, E(is_nan))],
        summary: "Whether `x` is NaN, the missing value.",
        details: "Comparisons with NaN are always false, so test missing values with this.",
        example: "guard !is_nan(last_beat);",
    },
    Builtin {
        name: "value_or",
        group: G::MissingValues,
        signatures: &[sig(
            &[("x", FLOAT), ("replacement", FLOAT)],
            Type::Float,
            B(Binary::ValueOr),
        )],
        summary: "`x`, or `replacement` when `x` is NaN.",
        details: "Both arguments are always computed; use an `if` when the replacement is expensive.",
        example: "let age = value_or(time - mark_last(beats, time), 1000.0);",
    },
    Builtin {
        name: "rgb",
        group: G::Color,
        signatures: &[sig(
            &[("red", FLOAT), ("green", FLOAT), ("blue", FLOAT)],
            Type::Color,
            T(Ternary::Rgb),
        )],
        summary: "A color from channels in 0..1.",
        details: "Channels may lie outside 0..1 until the color is written, which clamps them; a NaN channel is written as zero.",
        example: "let warm = rgb(1.0, 0.6, 0.2);",
    },
    Builtin {
        name: "hsv",
        group: G::Color,
        signatures: &[sig(
            &[("hue", FLOAT), ("saturation", FLOAT), ("value", FLOAT)],
            Type::Color,
            T(Ternary::Hsv),
        )],
        summary: "A color from hue in turns, saturation and value in 0..1.",
        details: "Hue wraps in both directions, so 1.25 is 0.25.",
        example: "let rainbow = hsv(pixel.fraction + time * 0.1, 1.0, 1.0);",
    },
    Builtin {
        name: "hue",
        group: G::Color,
        signatures: &[sig(&[("color", COLOR)], Type::Float, U(Unary::Hue))],
        summary: "A color's hue in turns, in `[0, 1)`.",
        details: "Grays have hue zero.",
        example: "let shifted = hsv(hue(source) + 0.5, saturation(source), intensity(source));",
    },
    Builtin {
        name: "saturation",
        group: G::Color,
        signatures: &[sig(&[("color", COLOR)], Type::Float, U(Unary::Saturation))],
        summary: "A color's HSV saturation in 0..1.",
        details: "Grays have saturation zero.",
        example: "let pastel = saturation(source) < 0.5;",
    },
    Builtin {
        name: "intensity",
        group: G::Color,
        signatures: &[sig(&[("color", COLOR)], Type::Float, U(Unary::Intensity))],
        summary: "A color's HSV value: its brightest channel as a fraction of full brightness.",
        details: "Black is zero and any fully bright channel is one.",
        example: "let masked = source * intensity(mask);",
    },
    Builtin {
        name: "red",
        group: G::Color,
        signatures: &[sig(&[("color", COLOR)], Type::Float, U(Unary::Red))],
        summary: "A color's red channel in 0..1, the scale `rgb` takes.",
        details: "`rgb(red(c), green(c), blue(c))` is `c` again.",
        example: "let warmth = red(source) - blue(source);",
    },
    Builtin {
        name: "green",
        group: G::Color,
        signatures: &[sig(&[("color", COLOR)], Type::Float, U(Unary::Green))],
        summary: "A color's green channel in 0..1.",
        details: "",
        example: "let swapped = rgb(green(source), red(source), blue(source));",
    },
    Builtin {
        name: "blue",
        group: G::Color,
        signatures: &[sig(&[("color", COLOR)], Type::Float, U(Unary::Blue))],
        summary: "A color's blue channel in 0..1.",
        details: "",
        example: "let gray = (red(source) + green(source) + blue(source)) / 3.0;",
    },
    Builtin {
        name: "invert",
        group: G::Color,
        signatures: &[sig(&[("color", COLOR)], Type::Color, U(Unary::Invert))],
        summary: "Each channel complemented: full brightness minus the channel.",
        details: "Inverting black gives white.",
        example: "invert(input)",
    },
    Builtin {
        name: "curve_clamped",
        group: G::Resources,
        signatures: &[sig(
            &[
                ("curve", CURVE),
                ("position", FLOAT),
                ("low", FLOAT),
                ("high", FLOAT),
            ],
            Type::Float,
            E(curve_clamped),
        )],
        summary: "`clamp(curve[position], low, high)` as one operation.",
        details: "Indexing a curve, `curve[position]`, samples it at a position in 0..1; an empty curve gives NaN.",
        example: "let level = curve_clamped(envelope, progress, 0.0, 1.0);",
    },
    Builtin {
        name: "curve_integral",
        group: G::Resources,
        signatures: &[sig(
            &[("curve", CURVE), ("position", FLOAT)],
            Type::Float,
            B(Binary::CurveIntegral),
        )],
        summary: "The area under the curve from position 0 to `position`.",
        details: "End values are held beyond the authored points, so the area keeps growing past them; a negative position gives a negative area. Multiply by `duration` to integrate a rate over the clip's seconds. An empty curve or a NaN position gives NaN.",
        example: "let spawned = curve_integral(rate, progress) * duration;",
    },
    Builtin {
        name: "gradient_color_scaled",
        group: G::Resources,
        signatures: &[sig(
            &[
                ("gradient", GRADIENT),
                ("position", FLOAT),
                ("scale", FLOAT),
            ],
            Type::Color,
            E(gradient_color_scaled),
        )],
        summary: "`gradient[position] * clamp(scale, 0, 1)` as one operation.",
        details: "Indexing a gradient, `gradient[position]`, samples its color at a position in 0..1; an empty gradient gives black.",
        example: "gradient_color_scaled(colors, pixel.fraction, level)",
    },
    Builtin {
        name: "mark_last",
        group: G::Events,
        signatures: &[sig(
            &[("marks", MARKS), ("time", FLOAT)],
            Type::Float,
            B(Binary::MarkLast),
        )],
        summary: "The time in seconds of the last mark at or before `time`.",
        details: "NaN before the first mark.",
        example: "let age = time - mark_last(beats, time);",
    },
    Builtin {
        name: "mark_last_index",
        group: G::Events,
        signatures: &[sig(
            &[("marks", MARKS), ("time", FLOAT)],
            Type::Int,
            B(Binary::MarkLastIndex),
        )],
        summary: "The zero-based index of the last mark at or before `time`.",
        details: "-1 before the first mark.",
        example: "let beat = mark_last_index(beats, time);",
    },
    Builtin {
        name: "mark_at",
        group: G::Events,
        signatures: &[sig(
            &[("marks", MARKS), ("index", INT)],
            Type::Float,
            B(Binary::MarkAt),
        )],
        summary: "The time in seconds of the mark at `index`.",
        details: "NaN outside `0..mark_count(marks)`.",
        example: "let next = mark_at(beats, beat + 1);",
    },
    Builtin {
        name: "image_color",
        group: G::Images,
        signatures: &[sig(
            &[("image", IMAGE), ("frame", INT), ("x", FLOAT), ("y", FLOAT)],
            Type::Color,
            E(image_color),
        )],
        summary: "Frame `frame` of the image at `x` pixels right of its left edge and `y` up from its bottom.",
        details: "Colors blend between the centers of its pixels, and fade to black past its edges. Frames repeat, so `frame` may count up forever; a still picture has one frame. The image is chosen once a frame, not per pixel. Black while the image is prepared, or when its file cannot be read.",
        example: "image_color(picture, int(time * image_rate(picture)), pixel.x * 8.0, pixel.y * 8.0)",
    },
    Builtin {
        name: "image_width",
        group: G::Images,
        signatures: &[sig(&[("image", IMAGE)], Type::Float, U(Unary::ImageWidth))],
        summary: "How wide the image is in pixels.",
        details: "0 while the image is prepared, or when its file cannot be read.",
        example: "let x = pixel.fraction * image_width(picture);",
    },
    Builtin {
        name: "image_height",
        group: G::Images,
        signatures: &[sig(&[("image", IMAGE)], Type::Float, U(Unary::ImageHeight))],
        summary: "How tall the image is in pixels.",
        details: "0 while the image is prepared, or when its file cannot be read.",
        example: "let y = (pixel.y - target.min_y) / (target.max_y - target.min_y) * image_height(picture);",
    },
    Builtin {
        name: "image_frames",
        group: G::Images,
        signatures: &[sig(&[("image", IMAGE)], Type::Int, U(Unary::ImageFrames))],
        summary: "How many frames the image has: 1 for a still picture.",
        details: "0 while the image is prepared, or when its file cannot be read.",
        example: "let last = image_frames(picture) - 1;",
    },
    Builtin {
        name: "image_rate",
        group: G::Images,
        signatures: &[sig(&[("image", IMAGE)], Type::Float, U(Unary::ImageRate))],
        summary: "How many frames a second the image shows: 0 for a still picture.",
        details: "",
        example: "let shown = int(time * image_rate(picture)) % image_frames(picture);",
    },
    Builtin {
        name: "glyph_coverage",
        group: G::Text,
        signatures: &[sig(
            &[("font", FONT), ("text", TEXT), ("x", FLOAT), ("y", FLOAT)],
            Type::Float,
            E(glyph_coverage),
        )],
        summary: "How much of the point `x`, `y` the text covers, 0..1, in the font's pixels: x from the text's start, y up from the bottom of its line.",
        details: "Characters are smoothed; `step(0.5, coverage)` makes them crisp. Points outside the text are 0. Sample several points for outlines and glow.",
        example: "let ink = glyph_coverage(font, message, pixel.x * 10.0 - time * 20.0, pixel.y * 10.0);",
    },
    Builtin {
        name: "text_width",
        group: G::Text,
        signatures: &[sig(
            &[("font", FONT), ("text", TEXT)],
            Type::Float,
            E(text_width),
        )],
        summary: "How wide the text is in the font, in its pixels.",
        details: "",
        example: "let start = -text_width(font, message) * progress;",
    },
    Builtin {
        name: "char_index",
        group: G::Text,
        signatures: &[sig(
            &[("font", FONT), ("text", TEXT), ("x", FLOAT)],
            Type::Int,
            E(char_index),
        )],
        summary: "Which character of the text is at `x` in the font's pixels, counting from 0.",
        details: "-1 before the text, and its length or more after it.",
        example: "let color = colors[char_index(font, message, x) % len(colors)];",
    },
    Builtin {
        name: "font_height",
        group: G::Text,
        signatures: &[sig(&[("font", FONT)], Type::Float, U(Unary::FontHeight))],
        summary: "The height of the font's line in pixels.",
        details: "0 while the font is prepared, or when its file cannot be read.",
        example: "let y = pixel.fraction * font_height(font);",
    },
    Builtin {
        name: "text_length",
        group: G::Text,
        signatures: &[sig(&[("text", TEXT)], Type::Int, E(text_length))],
        summary: "How many characters the text has.",
        details: "",
        example: "guard text_length(message) > 0;",
    },
    Builtin {
        name: "mark_text",
        group: G::Text,
        signatures: &[sig(
            &[("marks", MARKS), ("index", INT)],
            Type::Text,
            E(mark_text),
        )],
        summary: "The label of the mark at `index`, as text: a lyric, word or phrase.",
        details: "Empty for a mark without a label or an index outside the collection. The index is fixed for the frame, such as `mark_last_index(words, time)`.",
        example: "let word = mark_text(words, mark_last_index(words, time));",
    },
    Builtin {
        name: "number_text",
        group: G::Text,
        signatures: &[sig(
            &[("value", FLOAT), ("digits", INT)],
            Type::Text,
            E(number_text),
        )],
        summary: "The whole part of `value` as decimal text, with leading zeros to at least `digits` digits.",
        details: "A negative value starts with `-`. Values beyond about two billion read as their nearest int.",
        example: "let clock = concat(number_text(minutes, 1), concat(\":\", number_text(seconds, 2)));",
    },
    Builtin {
        name: "concat",
        group: G::Text,
        signatures: &[sig(&[("a", TEXT), ("b", TEXT)], Type::Text, E(concat))],
        summary: "`a` followed by `b`, up to 64 characters.",
        details: "",
        example: "concat(\"Hello \", name)",
    },
    Builtin {
        name: "audio_level",
        group: G::Audio,
        signatures: &[sig(
            &[
                ("time", FLOAT),
                ("low", FLOAT),
                ("high", FLOAT),
                ("attack", FLOAT),
                ("decay", FLOAT),
            ],
            Type::Float,
            Lowering::Audio(super::audio::AudioKind::Level),
        )],
        summary: "How loud the sequence's audio is between `low` and `high` hertz at `time` seconds, 0..1 of its loudest.",
        details: "The level rises toward louder sound over `attack` seconds and falls over `decay`. A low of 0 or less keeps the lowest frequencies, and a high at or above half the audio's sample rate the highest. The settings are numbers or parameters, fixed per clip, and a parameter that sets one cannot be automated. Time 0 is the clip's start; outside the audio, and while it is analyzed, the level is 0.",
        example: "let bass = audio_level(time, 0.0, 200.0, 0.0, 0.2);",
    },
    Builtin {
        name: "audio_band",
        group: G::Audio,
        signatures: &[sig(
            &[
                ("band", INT),
                ("bands", INT),
                ("time", FLOAT),
                ("attack", FLOAT),
                ("decay", FLOAT),
            ],
            Type::Float,
            Lowering::Audio(super::audio::AudioKind::Band),
        )],
        summary: "How loud band `band` of `bands` (up to 128) is at `time` seconds, 0..1.",
        details: "The bands are spaced evenly in pitch from 30 Hz to 16 kHz, band 0 the lowest; a band outside `0..bands` is 0. Each is measured in decibels: 0 is 60 dB below the loudest band of the audio and 1 is the loudest. `bands`, `attack` and `decay` are numbers or parameters, fixed per clip; `band` may vary by pixel.",
        example: "let height = audio_band(pixel.index % 16, 16, time, 0.0, 0.1);",
    },
    Builtin {
        name: "audio_wave",
        group: G::Audio,
        signatures: &[sig(
            &[("time", FLOAT)],
            Type::Float,
            Lowering::Audio(super::audio::AudioKind::Wave),
        )],
        summary: "The sequence's audio signal at `time` seconds, -1..1 of its loudest.",
        details: "Sampled 2000 times a second, so it follows the sound's shape up to 1 kHz.",
        example: "let sample = audio_wave(time - pixel.fraction * 0.05);",
    },
    Builtin {
        name: "mark_duration",
        group: G::Events,
        signatures: &[sig(
            &[("marks", MARKS), ("index", INT)],
            Type::Float,
            B(Binary::MarkDuration),
        )],
        summary: "How long the mark at `index` lasts, in seconds.",
        details: "Zero for an instantaneous mark and for every live mark; NaN outside `0..mark_count(marks)`.",
        example: "let span = mark_duration(words, mark_last_index(words, time));",
    },
    Builtin {
        name: "mark_count",
        group: G::Events,
        signatures: &[sig(&[("marks", MARKS)], Type::Int, U(Unary::MarkCount))],
        summary: "How many marks the collection holds; the same as `len(marks)`.",
        details: "",
        example: "guard mark_count(beats) > 0;",
    },
    Builtin {
        name: "curve_first_crossing",
        group: G::Events,
        signatures: &[sig(
            &[("curve", CURVE), ("value", FLOAT)],
            Type::Float,
            B(Binary::CurveFirstCrossing),
        )],
        summary: "The first position in 0..1 where the curve reaches `value`.",
        details: "NaN when it never does, or for an empty curve.",
        example: "let arrival = curve_first_crossing(chase, pixel.fraction);",
    },
    Builtin {
        name: "curve_last_crossing",
        group: G::Events,
        signatures: &[sig(
            &[("curve", CURVE), ("value", FLOAT), ("before", FLOAT)],
            Type::Float,
            T(Ternary::CurveLastCrossing),
        )],
        summary: "The last position at or before `before` where the curve reaches `value`.",
        details: "NaN when there is none.",
        example: "let left = curve_last_crossing(chase, pixel.fraction, progress);",
    },
    Builtin {
        name: "section_count",
        group: G::Sections,
        signatures: &[sig(&[("width", INT)], Type::Int, U(Unary::SectionCount))],
        summary: "How many runs of `width` pixels the target is split into.",
        details: "A per-fixture effect sections each fixture; a whole-target effect sections the target's fixtures in order. Widths below one count as one.",
        example: "let sections = section_count(10);",
    },
    Builtin {
        name: "section_index",
        group: G::Sections,
        signatures: &[sig(&[("width", INT)], Type::Int, U(Unary::SectionIndex))],
        summary: "The zero-based run of `width` pixels the current pixel is in.",
        details: "",
        example: "let lit = section_index(10) % 2 == 0;",
    },
    Builtin {
        name: "section_position",
        group: G::Sections,
        signatures: &[sig(&[("width", FLOAT)], Type::Float, E(section_position))],
        summary: "The current pixel's position within its run of `width` pixels, in `[0, 1)`.",
        details: "Widths below one count as one.",
        example: "let ramp = section_position(10.0);",
    },
    Builtin {
        name: "rand",
        group: G::Random,
        signatures: &[sig(&[("seed", FLOAT)], Type::Float, U(Unary::Rand))],
        summary: "A pseudo-random number in `[0, 1)` hashed from `seed`.",
        details: "Pure: the same seed always gives the same value, on every pixel, frame, controller and seek. Combine what identifies a decision (pixel, time bucket, mark) with a user seed parameter. Integer seeds above 2^24 lose precision. NaN gives NaN.",
        example: "let twinkle = rand((seed * 31.0 + pixel.index) * 31.0 + floor(time * 8.0));",
    },
    Builtin {
        name: "len",
        group: G::Collections,
        signatures: &[
            sig(&[("array", ANY_ARRAY)], Type::Int, U(Unary::Len)),
            sig(&[("marks", MARKS)], Type::Int, U(Unary::MarkCount)),
        ],
        summary: "How many items an array or marks collection holds.",
        details: "Indexing an array, `array[i]`, clamps `i` to the first or last item and floors a float `i`; an empty array gives the item type's default.",
        example: "let color = colors[pixel.index % len(colors)];",
    },
];

// Builtins that desugar to several operations.

/// `base` multiplied into one `exponent` times when that is a small count,
/// otherwise the general power.
fn power(checker: &mut Checker, args: &[Node]) -> Node {
    let (base, exponent) = (args[0], args[1]);
    // Repeated multiplication is exact; it needs a small, non-negative count.
    if checker.counts(exponent) {
        return checker.graph.binary(Binary::Power, base, exponent);
    }
    let exponent = checker.graph.unary(Unary::IntToFloat, exponent);
    checker.graph.binary(Binary::PowerFloat, base, exponent)
}

fn int_sign(checker: &mut Checker, args: &[Node]) -> Node {
    let graph = &mut checker.graph;
    let value = args[0];
    let zero = graph.int(0);
    let positive = graph.binary(Binary::IntGreater, value, zero);
    let negative = graph.binary(Binary::IntLess, value, zero);
    let (one, minus_one) = (graph.int(1), graph.int(-1));
    let low = graph.select(negative, minus_one, zero);
    graph.select(positive, one, low)
}

fn float_sign(checker: &mut Checker, args: &[Node]) -> Node {
    checker.float_sign(args[0])
}

fn int_clamp(checker: &mut Checker, args: &[Node]) -> Node {
    let raised = checker.graph.binary(Binary::IntMax, args[0], args[1]);
    checker.graph.binary(Binary::IntMin, raised, args[2])
}

fn smoothstep(checker: &mut Checker, args: &[Node]) -> Node {
    let graph = &mut checker.graph;
    let (low, high, value) = (args[0], args[1], args[2]);
    let width = graph.binary(Binary::Subtract, high, low);
    let offset = graph.binary(Binary::Subtract, value, low);
    let position = graph.binary(Binary::Divide, offset, width);
    graph.unary(Unary::Smoothstep, position)
}

fn step(checker: &mut Checker, args: &[Node]) -> Node {
    let graph = &mut checker.graph;
    let (edge, value) = (args[0], args[1]);
    let above = graph.binary(Binary::GreaterEqual, value, edge);
    let below = graph.binary(Binary::Less, value, edge);
    let (one, zero, nan) = (graph.float(1.0), graph.float(0.0), graph.float(f32::NAN));
    let low = graph.select(below, zero, nan);
    graph.select(above, one, low)
}

/// Halves round away from zero; adding 0.5 would round 0.49999997 up.
fn round(checker: &mut Checker, args: &[Node]) -> Node {
    let value = args[0];
    let whole = checker.graph.unary(Unary::Trunc, value);
    let fraction = checker.graph.binary(Binary::Subtract, value, whole);
    let fraction = checker.graph.unary(Unary::Abs, fraction);
    let half = checker.graph.float(0.5);
    let away = checker.graph.binary(Binary::GreaterEqual, fraction, half);
    let sign = checker.float_sign(value);
    let next = checker.graph.binary(Binary::Add, whole, sign);
    checker.graph.select(away, next, whole)
}

fn fract(checker: &mut Checker, args: &[Node]) -> Node {
    let floor = checker.graph.unary(Unary::Floor, args[0]);
    checker.graph.binary(Binary::Subtract, args[0], floor)
}

fn is_nan(checker: &mut Checker, args: &[Node]) -> Node {
    checker.graph.binary(Binary::NotEqual, args[0], args[0])
}

fn curve_clamped(checker: &mut Checker, args: &[Node]) -> Node {
    let value = checker.graph.binary(Binary::CurveSample, args[0], args[1]);
    checker
        .graph
        .ternary(Ternary::Clamp, value, args[2], args[3])
}

/// The running total of each character's advance: entry `k` is where
/// character `k` starts, and the last entry the text's width.
fn starts(checker: &mut Checker, font: Node, text: Node) -> Node {
    let characters = checker.characters(text);
    checker.tabulated(Tabulation::RunningSum, TEXT_LENGTH, |checker, index| {
        let code = checker.graph.binary(Binary::Index, characters, index);
        checker.graph.binary(Binary::GlyphAdvance, font, code)
    })
}

/// The character at `x`: -1 before the text.
fn placed(checker: &mut Checker, starts: Node, x: Node) -> Node {
    let entries = checker.graph.int(TEXT_LENGTH + 1);
    let placed = checker
        .graph
        .ternary(Ternary::Partition, starts, entries, x);
    let one = checker.graph.int(1);
    checker.graph.binary(Binary::IntSubtract, placed, one)
}

fn glyph_coverage(checker: &mut Checker, args: &[Node]) -> Node {
    let (font, text, x, y) = (args[0], args[1], args[2], args[3]);
    let characters = checker.characters(text);
    let starts = starts(checker, font, characters);
    let index = placed(checker, starts, x);
    let graph = &mut checker.graph;
    let zero = graph.int(0);
    let before = graph.binary(Binary::IntLess, index, zero);
    let code = graph.binary(Binary::Index, characters, index);
    let code = graph.select(before, zero, code);
    let pen = graph.binary(Binary::Index, starts, index);
    let local = graph.binary(Binary::Subtract, x, pen);
    graph.quaternary(Quaternary::GlyphCoverage, font, code, local, y)
}

fn image_color(checker: &mut Checker, args: &[Node]) -> Node {
    let (image, frame, x, y) = (args[0], args[1], args[2], args[3]);
    checker.require_per_frame(image, "an image is chosen once a frame, not per pixel");
    checker
        .graph
        .quaternary(Quaternary::ImageColor, image, frame, x, y)
}

fn text_width(checker: &mut Checker, args: &[Node]) -> Node {
    let starts = starts(checker, args[0], args[1]);
    let last = checker.graph.int(TEXT_LENGTH);
    checker.graph.binary(Binary::Index, starts, last)
}

fn char_index(checker: &mut Checker, args: &[Node]) -> Node {
    let starts = starts(checker, args[0], args[1]);
    placed(checker, starts, args[2])
}

fn text_length(checker: &mut Checker, args: &[Node]) -> Node {
    let characters = checker.characters(args[0]);
    let counts = checker.tabulated(Tabulation::RunningSum, TEXT_LENGTH, |checker, index| {
        let graph = &mut checker.graph;
        let code = graph.binary(Binary::Index, characters, index);
        let zero = graph.int(0);
        let present = graph.binary(Binary::NotEqual, code, zero);
        let one = graph.int(1);
        graph.select(present, one, zero)
    });
    let last = checker.graph.int(TEXT_LENGTH);
    checker.graph.binary(Binary::Index, counts, last)
}

fn mark_text(checker: &mut Checker, args: &[Node]) -> Node {
    let (marks, mark) = (args[0], args[1]);
    checker.tabulated(Tabulation::Table, TEXT_LENGTH, |checker, index| {
        checker.graph.ternary(Ternary::MarkText, marks, mark, index)
    })
}

/// Decimal digits of an int: powers of ten up to the largest an int holds.
const POWERS: [i32; 10] = [
    1,
    10,
    100,
    1_000,
    10_000,
    100_000,
    1_000_000,
    10_000_000,
    100_000_000,
    1_000_000_000,
];

fn number_text(checker: &mut Checker, args: &[Node]) -> Node {
    let (value, digits) = (args[0], args[1]);
    let graph = &mut checker.graph;
    let whole = graph.unary(Unary::FloatToInt, value);
    let magnitude = graph.unary(Unary::IntAbs, whole);
    let zero = graph.int(0);
    let negative = graph.binary(Binary::IntLess, whole, zero);
    let powers: Vec<Node> = POWERS.iter().map(|&power| graph.int(power)).collect();
    let powers = graph.add(Op::Items(powers.into()));
    // Digits past the first: the powers of ten the magnitude reaches.
    let reached = checker.tabulated(
        Tabulation::RunningSum,
        POWERS.len() as i32 - 1,
        |checker, index| {
            let graph = &mut checker.graph;
            let one = graph.int(1);
            let next = graph.binary(Binary::IntAdd, index, one);
            let power = graph.binary(Binary::Index, powers, next);
            let reaches = graph.binary(Binary::IntGreaterEqual, magnitude, power);
            let zero = graph.int(0);
            graph.select(reaches, one, zero)
        },
    );
    let graph = &mut checker.graph;
    let last = graph.int(POWERS.len() as i32 - 1);
    let extra = graph.binary(Binary::Index, reached, last);
    let one = graph.int(1);
    let own = graph.binary(Binary::IntAdd, extra, one);
    let most = graph.int(POWERS.len() as i32);
    let digits = graph.binary(Binary::IntMin, digits, most);
    let count = graph.binary(Binary::IntMax, own, digits);
    let sign = graph.select(negative, one, zero);
    let length = graph.binary(Binary::IntAdd, count, sign);
    checker.tabulated(Tabulation::Table, TEXT_LENGTH, |checker, index| {
        let graph = &mut checker.graph;
        let zero = graph.int(0);
        let one = graph.int(1);
        let past = graph.binary(Binary::IntGreaterEqual, index, length);
        let at_sign = graph.binary(Binary::IntLess, index, sign);
        let place = graph.binary(Binary::IntSubtract, index, sign);
        let from_end = graph.binary(Binary::IntSubtract, count, place);
        let power_index = graph.binary(Binary::IntSubtract, from_end, one);
        let power = graph.binary(Binary::Index, powers, power_index);
        let shifted = graph.binary(Binary::IntFloorDivide, magnitude, power);
        let ten = graph.int(10);
        let digit = graph.binary(Binary::IntRemainder, shifted, ten);
        let zero_code = graph.int(i32::from(b'0'));
        let digit_code = graph.binary(Binary::IntAdd, digit, zero_code);
        let minus = graph.int(i32::from(b'-'));
        let character = graph.select(at_sign, minus, digit_code);
        graph.select(past, zero, character)
    })
}

fn concat(checker: &mut Checker, args: &[Node]) -> Node {
    let first = checker.characters(args[0]);
    let second = checker.characters(args[1]);
    let length = text_length(checker, &[first]);
    checker.tabulated(Tabulation::Table, TEXT_LENGTH, |checker, index| {
        let graph = &mut checker.graph;
        let within = graph.binary(Binary::IntLess, index, length);
        let own = graph.binary(Binary::Index, first, index);
        let after = graph.binary(Binary::IntSubtract, index, length);
        let other = graph.binary(Binary::Index, second, after);
        graph.select(within, own, other)
    })
}

fn gradient_color_scaled(checker: &mut Checker, args: &[Node]) -> Node {
    let graph = &mut checker.graph;
    let (zero, one) = (graph.float(0.0), graph.float(1.0));
    let scale = graph.ternary(Ternary::Clamp, args[2], zero, one);
    let color = graph.binary(Binary::GradientSample, args[0], args[1]);
    graph.binary(Binary::ColorScale, color, scale)
}

/// Widths below one count as one.
fn section_position(checker: &mut Checker, args: &[Node]) -> Node {
    let graph = &mut checker.graph;
    let one = graph.float(1.0);
    let width = graph.binary(Binary::Max, args[0], one);
    let inverse = graph.binary(Binary::Divide, one, width);
    graph.binary(Binary::SectionPosition, width, inverse)
}

pub(crate) fn builtin(name: &str) -> Option<&'static Builtin> {
    BUILTINS.iter().find(|builtin| builtin.name == name)
}

/// What a context value reads.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Read {
    Context(Context),
    Constant(f32),
}

/// A context value: a reserved name or a field of `pixel` or `target`.
#[derive(Debug)]
pub struct ContextValue {
    /// `time`, or `pixel.index` for a field.
    pub(crate) name: &'static str,
    pub(crate) ty: Type,
    pub(crate) summary: &'static str,
    pub(crate) read: Read,
}

impl ContextValue {
    /// The scope and field of a field like `pixel.index`.
    pub(crate) fn field(&self) -> Option<(&'static str, &'static str)> {
        self.name.split_once('.')
    }
}

/// The context value `name` names, when it is no field.
pub(crate) fn context_value(name: &str) -> Option<&'static ContextValue> {
    CONTEXT.iter().find(|value| value.name == name)
}

/// The context field `scope.field`.
pub(crate) fn context_field(scope: &str, field: &str) -> Option<&'static ContextValue> {
    CONTEXT
        .iter()
        .find(|value| value.field() == Some((scope, field)))
}

/// Whether `name` is the scope of context fields, like `pixel`.
pub(crate) fn is_context_scope(name: &str) -> bool {
    CONTEXT
        .iter()
        .any(|value| value.field().is_some_and(|(scope, _)| scope == name))
}

/// Whether `name` is a context value or the scope of context fields, like
/// `time` or `pixel`.
pub(crate) fn is_context_name(name: &str) -> bool {
    context_value(name).is_some() || is_context_scope(name)
}

/// The type of an IR context read.
pub(crate) fn context_type(context: Context) -> Type {
    CONTEXT
        .iter()
        .find(|value| value.read == Read::Context(context))
        .map_or_else(
            || unreachable!("every context has a table row"),
            |value| value.ty.clone(),
        )
}

pub const CONTEXT: &[ContextValue] = &[
    ContextValue {
        name: "time",
        ty: Type::Float,
        summary: "Seconds since the effect's clip started; in an operator, since the sequence started.",
        read: Read::Context(Context::Time),
    },
    ContextValue {
        name: "duration",
        ty: Type::Float,
        summary: "The clip's duration in seconds; in an operator, the sequence's.",
        read: Read::Context(Context::Duration),
    },
    ContextValue {
        name: "progress",
        ty: Type::Float,
        summary: "`time / duration`, in 0..1.",
        read: Read::Context(Context::Progress),
    },
    ContextValue {
        name: "frame",
        ty: Type::Int,
        summary: "Frames since the effect's clip started, the nearest frame to `time`; in an operator, since the sequence started.",
        read: Read::Context(Context::Frame),
    },
    ContextValue {
        name: "frame_rate",
        ty: Type::Float,
        summary: "The sequence's frames per second.",
        read: Read::Context(Context::FrameRate),
    },
    ContextValue {
        name: "pixel.index",
        ty: Type::Int,
        summary: "The pixel's zero-based index within the target; in an operator, within its fixture instance.",
        read: Read::Context(Context::PixelIndex),
    },
    ContextValue {
        name: "pixel.fraction",
        ty: Type::Float,
        summary: "The pixel's index scaled to 0..1 across the target; in an operator, across its fixture instance.",
        read: Read::Context(Context::PixelFraction),
    },
    ContextValue {
        name: "pixel.x",
        ty: Type::Float,
        summary: "The pixel's layout x coordinate in meters, after fixture transforms.",
        read: Read::Context(Context::PixelX),
    },
    ContextValue {
        name: "pixel.y",
        ty: Type::Float,
        summary: "The pixel's layout y coordinate in meters.",
        read: Read::Context(Context::PixelY),
    },
    ContextValue {
        name: "pixel.z",
        ty: Type::Float,
        summary: "The pixel's layout z coordinate in meters, toward the viewer.",
        read: Read::Context(Context::PixelZ),
    },
    ContextValue {
        name: "pixel.member",
        ty: Type::Int,
        summary: "Which member of the clip's target the pixel belongs to, a fixture or slice, counting from 0 in member order.",
        read: Read::Context(Context::PixelMember),
    },
    ContextValue {
        name: "pixel.strand",
        ty: Type::Int,
        summary: "The strand the pixel lies on, counting from 0: each element of a fixture is one, and each row or column of a grid as it is wired. A whole target numbers its members' strands in member order; per fixture, each fixture's own.",
        read: Read::Context(Context::PixelStrand),
    },
    ContextValue {
        name: "pixel.along",
        ty: Type::Int,
        summary: "The pixel's place along its strand in wiring order, counting from 0.",
        read: Read::Context(Context::PixelAlong),
    },
    ContextValue {
        name: "target.count",
        ty: Type::Int,
        summary: "How many pixels the target has; in an operator, its fixture instance.",
        read: Read::Context(Context::TargetCount),
    },
    ContextValue {
        name: "target.min_x",
        ty: Type::Float,
        summary: "The smallest x coordinate of the target's pixels.",
        read: Read::Context(Context::TargetMinX),
    },
    ContextValue {
        name: "target.min_y",
        ty: Type::Float,
        summary: "The smallest y coordinate of the target's pixels.",
        read: Read::Context(Context::TargetMinY),
    },
    ContextValue {
        name: "target.max_x",
        ty: Type::Float,
        summary: "The largest x coordinate of the target's pixels.",
        read: Read::Context(Context::TargetMaxX),
    },
    ContextValue {
        name: "target.max_y",
        ty: Type::Float,
        summary: "The largest y coordinate of the target's pixels.",
        read: Read::Context(Context::TargetMaxY),
    },
    ContextValue {
        name: "target.min_z",
        ty: Type::Float,
        summary: "The smallest z coordinate of the target's pixels.",
        read: Read::Context(Context::TargetMinZ),
    },
    ContextValue {
        name: "target.max_z",
        ty: Type::Float,
        summary: "The largest z coordinate of the target's pixels.",
        read: Read::Context(Context::TargetMaxZ),
    },
    ContextValue {
        name: "target.members",
        ty: Type::Int,
        summary: "How many members, fixtures or slices, the clip's target has, per fixture as for the whole target.",
        read: Read::Context(Context::TargetMembers),
    },
    ContextValue {
        name: "target.strands",
        ty: Type::Int,
        summary: "How many strands the target has: per fixture, the fixture's.",
        read: Read::Context(Context::TargetStrands),
    },
    ContextValue {
        name: "target.strand_length",
        ty: Type::Int,
        summary: "How many pixels the target's longest strand has.",
        read: Read::Context(Context::TargetStrandLength),
    },
    ContextValue {
        name: "PI",
        ty: Type::Float,
        summary: "π, half a turn in radians.",
        read: Read::Constant(core::f32::consts::PI),
    },
    ContextValue {
        name: "TAU",
        ty: Type::Float,
        summary: "2π, a full turn in radians.",
        read: Read::Constant(core::f32::consts::TAU),
    },
];

/// A method of operator inputs.
#[derive(Debug)]
pub struct Method {
    pub(crate) name: &'static str,
    args: &'static [(&'static str, ArgType)],
    result: Type,
    pub(crate) summary: &'static str,
}

impl Method {
    /// `name(a: int) -> color`.
    pub(crate) fn label(&self) -> String {
        label(self.name, self.args, &self.result)
    }
}

/// `at`, the only method.
pub const AT: Method = Method {
    name: "at",
    args: &[("pixel", INT)],
    result: Type::Color,
    summary: "The signal at another pixel of this fixture, at this frame; `pixel` is a zero-based index within the fixture.",
};

/// A reduction over a range of indexes.
#[derive(Debug)]
pub struct Reducer {
    pub(crate) name: &'static str,
    pub(crate) summary: &'static str,
    pub(crate) kind: ReducerKind,
}

pub const REDUCERS: &[Reducer] = &[
    Reducer {
        name: "max",
        summary: "The largest value of the body over the range.",
        kind: ReducerKind::Max,
    },
    Reducer {
        name: "min",
        summary: "The smallest value of the body over the range.",
        kind: ReducerKind::Min,
    },
    Reducer {
        name: "sum",
        summary: "The sum of the body over the range.",
        kind: ReducerKind::Sum,
    },
    Reducer {
        name: "any",
        summary: "Whether the body holds for some index.",
        kind: ReducerKind::Any,
    },
    Reducer {
        name: "all",
        summary: "Whether the body holds for every index.",
        kind: ReducerKind::All,
    },
    Reducer {
        name: "first",
        summary: "The body at the first index whose guards pass, or `else`.",
        kind: ReducerKind::First,
    },
    Reducer {
        name: "last",
        summary: "The body at the last index whose guards pass, or `else`.",
        kind: ReducerKind::Last,
    },
    Reducer {
        name: "table",
        summary: "An array of the body at each index of at most 128, computed once per frame from parameters, time and the target; index it with `[k]` from 0.",
        kind: ReducerKind::Table,
    },
    Reducer {
        name: "table sum",
        summary: "The body's running sums over at most 127 indices: entry `k` sums the first `k` values, so entry 0 is zero and the last is the total.",
        kind: ReducerKind::TableSum,
    },
];

pub(crate) fn reducer(kind: ReducerKind) -> &'static Reducer {
    REDUCERS
        .iter()
        .find(|reducer| reducer.kind == kind)
        .unwrap_or_else(|| unreachable!("every reducer has a table row"))
}

/// The type names scripts write; other types are `enum { ... }` and
/// `array<...>`.
pub const NAMED_TYPES: [(&str, Type); 10] = [
    ("int", Type::Int),
    ("float", Type::Float),
    ("bool", Type::Bool),
    ("color", Type::Color),
    ("curve", Type::Curve),
    ("gradient", Type::Gradient),
    ("marks", Type::Marks),
    ("text", Type::Text),
    ("font", Type::Font),
    ("image", Type::Image),
];
pub const ENUM_WORD: &str = "enum";
pub const ARRAY_WORD: &str = "array";

/// A type as scripts spell it.
pub fn type_text(ty: &Type) -> String {
    match ty {
        Type::Void => "void".into(),
        Type::Array(item) => format!("{ARRAY_WORD}<{}>", type_text(item)),
        Type::Enum(options) => format!(
            "{ENUM_WORD} {{ {} }}",
            options
                .iter()
                .map(Identifier::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        named => NAMED_TYPES
            .iter()
            .find(|(_, candidate)| candidate == named)
            .map_or_else(
                || unreachable!("every other type has a name"),
                |(name, _)| (*name).to_string(),
            ),
    }
}

/// A type as diagnostics name it: `an int`, `an array of float`, `marks`.
pub(crate) fn describe(ty: &Type) -> String {
    match ty {
        Type::Void => "nothing".into(),
        Type::Marks => type_text(ty),
        Type::Array(item) => format!("an {ARRAY_WORD} of {}", type_text(item)),
        Type::Enum(options) => format!(
            "an {ENUM_WORD} of {}",
            options
                .iter()
                .map(Identifier::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Type::Int => format!("an {}", type_text(ty)),
        _ => format!("a {}", type_text(ty)),
    }
}

/// The generated builtin reference, `docs/effect_builtins.md`.
pub fn builtin_reference() -> String {
    let mut text = String::new();
    let _ = writeln!(
        text,
        "# Effect language builtins\n\n\
         <!-- Generated from crates/donder-language/src/compiler/builtins.rs by \
         `pnpm generate:builtins`; do not edit. -->\n\n\
         Every builtin function, context value and signal method of the [effect \
         language](effect_language.md). A call takes a signature its arguments \
         fit exactly, and otherwise one they fit with int arguments widened to \
         floats, so an int is accepted wherever a float is expected and \
         signatures with int forms keep ints as ints when every argument is \
         an int.\n\n## Context values\n\n\
         | Name | Type | Meaning |\n| --- | --- | --- |"
    );
    for value in CONTEXT {
        let _ = writeln!(
            text,
            "| `{}` | {} | {} |",
            value.name,
            type_text(&value.ty),
            value.summary
        );
    }
    let _ = writeln!(
        text,
        "\n## Signal methods\n\nOperator inputs are signals.\n\n```text\n{}\n```\n\n{}",
        AT.label(),
        AT.summary
    );
    for group in BuiltinGroup::ALL {
        let _ = writeln!(text, "\n## {}", group.title());
        for builtin in BUILTINS.iter().filter(|builtin| builtin.group == group) {
            let _ = writeln!(text, "\n### `{}`\n\n```text", builtin.name);
            for signature in builtin.signatures {
                let _ = writeln!(text, "{}", signature.label(builtin.name));
            }
            let _ = writeln!(text, "```\n\n{}", builtin.summary);
            if !builtin.details.is_empty() {
                let _ = writeln!(text, "{}", builtin.details);
            }
            let _ = writeln!(text, "\n```text\n{}\n```", builtin.example);
        }
    }
    text
}
