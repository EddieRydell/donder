# Effect language

Effects and operators are written in Donder's effect language, stored in
`.effect.donder` and `.operator.donder` documents, and compiled by
`donder-language` (see [effect compiler](effect_compiler.md)). The bundled
libraries are ordinary project files; `examples/starter` contains every bundled
effect and operator.

The language is pure: a definition computes the current pixel's color from
time, its parameters, its pixel and target, and (for operators) its input
signals. Nothing is mutable, so every value can be computed wherever and as
rarely as its dependencies allow.

## Declarations

```text
effect Wash {
  param colors: gradient;
  param level: curve in 0.0..1.0;
  param direction: enum { forward, backward } = forward;
  param bands: int in 1..16 = 4;

  sample {
    let position = if direction == backward { 1.0 - pixel.fraction } else { pixel.fraction };
    colors[position] * level[progress]
  }
}

operator Mirror {
  input source;

  sample { source.at(time, target.count - 1 - pixel.index) }
}
```

An effect computes one color for each pixel of its target in every frame. An
operator does the same over its `input` signals. Parameter types are `int`,
`float`, `bool`, `color`, `enum { ... }`, `curve`, `gradient`, `marks` and
`array<T>` of any of those but arrays. Required parameters have no default and
must be supplied by every instance.

`int`, `float` and `curve` parameters declare an inclusive range with `in min..max`;
a curve's range bounds its point values. Defaults, authored values and edits
outside the range are rejected, never clamped, so code can rely on it instead of
re-validating its inputs.

Parameters of type float, int, bool, enum and curve can be automated. Automation
maps its normalized curve onto the declared range, an enum's options in order, or
a bool's false and true halves.

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
`colors[t]`. Arrays are indexed by an integer that clamps to the first or last
element; an empty array yields the element type's default, and `len()` is zero.
Use `colors[int(position)]` to index with a computed float.

Integers widen to floats implicitly in arithmetic, arguments, annotated lets and
`if` branches, and `==` and `!=` compare an int with a float as floats. `int(x)`
is the only conversion the other way: it truncates toward zero, saturates at the
int range, and turns NaN into zero. Write `int(floor(x))` to floor negative
values. Comparisons do not chain; combine them with `&&` and `||`.

## Reductions

Repeated work is a reduction over an integer range: `a..b` excludes `b` and
`a..=b` includes it.

| Reduction | Result |
| --- | --- |
| `max for i in a..b { x }` | Largest `x` (componentwise for colors), from black or negative infinity |
| `min for i in a..b { x }` | Smallest number, from positive infinity |
| `sum for i in a..b { x }` | Sum, from zero; color addition saturates |
| `any for i in a..b { c }` | Whether some iteration holds |
| `all for i in a..b { c }` | Whether every iteration holds |
| `first for i in a..b { x } else { y }` | `x` of the first iteration that is not skipped, or `y` |
| `last for i in a..b { x } else { y }` | `x` of the last such iteration, or `y` |

A `guard` in the body skips an iteration. For colors, `first` and `last`
default to black without an `else`. Only they take an `else`, so in
`guard all for i in 0..n { c } else value;` the `else` belongs to the guard.

Syntax nests at most 128 levels deep. Expressions, blocks and each operator
of a chain such as `a + b + c` are levels.

```text
operator Echo {
  input input;
  param seconds: float in 0.0..60.0 = 0.1;
  param repeats: int in 1..32 = 3;
  param decay: float in 0.0..1.0 = 0.5;

  sample {
    max for repeat in 0..=repeats {
      input.at(time - seconds * repeat) * pow(decay, repeat)
    }
  }
}
```

The compiler bounds each range's length from literals, parameter ranges, `len()`
of array and marks parameters, and enclosing indices, through arithmetic, `min`,
`max`, `clamp`, `abs`, `floor`, `ceil`, `trunc`, `round_even` and `int`. A range
with no such bound, or a bound above 10,000, is rejected. A bound that depends
on a length is checked when an instance supplies its values. `pow(x, n)` takes
an integer `n`, bounded the same way, and multiplies `x` into one `n` times.

