# Effect language

Effects and operators are written in Donder's effect language, stored in
script documents (`*.donder`, as opposed to `*.data.donder` data documents;
see [project language](project_language.md)), and compiled by
`donder-language` (see [effect compiler](effect_compiler.md)). The bundled
libraries are ordinary project files; `examples/starter` contains every bundled
effect and operator.

The language is pure: a definition computes the current pixel's color from
time, its parameters, its pixel and target, its signal's previous frame and
(for operators) its input signals. Nothing is mutable, so every value can be computed wherever and as
rarely as its dependencies allow.

New to the language? Start with the [tutorial](effect_tutorial.md). Every builtin
function and context value is described in the generated [builtin
reference](effect_builtins.md).

## Declarations

```text
effect Wash "A gradient across the target, faded by a curve." {
  param colors: gradient "Colors from one end to the other.";
  param level: curve in 0.0..1.0;
  param direction: enum { Forward, Backward } = Forward;
  param bands: int in 1..16 = 4;

  sample {
    let position = if direction == Backward { 1.0 - pixel.fraction } else { pixel.fraction };
    colors[position] * level[progress]
  }
}

operator Mirror "The input reversed across the target." {
  input source;

  sample { source.at(target.count - 1 - pixel.index) }
}
```

A script may hold effects, operators and functions in any mix; each
declaration's keyword says what it is. Comments start with `--` and run to the
end of the line. Because of that, a subtracted negation needs a space: `a - -b`.

Effects, operators, parameters and functions may carry a description string,
which the inspector shows: after the name of an effect, operator or function,
and at the end of a parameter, after its default. Descriptions are optional.
Strings are double-quoted with the escapes `\"`, `\\`, `\n` and `\t`.

An effect computes one color for each pixel of its target in every frame. An
operator does the same over its `input` signals. Parameter types are `int`,
`float`, `bool`, `color`, `enum { ... }`, `curve`, `gradient`, `marks` and
`array<T>` of any of those but arrays. Required parameters have no default and
must be supplied by every instance.

Effects, operators and enum options are `PascalCase` (`Forward`,
`AcrossItems`); parameters, inputs, functions, arguments, `let` bindings and
reduction indexes are `snake_case`. A name in the wrong case is an error with a
fix. Parameters and inputs appear in data documents, so they also cannot be
`none`, `import` or `from`. An option is written bare where a value of that enum is
expected: as a parameter's default and in comparisons with a value of that
enum, such as `direction == Backward`. In data documents a clip's value for an
enum parameter is the same bare option.

`int`, `float`, `curve` and `array<curve>` parameters declare an inclusive range
with `in min..max`; a curve's range bounds its point values, and an
`array<curve>`'s range bounds every item's. The editor's curve axis spans the
range, and a new curve rises from its minimum to its maximum. Defaults, authored values and edits
outside the range are rejected, never clamped, so code can rely on it instead of
re-validating its inputs.

Parameters of type float, int, bool, enum and curve can be automated. Automation
maps its normalized curve (values in 0..1) onto the declared range, an enum's
options in order, or a bool's false and true halves. `integral(speed)` integrates
a float parameter over time, so a speed or rate used as `integral(speed)` rather
than `time * speed` keeps its place when automation changes it.

## Expressions

A `sample` block, like every block, is a list of statements followed by the
expression it produces.

- `let name = expression;` binds an immutable value. A later `let` may reuse
  the name, so `let v = v + sin(x);` refines a value step by step. `let name:
  float = 1;` converts an int.
- `if condition { a } else { b }` is an expression and always has an `else`;
  chain conditions with `else if`.
- `guard condition;` continues only when the condition holds. Otherwise the
  `sample` block produces black, or a reduction body contributes nothing.
  `guard condition else value;` produces `value` instead. Guards are the
  idiom for early exits:

  ```text
  let age = time - mark_last(beats, time);
  guard age >= 0.0 && age < decay;
  gradient[age / decay]
  ```
- Array literals such as `[a, b, c]` may be indexed (`[a, b, c][i]`, clamped)
  or measured with `len()`; they hold numbers, bools or colors.

