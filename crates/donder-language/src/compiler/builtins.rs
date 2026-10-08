//! The effect language's builtin functions and context values: one table that
//! name resolution reads and `docs/effect_builtins.md` is generated from.
use core::fmt::Write;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BuiltinFunction {
    Sin,
    Cos,
    Tan,
    Atan2,
    Sqrt,
    Exp,
    Log,
    Pow,
    Abs,
    Sign,
    Min,
    Max,
    Clamp,
    Mix,
    Smoothstep,
    Step,
    Integral,
    Floor,
    Ceil,
    Trunc,
    Round,
    RoundEven,
    Fract,
    Int,
    IsNan,
    ValueOr,
    Rgb,
    Hsv,
    Hue,
    Saturation,
    Intensity,
    Red,
    Green,
    Blue,
    Invert,
    CurveClamped,
    CurveIntegral,
    GradientColorScaled,
    MarkLast,
    MarkLastIndex,
    MarkAt,
    MarkCount,
    CurveFirstCrossing,
    CurveLastCrossing,
    SectionCount,
    SectionIndex,
    SectionPosition,
    Rand,
    Len,
}

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

/// One way to call a builtin: argument names with types, and the result type.
#[derive(Debug)]
pub struct Signature {
    pub args: &'static [(&'static str, &'static str)],
    pub result: &'static str,
}

#[derive(Debug)]
pub struct Builtin {
    pub function: BuiltinFunction,
    pub name: &'static str,
    pub group: BuiltinGroup,
    pub signatures: &'static [Signature],
    pub summary: &'static str,
    /// Edge cases: NaN, ranges, units and conversions.
    pub details: &'static str,
    pub example: &'static str,
}

impl Builtin {
    pub fn arity(&self) -> usize {
        self.signatures[0].args.len()
    }
}

const fn sig(args: &'static [(&'static str, &'static str)], result: &'static str) -> Signature {
    Signature { args, result }
}

const X: &[(&str, &str)] = &[("x", "float")];

use BuiltinFunction as F;
use BuiltinGroup as G;