## Context and builtins

| Group | Names |
| --- | --- |
| Time | `time`, `duration`, `progress` |
| Pixel | `pixel.index`, `pixel.fraction`, `pixel.x`, `pixel.y` |
| Target | `target.count`, `target.min_x`, `target.min_y`, `target.max_x`, `target.max_y` |
| Sections | `section_count(width)`, `section_index(width)`, `section_position(width)` |
| Math | `sin`, `cos`, `abs`, `floor`, `ceil`, `trunc`, `round_even` (ties to even), `sqrt`, `atan2(y, x)`, `pow(x, n)`, `min`, `max`, `clamp`, `smoothstep`, `mix`, constants `PI` and `TAU` |
| Conversion | `int(x)` |
| Missing values | `is_nan(x)`, `value_or(x, replacement)` |
| Color | `rgb(r, g, b)`, `hsv(h, s, v)`, `hue(c)`, `saturation(c)`, `intensity(c)`, `invert(c)`, `mix(a, b, t)`, `max(a, b)` |
| Resources | `curve_clamped(curve, position, min, max)`, `gradient_color_scaled(gradient, position, scale)` |
| Events | `mark_last(marks, time)`, `mark_last_index(marks, time)`, `mark_at(marks, index)`, `mark_count(marks)`, `curve_first_crossing(curve, value)`, `curve_last_crossing(curve, value, position)` |
| Random | `rand(seed)` |
| Other | `len(array or marks)` |

The context names (`time`, `duration`, `progress`, `pixel`, `target`, `PI`,
`TAU`) are reserved.

- **Time:** in an effect, `time` is seconds since the clip started,
  `duration` is the clip's duration, and `progress` is their ratio in
  `[0, 1]`. In an operator they are measured over the sequence.
- **Pixels and space:** indices count pixels within the effect's target.
  Spatial values use layout-space meters, after fixture transforms.
- **Random:** `rand(seed)` hashes one float to a value in `[0, 1)`. It is pure:
  the same seed always gives the same value, so every pixel, frame, controller
  and seek agrees. Combine whatever identifies the decision (a pixel or section,
  a time bucket, a mark time) with a user `param seed: float`, for example
  `rand((seed * 31.0 + section) * 31.0 + bucket)`. Integer seeds above 2^24 lose
  precision as floats.
- **Colors** have 8-bit RGB channels. Color `+` saturates, `*` of two colors
  multiplies channels, and `*` with a number scales. Hue is measured in turns and
  `hsv` wraps it in both directions. `intensity` is HSV value (the maximum
  channel divided by 255); grayscale colors have hue and saturation zero.

## Operator inputs

An operator samples an immutable input signal by time and pixel:

- `source` used as a color samples the current pixel now.
- `source.at(t)` samples the current pixel at `t` seconds.
- `source.at(t, pixel)` samples a zero-based pixel of the current fixture
  instance. Local coordinates never wrap across fixtures.
- `source.at_global(t, pixel)` samples a zero-based pixel of the full layout,
  in instance order with each instance's pixels contiguous. This order does not
  depend on which controller ports are selected.

Out-of-range pixels and times outside the sequence return black, without
evaluating the input. Samples can query other operators and combine spatial and
temporal changes. A spatial read keeps the pixels it needs when a controller
fragment is prepared, even if they are patched elsewhere; see
[output selection](output_selection.md).

## Numeric rules

- NaN means "missing": empty curves, an unavailable mark, a query before the
  first event. It propagates through float arithmetic, `min`, `max`, `clamp` and
  `rand`, and every NaN comparison is false. `clamp` with `min > max` also
  returns NaN.
- A NaN reaching a color becomes black: `rgb` or `hsv` with any NaN input, color
  `mix` or scaling by NaN, and sampling a gradient at NaN. That black is an
  ordinary color afterwards (inverting it gives white).
