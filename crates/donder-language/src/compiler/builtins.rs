//! The effect language's builtin names: functions, context values, methods,
//! reducers and type names. Each is listed once here; name resolution, type
//! checking, editor analysis and `docs/effect_builtins.md` all read these
//! tables.
use super::check::Checker;
use super::ir::{Binary, Context, Node, Ternary, Unary};
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
}

impl BuiltinGroup {
    const ALL: [Self; 9] = [
        Self::Math,
        Self::Rounding,
        Self::MissingValues,
        Self::Color,
        Self::Resources,
        Self::Events,
        Self::Sections,
        Self::Random,
        Self::Collections,
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
            _ => None,
        }
    }

    /// The type as scripts spell it.
    pub(crate) fn text(&self) -> String {
        match self {
            Self::Is(ty) => type_text(ty),
            Self::Array => format!("{ARRAY_WORD}<T>"),
        }
    }

    /// The type as diagnostics name it.
    pub(crate) fn describe(&self) -> String {
        match self {
            Self::Is(ty) => describe(ty),
            Self::Array => format!("an {ARRAY_WORD}"),
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
        format!(
            "{name}({}) -> {}",
            self.args
                .iter()
                .map(|(arg, ty)| format!("{arg}: {}", ty.text()))
                .collect::<Vec<_>>()
                .join(", "),
            type_text(&self.result)
        )
    }
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
const X: &[(&str, ArgType)] = &[("x", FLOAT)];

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
        example: "let frame = round(time * 40.0);",
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
    /// This pixel last frame.
    Previous,
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
        name: "frame_rate",
        ty: Type::Float,
        summary: "The sequence's frames per second.",
        read: Read::Context(Context::FrameRate),
    },
    ContextValue {
        name: "frames_elapsed",
        ty: Type::Int,
        summary: "Frames since the frame shown before: one in steady playback, more after skipped frames, one after a seek, and zero when a console command re-renders the same frame.",
        read: Read::Context(Context::FramesElapsed),
    },
    ContextValue {
        name: "previous",
        ty: Type::Color,
        summary: "This pixel last frame: an operator's own output, or an effect's layer. `previous.at(i)` reads another pixel of the fixture and `around previous` its neighbors. Black until the first frame and for a clip previewed alone; kept as it was after a seek.",
        read: Read::Previous,
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

/// A method of operator inputs and `previous`.
#[derive(Debug)]
pub struct Method {
    pub(crate) name: &'static str,
    pub(crate) signature: &'static str,
    pub(crate) summary: &'static str,
}

/// `at`, the only method.
pub const AT: Method = Method {
    name: "at",
    signature: "at(pixel: int) -> color",
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
];

pub(crate) fn reducer(kind: ReducerKind) -> &'static Reducer {
    REDUCERS
        .iter()
        .find(|reducer| reducer.kind == kind)
        .unwrap_or_else(|| unreachable!("every reducer has a table row"))
}

/// The type names scripts write; other types are `enum { ... }` and
/// `array<...>`.
pub const NAMED_TYPES: [(&str, Type); 7] = [
    ("int", Type::Int),
    ("float", Type::Float),
    ("bool", Type::Bool),
    ("color", Type::Color),
    ("curve", Type::Curve),
    ("gradient", Type::Gradient),
    ("marks", Type::Marks),
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
        "\n## Signal methods\n\nOperator inputs and `previous` are signals.\n\n```text\n{}\n```\n\n{}",
        AT.signature, AT.summary
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