pub const BUILTINS: &[Builtin] = &[
    Builtin {
        function: F::Sin,
        name: "sin",
        group: G::Math,
        signatures: &[sig(X, "float")],
        summary: "Sine of an angle in radians.",
        details: "NaN gives NaN. Never folded at compile time, so host and device agree.",
        example: "let wave = sin(time * TAU);",
    },
    Builtin {
        function: F::Cos,
        name: "cos",
        group: G::Math,
        signatures: &[sig(X, "float")],
        summary: "Cosine of an angle in radians.",
        details: "NaN gives NaN.",
        example: "let wave = cos(pixel.fraction * PI);",
    },
    Builtin {
        function: F::Tan,
        name: "tan",
        group: G::Math,
        signatures: &[sig(X, "float")],
        summary: "Tangent of an angle in radians.",
        details: "Grows without bound near odd multiples of PI / 2. NaN gives NaN.",
        example: "let slope = tan(angle);",
    },
    Builtin {
        function: F::Atan2,
        name: "atan2",
        group: G::Math,
        signatures: &[sig(&[("y", "float"), ("x", "float")], "float")],
        summary: "Angle of the point (x, y) from the positive x axis, in radians from -PI to PI.",
        details: "Note the argument order: y first.",
        example: "let angle = atan2(pixel.y - 1.0, pixel.x - 1.0);",
    },
    Builtin {
        function: F::Sqrt,
        name: "sqrt",
        group: G::Math,
        signatures: &[sig(X, "float")],
        summary: "Square root.",
        details: "Negative numbers and NaN give NaN.",
        example: "let distance = sqrt(dx * dx + dy * dy);",
    },
    Builtin {
        function: F::Exp,
        name: "exp",
        group: G::Math,
        signatures: &[sig(X, "float")],
        summary: "e raised to `x`.",
        details: "Large inputs give infinity. NaN gives NaN.",
        example: "let decay = exp(-age * 4.0);",
    },
    Builtin {
        function: F::Log,
        name: "log",
        group: G::Math,
        signatures: &[sig(X, "float")],
        summary: "Natural logarithm.",
        details: "Zero gives negative infinity; negative numbers and NaN give NaN.",
        example: "let octave = log(frequency / 440.0) / log(2.0);",
    },
    Builtin {
        function: F::Pow,
        name: "pow",
        group: G::Math,
        signatures: &[
            sig(&[("base", "float"), ("exponent", "int")], "float"),
            sig(&[("base", "float"), ("exponent", "float")], "float"),
        ],
        summary: "`base` raised to `exponent`.",
        details: "An int exponent that the compiler can bound to 0..10,000 multiplies `base` into one that many times, exactly. Any other exponent, including negative ints, uses the general power function: `pow(2, -1)` is 0.5. A negative base with a fractional exponent is NaN.",
        example: "let falloff = pow(1.0 - progress, 3);",
    },
    Builtin {
        function: F::Abs,
        name: "abs",
        group: G::Math,
        signatures: &[sig(&[("x", "int")], "int"), sig(X, "float")],
        summary: "Absolute value; an int stays an int.",
        details: "The int minimum wraps to itself, like negation.",
        example: "let distance = abs(pixel.index - center);",
    },
    Builtin {
        function: F::Sign,
        name: "sign",
        group: G::Math,
        signatures: &[sig(&[("x", "int")], "int"), sig(X, "float")],
        summary: "-1, 0 or 1 by the sign of `x`; an int stays an int.",
        details: "A float zero keeps its sign and NaN stays NaN.",
        example: "let direction = sign(velocity);",
    },
    Builtin {
        function: F::Min,
        name: "min",
        group: G::Math,
        signatures: &[
            sig(&[("a", "int"), ("b", "int")], "int"),
            sig(&[("a", "float"), ("b", "float")], "float"),
        ],
        summary: "The smaller argument. Two ints give an int; otherwise both are floats.",
        details: "NaN in either argument gives NaN.",
        example: "let last = min(radius, target.count - 1 - pixel.index);",
    },
    Builtin {
        function: F::Max,
        name: "max",
        group: G::Math,
        signatures: &[
            sig(&[("a", "int"), ("b", "int")], "int"),
            sig(&[("a", "float"), ("b", "float")], "float"),
            sig(&[("a", "color"), ("b", "color")], "color"),
        ],
        summary: "The larger argument. Two ints give an int; two colors take the larger of each channel.",
        details: "NaN in either float argument gives NaN.",
        example: "let first = max(-radius, -pixel.index);",
    },
    Builtin {
        function: F::Clamp,
        name: "clamp",
        group: G::Math,
        signatures: &[
            sig(&[("x", "int"), ("low", "int"), ("high", "int")], "int"),
            sig(
                &[("x", "float"), ("low", "float"), ("high", "float")],
                "float",
            ),
        ],
        summary: "`x` limited to `low..=high`. Three ints give an int.",
        details: "Float clamping with `low > high`, or NaN in any argument, gives NaN. Int clamping applies `low` first, so `low > high` gives `high`.",
        example: "let level = clamp(age / fade, 0.0, 1.0);",
    },
    Builtin {
        function: F::Mix,
        name: "mix",
        group: G::Math,
        signatures: &[
            sig(&[("a", "float"), ("b", "float"), ("t", "float")], "float"),
            sig(&[("a", "color"), ("b", "color"), ("t", "float")], "color"),
        ],
        summary: "Linear interpolation from `a` at `t = 0` to `b` at `t = 1`.",
        details: "`t` outside 0..1 extrapolates; color channels stay within 0..255. A NaN `t` gives NaN, or black for colors.",
        example: "let color = mix(#ff0000, #0000ff, pixel.fraction);",
    },
    Builtin {
        function: F::Smoothstep,
        name: "smoothstep",
        group: G::Math,
        signatures: &[sig(
            &[("low", "float"), ("high", "float"), ("x", "float")],
            "float",
        )],
        summary: "0 below `low`, 1 above `high`, and a smooth cubic between.",
        details: "`low == high` divides by zero and gives NaN.",
        example: "let edge = smoothstep(0.4, 0.6, pixel.fraction);",
    },
    Builtin {
        function: F::Step,
        name: "step",
        group: G::Math,
        signatures: &[sig(&[("edge", "float"), ("x", "float")], "float")],
        summary: "0 when `x < edge`, otherwise 1.",
        details: "NaN in either argument gives NaN.",
        example: "let on = step(0.5, progress);",
    },
    Builtin {
        function: F::Integral,
        name: "integral",
        group: G::Math,
        signatures: &[sig(&[("parameter", "float")], "float")],
        summary: "A float parameter integrated over time, in its units times seconds.",
        details: "Integrates from the clip's start in an effect, or the sequence's start in an operator. A fixed value gives `parameter * time`; automation is integrated exactly, so a changing speed or rate never jumps. The argument must name a float parameter.",
        example: "let phase = integral(speed) * TAU;",
    },
    Builtin {
        function: F::Floor,
        name: "floor",
        group: G::Rounding,
        signatures: &[sig(X, "float")],
        summary: "The largest whole number at most `x`, as a float.",
        details: "Use `x // 1` for the same value as an int when `x` is an int expression, or `int(floor(x))` for a float.",
        example: "let step = floor(time * 4.0);",
    },
    Builtin {
        function: F::Ceil,
        name: "ceil",
        group: G::Rounding,
        signatures: &[sig(X, "float")],
        summary: "The smallest whole number at least `x`, as a float.",
        details: "NaN gives NaN.",
        example: "let bars = ceil(height / 3.0);",
    },
    Builtin {
        function: F::Trunc,
        name: "trunc",
        group: G::Rounding,
        signatures: &[sig(X, "float")],
        summary: "`x` without its fractional part, rounding toward zero, as a float.",
        details: "NaN gives NaN.",
        example: "let whole = trunc(value);",
    },
    Builtin {
        function: F::Round,
        name: "round",
        group: G::Rounding,
        signatures: &[sig(X, "float")],
        summary: "The nearest whole number, halves away from zero, as a float.",
        details: "`round(2.5)` is 3 and `round(-2.5)` is -3. NaN gives NaN.",
        example: "let frame = round(time * 40.0);",
    },
    Builtin {
        function: F::RoundEven,
        name: "round_even",
        group: G::Rounding,
        signatures: &[sig(X, "float")],
        summary: "The nearest whole number, halves to the even neighbor, as a float.",
        details: "`round_even(2.5)` is 2 and `round_even(3.5)` is 4.",
        example: "let frame = round_even(time * 40.0);",
    },
    Builtin {
        function: F::Fract,
        name: "fract",
        group: G::Rounding,
        signatures: &[sig(X, "float")],
        summary: "The fractional part `x - floor(x)`, in `[0, 1)`.",
        details: "Negative numbers wrap: `fract(-0.25)` is 0.75. Infinity and NaN give NaN.",
        example: "let phase = fract(time * 0.5 + pixel.fraction);",
    },
    Builtin {
        function: F::Int,
        name: "int",
        group: G::Rounding,
        signatures: &[sig(X, "int")],
        summary: "A float converted to an int, truncating toward zero.",
        details: "Saturates at the int range and turns NaN into zero. Write `int(floor(x))` to floor negative values.",
        example: "let index = int(progress * len(colors));",
    },
    Builtin {
        function: F::IsNan,
        name: "is_nan",
        group: G::MissingValues,
        signatures: &[sig(X, "bool")],
        summary: "Whether `x` is NaN, the missing value.",
        details: "Comparisons with NaN are always false, so test missing values with this.",
        example: "guard !is_nan(last_beat);",
    },
    Builtin {
        function: F::ValueOr,
        name: "value_or",
        group: G::MissingValues,
        signatures: &[sig(&[("x", "float"), ("replacement", "float")], "float")],
        summary: "`x`, or `replacement` when `x` is NaN.",
        details: "Both arguments are always computed; use an `if` when the replacement is expensive.",
        example: "let age = value_or(time - mark_last(beats, time), 1000.0);",
    },
    Builtin {
        function: F::Rgb,
        name: "rgb",
        group: G::Color,
        signatures: &[sig(
            &[("red", "float"), ("green", "float"), ("blue", "float")],
            "color",
        )],
        summary: "A color from channels in 0..1.",
        details: "Channels clamp to 0..1 and round to 8 bits. NaN in any channel gives black.",
        example: "let warm = rgb(1.0, 0.6, 0.2);",
    },
    Builtin {
        function: F::Hsv,
        name: "hsv",
        group: G::Color,
        signatures: &[sig(
            &[
                ("hue", "float"),
                ("saturation", "float"),
                ("value", "float"),
            ],
            "color",
        )],
        summary: "A color from hue in turns, saturation and value in 0..1.",
        details: "Hue wraps in both directions, so 1.25 is 0.25. NaN in any argument gives black.",
        example: "let rainbow = hsv(pixel.fraction + time * 0.1, 1.0, 1.0);",
    },
    Builtin {
        function: F::Hue,
        name: "hue",
        group: G::Color,
        signatures: &[sig(&[("color", "color")], "float")],
        summary: "A color's hue in turns, in `[0, 1)`.",
        details: "Grays have hue zero.",
        example: "let shifted = hsv(hue(source) + 0.5, saturation(source), intensity(source));",
    },
    Builtin {
        function: F::Saturation,
        name: "saturation",
        group: G::Color,
        signatures: &[sig(&[("color", "color")], "float")],
        summary: "A color's HSV saturation in 0..1.",
        details: "Grays have saturation zero.",
        example: "let pastel = saturation(source) < 0.5;",
    },
    Builtin {
        function: F::Intensity,
        name: "intensity",
        group: G::Color,
        signatures: &[sig(&[("color", "color")], "float")],
        summary: "A color's HSV value: its brightest channel divided by 255.",
        details: "Black is zero and any fully bright channel is one.",
        example: "let masked = source * intensity(mask);",
    },
    Builtin {
        function: F::Red,
        name: "red",
        group: G::Color,
        signatures: &[sig(&[("color", "color")], "float")],
        summary: "A color's red channel in 0..1, the scale `rgb` takes.",
        details: "`rgb(red(c), green(c), blue(c))` is `c` again.",
        example: "let warmth = red(source) - blue(source);",
    },
    Builtin {
        function: F::Green,
        name: "green",
        group: G::Color,
        signatures: &[sig(&[("color", "color")], "float")],
        summary: "A color's green channel in 0..1.",
        details: "",
        example: "let swapped = rgb(green(source), red(source), blue(source));",
    },
    Builtin {
        function: F::Blue,
        name: "blue",
        group: G::Color,
        signatures: &[sig(&[("color", "color")], "float")],
        summary: "A color's blue channel in 0..1.",
        details: "",
        example: "let gray = (red(source) + green(source) + blue(source)) / 3.0;",
    },
    Builtin {
        function: F::Invert,
        name: "invert",
        group: G::Color,
        signatures: &[sig(&[("color", "color")], "color")],
        summary: "Each channel complemented: `255 - channel`.",
        details: "Inverting black gives white.",
        example: "invert(input)",
    },
    Builtin {
        function: F::CurveClamped,
        name: "curve_clamped",
        group: G::Resources,
        signatures: &[sig(
            &[
                ("curve", "curve"),
                ("position", "float"),
                ("low", "float"),
                ("high", "float"),
            ],
            "float",
        )],
        summary: "`clamp(curve[position], low, high)` as one operation.",
        details: "Indexing a curve, `curve[position]`, samples it at a position in 0..1; an empty curve gives NaN.",
        example: "let level = curve_clamped(envelope, progress, 0.0, 1.0);",
    },
    Builtin {
        function: F::CurveIntegral,
        name: "curve_integral",
        group: G::Resources,
        signatures: &[sig(&[("curve", "curve"), ("position", "float")], "float")],
        summary: "The area under the curve from position 0 to `position`.",
        details: "End values are held beyond the authored points, so the area keeps growing past them; a negative position gives a negative area. Multiply by `duration` to integrate a rate over the clip's seconds. An empty curve or a NaN position gives NaN.",
        example: "let spawned = curve_integral(rate, progress) * duration;",
    },
    Builtin {
        function: F::GradientColorScaled,
        name: "gradient_color_scaled",
        group: G::Resources,
        signatures: &[sig(
            &[
                ("gradient", "gradient"),
                ("position", "float"),
                ("scale", "float"),
            ],
            "color",
        )],
        summary: "`gradient[position] * clamp(scale, 0, 1)` as one operation.",
        details: "Indexing a gradient, `gradient[position]`, samples its color at a position in 0..1; an empty gradient or a NaN position gives black.",
        example: "gradient_color_scaled(colors, pixel.fraction, level)",
    },
    Builtin {
        function: F::MarkLast,
        name: "mark_last",
        group: G::Events,
        signatures: &[sig(&[("marks", "marks"), ("time", "float")], "float")],
        summary: "The time in seconds of the last mark at or before `time`.",
        details: "NaN before the first mark.",
        example: "let age = time - mark_last(beats, time);",
    },
    Builtin {
        function: F::MarkLastIndex,
        name: "mark_last_index",
        group: G::Events,
        signatures: &[sig(&[("marks", "marks"), ("time", "float")], "int")],
        summary: "The zero-based index of the last mark at or before `time`.",
        details: "-1 before the first mark.",
        example: "let beat = mark_last_index(beats, time);",
    },
    Builtin {
        function: F::MarkAt,
        name: "mark_at",
        group: G::Events,
        signatures: &[sig(&[("marks", "marks"), ("index", "int")], "float")],
        summary: "The time in seconds of the mark at `index`.",
        details: "NaN outside `0..mark_count(marks)`.",
        example: "let next = mark_at(beats, beat + 1);",
    },
    Builtin {
        function: F::MarkCount,
        name: "mark_count",
        group: G::Events,
        signatures: &[sig(&[("marks", "marks")], "int")],
        summary: "How many marks the collection holds; the same as `len(marks)`.",
        details: "",
        example: "guard mark_count(beats) > 0;",
    },
    Builtin {
        function: F::CurveFirstCrossing,
        name: "curve_first_crossing",
        group: G::Events,
        signatures: &[sig(&[("curve", "curve"), ("value", "float")], "float")],
        summary: "The first position in 0..1 where the curve reaches `value`.",
        details: "NaN when it never does, or for an empty curve.",
        example: "let arrival = curve_first_crossing(chase, pixel.fraction);",
    },
    Builtin {
        function: F::CurveLastCrossing,
        name: "curve_last_crossing",
        group: G::Events,
        signatures: &[sig(
            &[("curve", "curve"), ("value", "float"), ("before", "float")],
            "float",
        )],
        summary: "The last position at or before `before` where the curve reaches `value`.",
        details: "NaN when there is none.",
        example: "let left = curve_last_crossing(chase, pixel.fraction, progress);",
    },
    Builtin {
        function: F::SectionCount,
        name: "section_count",
        group: G::Sections,
        signatures: &[sig(&[("width", "int")], "int")],
        summary: "How many runs of `width` pixels the target is split into.",
        details: "A per-fixture effect sections each fixture; a whole-target effect sections the target's fixtures in order. Widths below one count as one.",
        example: "let sections = section_count(10);",
    },
    Builtin {
        function: F::SectionIndex,
        name: "section_index",
        group: G::Sections,
        signatures: &[sig(&[("width", "int")], "int")],
        summary: "The zero-based run of `width` pixels the current pixel is in.",
        details: "",
        example: "let lit = section_index(10) % 2 == 0;",
    },
    Builtin {
        function: F::SectionPosition,
        name: "section_position",
        group: G::Sections,
        signatures: &[sig(&[("width", "float")], "float")],
        summary: "The current pixel's position within its run of `width` pixels, in `[0, 1)`.",
        details: "Widths below one count as one.",
        example: "let ramp = section_position(10.0);",
    },
    Builtin {
        function: F::Rand,
        name: "rand",
        group: G::Random,
        signatures: &[sig(&[("seed", "float")], "float")],
        summary: "A pseudo-random number in `[0, 1)` hashed from `seed`.",
        details: "Pure: the same seed always gives the same value, on every pixel, frame, controller and seek. Combine what identifies a decision (pixel, time bucket, mark) with a user seed parameter. Integer seeds above 2^24 lose precision. NaN gives NaN.",
        example: "let twinkle = rand((seed * 31.0 + pixel.index) * 31.0 + floor(time * 8.0));",
    },
    Builtin {
        function: F::Len,
        name: "len",
        group: G::Collections,
        signatures: &[
            sig(&[("array", "array<T>")], "int"),
            sig(&[("marks", "marks")], "int"),
        ],
        summary: "How many items an array or marks collection holds.",
        details: "Indexing an array, `array[i]`, clamps `i` to the first or last item and floors a float `i`; an empty array gives the item type's default.",
        example: "let color = colors[pixel.index % len(colors)];",
    },
];