Curves and gradients are indexed by a normalized position: `level[0.5]`,
`colors[t]`. `curve_integral(rate, progress) * duration` accumulates a curve
over the clip, so a varying rate or speed gives a count or position without
jumps. Arrays are indexed by a number that clamps to the first or last
element, so `colors[position * len(colors)]` picks a color directly; a float
index rounds down. An empty array yields the element type's default, and
`len()` is zero. Comparisons do not chain; combine them with `&&` and `||`.

## Numbers

`int` and `float` mix freely, and an int never silently loses its fraction:

- An int widens to a float wherever a float is expected: in arithmetic with a
  float, in arguments, annotated lets and `if` branches. `==` and `!=` compare an
  int with a float as floats.
- `/` is always real division: `7 / 2` is 3.5.
- `//` is floored division: `7 // 2` is 3 and `-7 // 2` is -4. Two ints give an
  int (division by zero gives zero); otherwise it is `floor(a / b)`.
- `%` is the matching floored remainder: `x % n` wraps into `[0, n)` for a
  positive `n`, and `a == (a // n) * n + a % n` for ints and a nonzero `n`.
- `+`, `-`, `*`, `//` and `%` of two ints give an int. Int arithmetic wraps at 32
  bits.
- `min`, `max`, `clamp`, `abs` and `sign` keep ints as ints when every argument
  is an int, and work on floats otherwise.
- `int(x)` is the only conversion from float to int: it truncates toward zero,
  saturates at the int range and turns NaN into zero. `floor`, `ceil` and
  `round` round but keep a float.

## Functions

A document may declare functions next to its effects or operators. Any
declaration in the document can call them, and functions can call each other:

```text
fn ease_out "How far `t` has eased out, sharper for a larger power." (t: float, power: int) -> float {
  1.0 - pow(1.0 - clamp(t, 0.0, 1.0), power)
}

effect Fade {
  param colors: gradient;
  sample { colors[pixel.fraction] * ease_out(progress, 3) }
}
```

Arguments and the result have explicit types; an int argument widens to a float
argument. The body is an ordinary block, except that a `guard` needs an `else`.
A function sees its arguments and the context values (`time`, `pixel`, `target`,
...), which belong to whichever effect or operator calls it. It cannot read the
caller's parameters or inputs, so pass them as arguments. Calls are expanded
where they appear, so a function costs nothing at playback and a reduction in it
is bounded by the arguments of each call. Functions cannot recurse or reuse a
builtin's name, and a function nobody calls is still checked.

## Reductions

Repeated work is a reduction over an integer range: `a..b` excludes `b` and
`a..=b` includes it.

| Reduction | Result |
| --- | --- |
| `max for i in a..b { x }` | Largest `x` (componentwise for colors), from black or negative infinity |
| `min for i in a..b { x }` | Smallest number, from positive infinity |
| `sum for i in a..b { x }` | Sum, from zero |
| `any for i in a..b { c }` | Whether some iteration holds |
| `all for i in a..b { c }` | Whether every iteration holds |
| `first for i in a..b { x } else { y }` | `x` of the first iteration that is not skipped, or `y` |
| `last for i in a..b { x } else { y }` | `x` of the last such iteration, or `y` |

A `guard` in the body skips an iteration. For colors, `first` and `last`
default to black without an `else`. Only they take an `else`, so in
`guard all for i in 0..n { c } else value;` the `else` belongs to the guard.

### Neighborhoods

An operator reads its input's neighbors along each fixture's pixel order with
`around`: the index is the offset from the current pixel, and `as` names the
input there, now.

```text
let glow = sum for offset in -radius..=radius around input as neighbor {
  let weight = threshold_weight(brightness(neighbor, measure), threshold, softness);
  guard weight > 0.0;
  neighbor * (weight * falloff_weight(offset, radius, falloff))
};
```

- A `sum` or `max` of the neighbor, optionally scaled by numbers and guarded,
  runs as one fast neighborhood pass. Each factor of the scale, and each
  guard, may depend on the neighbor (with parameters and time), on the offset,
  or on neither, but not on two of them: `neighbor * exp(offset *
  intensity(neighbor))` is an error. Change the neighbor's color after the
  reduction, not inside it.