- `value_or` computes both arguments; write `if is_nan(x) { y } else { x }` when
  the replacement is expensive, so it is computed only when needed.
- Integer division produces a float. Integer `+`, `-`, `*` and negation wrap at
  32 bits. `%` is a floored remainder for ints and floats: a nonzero result takes
  the divisor's sign, so `x % n` wraps into `[0, n)` for a positive `n`. Integer
  remainder by zero and `i32::MIN % -1` return zero; float remainder by zero is
  NaN. Integer comparisons stay in integers.
- Real-number algebra may change intermediate rounding: division by a value that
  is fixed for an instance multiplies by its reciprocal.
- Curves are piecewise linear with strictly increasing positions in `[0, 1]`.
  Sampling outside the authored range holds the end values, so a pulse shape must
  end at zero to stay dark. Sequence automation uses the same curve sampling.
- Gradients with no stops, or sampled at NaN, return black.
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
second fixture starts at section 17. A per-fixture effect sections its own
fixture; a whole-target effect sections the target's fixtures in order.

## Limits

- A sequence has at most 250,000 prepared frames.
- A definition has at most 64 reductions. Its most general program (every
  parameter automated) may hold at most 192 bytes of per-pixel values at once
  and nest at most 16 per-pixel choices and reductions; more is a compile
  error.
- Reductions are bounded as described above.

Preparation and playback never report errors at frame time. Prepared-archive
decoding checks structure and resource budgets and trusts the compiler for the
rest; see [ESP32 loading](esp32_loading.md).

## Standard library

`effects/standard.effect.donder` defines Pulse, Chase, Spin, Wipe, MarkPulse,
MarkChase and MarkWipe. ImpactBurst and MarkImpactBurst are separate documents.
New projects include all of these. The starter project also has ScanSweep,
ShimmerField, SparkleComet and [the Vixen ports](vixen_effects.md).

- **Spin** wraps a chase position extended by `revolutions` onto the strand.
- **Wipe** projects pixel positions onto `direction_angle` (degrees; 0 is +X,
  90 is +Y) across the target's bounding rectangle, independent of pixel order.
  `pulse_width` is the fraction of the effect each pixel's pulse lasts.
- **MarkPulse, MarkWipe and MarkImpactBurst** restart from the latest mark.
  MarkPulse lights fixture-aware sections. MarkWipe cycles its gradients and
  positions by mark index; its `direction_angle` curve is in turns, sampled
  at the mark's time.
- **MarkChase** shares one chase curve across marks: a pixel's travel delay is
  where the curve first reaches it, so a new mark replaces the old pulse only when
  its chase arrives.
- Mark effects stop at their clip boundary and after the pulse duration. Empty
  mark or gradient collections produce black.

`operators/standard.operator.donder`:

| Operator | Inputs | Behavior |
| --- | --- | --- |
| Max | `a`, `b` | Component-wise maximum |
| Add | `a`, `b` | Saturating RGB addition |
| Multiply | `a`, `b` | Component-wise multiplication |
| IntensityModulate | `source`, `mask` | Scales `source` by the mask's brightest channel |
| Dim | `input` | Scales by `amount = 0.5` |
| Invert | `input` | Complements each channel |
| Colorize | `input` | Scales `tint = #ffffff` by the input's brightest channel |
| HueShift | `source` | Adds `shift = 0.0` turns to the hue |
| Delay | `input` | Samples `seconds = 0.1` earlier |
| Echo | `input` | Maximum of the input and `repeats = 3` copies spaced `seconds = 0.1` apart, scaled by powers of `decay = 0.5` |
| Blur | `input` | Tent-weighted average of `radius = 2` pixels on each side along each fixture's pixel order, renormalized at fixture ends |

Samples before the sequence are black. The starter adds Gain and TimeWarp as
separate operator documents. `examples/stanford_room` carries its own copy of the
library with FreezeFrame and HueMap added.