pub(crate) fn builtin(name: &str) -> Option<&'static Builtin> {
    BUILTINS.iter().find(|builtin| builtin.name == name)
}

/// Whether `name` is a context value or the scope of context fields, like
/// `time` or `pixel`.
pub(crate) fn is_context_name(name: &str) -> bool {
    CONTEXT
        .iter()
        .any(|value| value.name.split('.').next() == Some(name))
}

/// A context value: a reserved name or a `pixel` or `target` field.
#[derive(Debug)]
pub struct ContextValue {
    pub name: &'static str,
    pub ty: &'static str,
    pub summary: &'static str,
}

pub(crate) const CONTEXT: &[ContextValue] = &[
    ContextValue {
        name: "time",
        ty: "float",
        summary: "Seconds since the effect's clip started; in an operator, since the sequence started.",
    },
    ContextValue {
        name: "duration",
        ty: "float",
        summary: "The clip's duration in seconds; in an operator, the sequence's.",
    },
    ContextValue {
        name: "progress",
        ty: "float",
        summary: "`time / duration`, in 0..1.",
    },
    ContextValue {
        name: "pixel.index",
        ty: "int",
        summary: "The pixel's zero-based index within the target; in an operator, within its fixture instance.",
    },
    ContextValue {
        name: "pixel.fraction",
        ty: "float",
        summary: "The pixel's index scaled to 0..1 across the target; in an operator, across its fixture instance.",
    },
    ContextValue {
        name: "pixel.x",
        ty: "float",
        summary: "The pixel's layout x coordinate in meters, after fixture transforms.",
    },
    ContextValue {
        name: "pixel.y",
        ty: "float",
        summary: "The pixel's layout y coordinate in meters.",
    },
    ContextValue {
        name: "target.count",
        ty: "int",
        summary: "How many pixels the target has; in an operator, its fixture instance.",
    },
    ContextValue {
        name: "target.min_x",
        ty: "float",
        summary: "The smallest x coordinate of the target's pixels.",
    },
    ContextValue {
        name: "target.min_y",
        ty: "float",
        summary: "The smallest y coordinate of the target's pixels.",
    },
    ContextValue {
        name: "target.max_x",
        ty: "float",
        summary: "The largest x coordinate of the target's pixels.",
    },
    ContextValue {
        name: "target.max_y",
        ty: "float",
        summary: "The largest y coordinate of the target's pixels.",
    },
    ContextValue {
        name: "PI",
        ty: "float",
        summary: "π, half a turn in radians.",
    },
    ContextValue {
        name: "TAU",
        ty: "float",
        summary: "2π, a full turn in radians.",
    },
];