- The factors are found after flattening every product and quotient of the
  scale, and every guard, including `if c { .. } else { black }` around the
  scaled neighbor, so grouping and order never matter: `neighbor * a * b`,
  `a * (neighbor * b)` and `neighbor * (a * b)` are the same pass. The same
  rule decides a [scan's](#scans) weight.
- Past a fixture's ends a neighbor is missing and contributes nothing.
  `around extended input` reads the end pixel there instead, and `around
  mirrored input` the pixels reflected about it (the end pixel is not
  repeated).
- A body that does not use the neighbor runs only for the offsets that have
  one, so `sum for offset in -r..=r around input { weight(offset) }` totals the
  weights a pixel near a fixture end actually receives.
- The range spans at most 128 offsets, and an extended or mirrored one reaches
  at most 64 pixels each way, as literals and parameter ranges must prove.

### Scans

A scan carries light along each fixture's pixel order, however far it reaches:

```text
let plain = scan backward input decay d;
let tail = scan forward input decay d as light {
  light * threshold_weight(brightness(light, measure), threshold, softness)
};
```

- At each pixel the scan's value is its value at the pixel before times the
  `decay`, plus the light the pixel adds; the first pixel's previous value is
  black. `scan forward` runs from each fixture's first pixel to its last,
  `scan backward` the other way. The scanned signal is an input or `previous`.
- `decay` and `as` are keywords only here. The decay may depend on parameters
  and time, not on the pixel.
- Without `as`, each pixel adds the input there. `as light { .. }` names the
  input at the pixel and computes the light it adds: `light` scaled by
  numbers, and guarded, that depend on `light`, parameters and time alone,
  grouped and ordered in any way. Anything else is an error.
- The running value is kept in floats and rounded to a color only when
  written, so faint light keeps fading instead of stopping at the last step,
  and the result saturates at full brightness.
- A scan's whole frame is computed once per frame, so its cost does not depend
  on how far the light travels. `max` or `+` of a forward and a backward scan
  spreads light both ways. A program runs at most 8 scans.
- Light that lingers from frame to frame reads `previous` instead; see
  [Previous frames](#previous-frames).

Syntax nests at most 128 levels deep. Expressions, blocks and each operator
of a chain such as `a + b + c` are levels.

The compiler bounds each range's length from literals, parameter ranges, `len()`
of array and marks parameters, and enclosing indices, through arithmetic, `min`,
`max`, `clamp`, `abs`, `floor`, `ceil` and `int`. A range
with no such bound, or a bound above 10,000, is rejected. A bound that depends
on a length is checked when an instance supplies its values. Write bounds as
ints; `int(...)` or `//` converts a float. `pow(x, n)` with an int `n` bounded
the same way, and at least zero, multiplies `x` into one `n` times; other
exponents use the general power function.

## Context and builtins

The [builtin reference](effect_builtins.md) lists every context value and
builtin function with its signatures, edge cases and an example. It is generated
from the compiler's builtin table, so it always matches what the compiler
accepts.

The context names (`time`, `duration`, `progress`, `frames_elapsed`,
`previous`, `pixel`, `target`, `PI`, `TAU`) are reserved, and a function cannot take a builtin's name. In an effect,
time is measured over the clip; in an
operator, over the sequence. Spatial values use layout-space meters, after
fixture transforms.

**Colors** are three 32-bit floats, red, green and blue, where 0 is off and 1
  is full brightness, in programs, frames and `previous` alike. Nothing clamps
  them between operations: light can add past 1 or go negative, and NaN
  follows IEEE 754 arithmetic. Color `+` adds, `*` of two colors multiplies
  channels, and `*` with a number scales. Authored hex colors are 8-bit per
  channel (`#ff8000` is 1, 0.502, 0). Only output to controllers converts:
  each channel is clamped to 0..1 and becomes a byte through the port's gamma
  table, so NaN and negative light are off. Hue is measured in turns and `hsv` wraps it in both directions.
  `intensity` is HSV value (the maximum channel); grayscale colors have hue and
  saturation zero.

## Operator inputs

An operator reads its input signals in the current frame:

- `source` used as a color reads the current pixel.
- `source.at(pixel)` reads a zero-based pixel of the current fixture
  instance. Local coordinates never wrap across fixtures. To read neighbors,
  use an `around` reduction.

Out-of-range pixels return black. Every input's whole frame is computed before
the operator runs, so a read at any pixel is one load. A signal is read only
at the current frame, and at the one before through `previous`. A spatial read keeps the pixels it needs when a controller
fragment is prepared, even if they are patched elsewhere; see
[output selection](output_selection.md).

## Previous frames

`previous` is the signal's own frame from the frame before: in an operator,
its output; in an effect, its layer's composited frame. It reads like an
input: `previous` at the current pixel, `previous.at(pixel)`, `around
previous` and `scan forward previous decay d`.
`frames_elapsed` is the number of frames since that frame, so a decay applied
per frame stays steady when frames are skipped:

```text
operator Afterglow {
  input input;
  param seconds: float in 0.01..10.0 = 0.5;

  sample {
    let decay = pow(0.1, 1.0 / (seconds * frame_rate));
    max(input, previous * pow(decay, frames_elapsed))
  }
}
```

- `previous` is black before the first frame and is never cleared: a clip
  that starts reads its layer as the clip before left it.
- After a seek or when playback goes backward, `frames_elapsed` is 1 and
  `previous` is whatever frame was shown last. Re-evaluating the same frame
  after a live command gives a `frames_elapsed` of 0.
- A clip evaluated alone, as the sequence editor's thumbnails are, reads a black
  `previous`.
- A signal that reads `previous` is never fused with its input or consumer, and
  a show may have at most 16 of them.

## Numeric rules

- NaN means "missing": empty curves, an unavailable mark, a query before the
  first event. It propagates through float arithmetic, `min`, `max`, `clamp` and
  `rand`, and every NaN comparison is false. `clamp` with `min > max` also
  returns NaN.
- A NaN color channel is written as zero; the other channels keep their values.
- `value_or` computes both arguments; write `if is_nan(x) { y } else { x }` when
  the replacement is expensive, so it is computed only when needed.
- Integer `//` and `%` by zero return zero, and `i32::MIN // -1` wraps; float
  remainder by zero is NaN. Integer comparisons stay in integers.
- Real-number algebra may change intermediate rounding: division by a value that
  is fixed for an instance multiplies by its reciprocal.
- Curves are piecewise linear with strictly increasing positions in `[0, 1]`.
  Sampling outside the authored range holds the end values, so a pulse shape must
  end at zero to stay dark. Sequence automation uses the same curve sampling.
- Gradients with no stops return black.
- Literal errors are compile errors; a bad literal is never replaced by zero.

## Events and sections

Event queries take explicit coordinates and return the event's coordinate, not
elapsed time. Curve positions are normalized; mark times are seconds.

| Query | Result |
| --- | --- |
| `mark_last(marks, time)` | Latest mark at or before `time`, or NaN |
| `mark_last_index(marks, time)` | Its index, or `-1` |
| `mark_at(marks, index)` | Time of that mark, or NaN for a missing index |
| `mark_count(marks)` | Number of marks |
| `curve_first_crossing(curve, value)` | First position where the curve reaches `value`, or NaN |
| `curve_last_crossing(curve, value, position)` | Latest crossing at or before `position`, or NaN |

Marks are kept in time order; at equal times the last index wins. Crossings work
on increasing, decreasing and nonmonotonic curves. An exact touch counts, and a
flat stretch at the value counts once, on arrival. Before the first mark, NaN
reaches the color and the pixel is black; each new mark restarts a pulse.

`section_count(width)` and `section_index(width)` split the target into runs of
`width` pixels that respect fixture boundaries: a short final section belongs to
its fixture. Two 113-pixel fixtures with width 7 have 17 sections each, and the
second fixture starts at section 17. A per-fixture clip sections its own
fixture; a whole-target clip sections the target's fixtures in order.

## Limits

- A sequence has at most 250,000 prepared frames.
- A definition has at most 64 reductions. Its most general program (every
  parameter automated) may hold at most 192 bytes of per-pixel values at once
  and nest at most 16 per-pixel choices and reductions; more is a compile
  error.
- Reductions are bounded as described above.

- A show's graph holds at most 16 signal frames at once and at most 16
  signals that read `previous`; a program runs at most 8 scans.

Preparation and playback never report errors at frame time. Programs are
checked once when they compile; a show is trusted when it loads.

## Standard library

`effects/standard.donder` defines Pulse, Chase, Spin, Wipe, MarkPulse,
MarkChase and MarkWipe. ImpactBurst and MarkImpactBurst are separate documents.
New projects include all of these. The starter project also has ScanSweep,
ShimmerField, SparkleComet and [the Vixen ports](vixen_effects.md).

- **Spin** wraps a chase position extended by `revolutions` onto the strand.
- **Wipe** projects pixel positions onto `direction_angle` (degrees; 0 is +X,
  90 is +Y) across the target's bounding rectangle, independent of pixel order.
  `pulse_width` is the fraction of the clip each pixel's pulse lasts, and
  `passes` sweeps the pulse across that many times, one after another. `mode`
  `Pass` lights a pixel only while its pulse crosses it, `On` keeps it lit
  after the last pass, and `Off` lights it before the first.
- **MarkPulse, MarkWipe and MarkImpactBurst** restart from the latest mark.
  MarkPulse lights random clusters, placed anew on every mark. MarkWipe cycles
  its gradients and positions by mark index; its `direction_angle` curve is in
  turns, sampled at the mark's time.
- **MarkChase** shares one chase curve across marks: a pixel's travel delay is
  where the curve first reaches it, so a new mark replaces the old pulse only when
  its chase arrives.
- Mark effects stop at their clip boundary and after the pulse duration. Empty
  mark or gradient collections produce black. With `overlap_marks` on, a new
  mark no longer cuts off earlier pulses: every pulse still running stays lit,
  up to the 32 latest, and where pulses overlap the brighter color wins.

`operators/standard.donder`:

| Operator | Inputs | Behavior |
| --- | --- | --- |
| Max | `a`, `b` | Component-wise maximum |
| Add | `a`, `b` | RGB addition |
| Multiply | `a`, `b` | Component-wise multiplication |
| IntensityModulate | `source`, `mask` | Scales `source` by the mask's brightest channel |
| Dim | `input` | Scales by `amount = 0.5` |
| Invert | `input` | Complements each channel |
| Colorize | `input` | Scales `tint = #ffffff` by the input's brightest channel |
| HueShift | `source` | Adds `shift = 0.0` turns to the hue |
| Blur | `input` | Tent-weighted average of `radius = 2` pixels on each side along each fixture's pixel order, renormalized at fixture ends |
| Threshold | `input` | Passes pixels whose brightness (`measure` = Brightest, Luminance or Average) exceeds `threshold = 0.5`, fading across `softness = 0.1`; `invert` passes the darker pixels instead |
| Trail | `input` | Carries each pixel's light along each fixture's pixel order, keeping `decay = 0.8` of it per pixel, `Forward`, `Backward` or `Both` (the brighter) |
| Bloom | `input` | Thresholds like Threshold (`threshold = 0.6`, `softness = 0.2`), spreads the passing light both ways along each fixture's pixel order with forward and backward scans, fading to a tenth over `radius = 6` pixels, scales it by `strength = 2.0` and `tint`, and combines it with the input by `blend` (Add, Screen, Lighten or GlowOnly). The glow fades off fixture ends, and its cost does not depend on the radius |
| Afterglow | `input` | Holds light after it goes out, fading it to a tenth over `seconds = 0.5`: the brighter of the input and its `previous` frame faded by the frames since; steady light shows unchanged, and its cost does not depend on the tail's length |

The starter adds Gain as a separate operator document. `examples/stanford_room`
carries its own copy of the library with HueMap added, and FreezeFrame, which
captures its input every `interval_seconds` into the frame it holds in
`previous` and fades it to a hundredth over `fade_seconds`.