/// The generated builtin reference, `docs/effect_builtins.md`.
pub fn builtin_reference() -> String {
    let mut text = String::new();
    let _ = writeln!(
        text,
        "# Effect language builtins\n\n\
         <!-- Generated from crates/donder-language/src/compiler/builtins.rs by \
         `pnpm generate:builtins`; do not edit. -->\n\n\
         Every builtin function and context value of the [effect \
         language](effect_language.md). An int argument is accepted wherever a float \
         is expected. Signatures listing both int and float forms keep ints as ints \
         when every argument is an int.\n\n## Context values\n\n\
         | Name | Type | Meaning |\n| --- | --- | --- |"
    );
    for value in CONTEXT {
        let _ = writeln!(
            text,
            "| `{}` | {} | {} |",
            value.name, value.ty, value.summary
        );
    }
    for group in BuiltinGroup::ALL {
        let _ = writeln!(text, "\n## {}", group.title());
        for builtin in BUILTINS.iter().filter(|builtin| builtin.group == group) {
            let _ = writeln!(text, "\n### `{}`\n\n```text", builtin.name);
            for signature in builtin.signatures {
                let args = signature
                    .args
                    .iter()
                    .map(|(name, ty)| format!("{name}: {ty}"))
                    .collect::<Vec<_>>()
                    .join(", ");
                let _ = writeln!(text, "{}({args}) -> {}", builtin.name, signature.result);
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
