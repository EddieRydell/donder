# Effect language

Effects and operators are written in Donder's effect language, stored in
`.effect.donder` and `.operator.donder` documents, and compiled by
`donder-language` to typed bytecode. The bundled libraries are ordinary project
files; `examples/starter` contains every bundled effect and operator.

## Declarations

```c
effect Wash {
  param gradient colors;
  param curve level;
  param enum direction { forward, backward } = forward;
  param int bands = 4;

  color sample() {
    float position = pixel_fraction();
    if (direction == backward) {
      position = 1.0 - position;
    }
    return colors[position] * level[progress()];
  }
}

operator Mirror {
  input Signal source;

  color sample() {
    return source.at(seconds(), pixel_count() - 1 - pixel_index());
  }
}
```

An effect computes one color for each pixel of its target in every frame. An
operator does the same over its connected `input Signal` ports. Parameter types
are `int`, `float`, `bool`, `color`, `enum`, `curve`, `gradient`, `marks` and
arrays of them. Required parameters have no default and must be supplied by every
instance.

Parameters of type float, int, bool, enum and curve can be automated. Preparation
specializes control flow on every parameter that an effect instance does not
automate.

Curves and gradients are indexed by a normalized position: `level[0.5]`,
`colors[t]`. Arrays are indexed by an integer that clamps to the first or last
element; an empty array yields the element type's default, and `len()` is zero.
Use `colors[int(position)]` to index with a computed float.

Integers widen to floats implicitly in assignments, arguments and mixed
arithmetic, and `==` and `!=` compare an int with a float as floats. `int(x)`
is the only conversion the other way: it truncates toward zero, saturates at the
int range, and turns NaN into zero. Write `int(floor(x))` to floor negative
values.

## Context and builtins

| Group | Functions |
| --- | --- |
| Time | `seconds()`, `duration()`, `progress()` |
| Pixel | `pixel_index()`, `pixel_count()`, `pixel_fraction()` |
| Space (meters) | `pixel_x()`, `pixel_y()`, `target_min_x()`, `target_min_y()`, `target_max_x()`, `target_max_y()` |
| Sections | `section_count(width)`, `section_index(width)`, `section_position(width)` |
| Math | `sin`, `cos`, `abs`, `floor`, `sqrt`, `atan2(y, x)`, `min`, `max`, `clamp`, `smoothstep`, `mix`, constants `PI` and `TAU` |
| Conversion | `int(x)` |
| Missing values | `is_nan(x)`, `value_or(x, replacement)` |
| Color | `rgb(r, g, b)`, `hsv(h, s, v)`, `hue(c)`, `saturation(c)`, `intensity(c)`, `invert(c)`, `mix(a, b, t)`, `max(a, b)` |
| Resources | `curve_clamped(curve, position, min, max)`, `gradient_color_scaled(gradient, position, scale)` |
| Events | `mark_last(marks, time)`, `mark_last_index(marks, time)`, `mark_at(marks, index)`, `mark_count(marks)`, `curve_first_crossing(curve, value)`, `curve_last_crossing(curve, value, position)` |
| Random | `rand(seed)` |
| Other | `len(array or marks)` |

- **Time:** in an effect, `seconds()` is time since the clip started,
  `duration()` is the clip's duration, and `progress()` is their ratio in
  `[0, 1]`.
- **Pixels and space:** indices count pixels within the effect's target.
  Spatial functions use layout-space meters, after fixture transforms.
- **Random:** `rand(seed)` hashes one float to a value in `[0, 1)`. It is pure:
  the same seed always gives the same value, so every pixel, frame, controller
  and seek agrees. Combine whatever identifies the decision (a pixel or section,
  a time bucket, a mark time) with a user `param float seed`, for example
  `rand((seed * 31.0 + section) * 31.0 + bucket)`. Integer seeds above 2^24 lose
  precision as floats.
- **Colors** have 8-bit RGB channels. Color `+` saturates, and `*` with a number
  scales. Hue is measured in turns and `hsv` wraps it in both directions.
  `intensity` is HSV value (the maximum channel divided by 255); grayscale
  colors have hue and saturation zero.

## Operator inputs

An operator samples an immutable input signal by time and pixel:

- `source.at(seconds)` samples the current pixel.
- `source.at(seconds, pixel)` samples a zero-based pixel of the current fixture
  instance. Local coordinates never wrap across fixtures.
- `source.at_global(seconds, pixel)` samples a zero-based pixel of the full
  layout, in instance order with each instance's pixels contiguous. This order
  does not depend on which controller ports are selected.

Out-of-range pixels and times outside the sequence return black, without
evaluating the input. Queries can sample other operators and combine spatial and
temporal changes. A spatial read keeps the pixels it needs when a controller
fragment is prepared, even if they are patched elsewhere; see
[output selection](output_selection.md).

## Loops

- `for (int mark in beats) { ... }` visits the indices of a `marks` value in
  order.
- `for (int i in range(count, cap)) { ... }` runs `max(0, min(count, cap))`
  times. The cap must be a positive integer literal of at most 10,000.
- C-style `for` loops need a trip count the compiler can prove constant and at
  most 10,000; other loops are rejected.

Loop indices cannot be assigned in the body.

## Numeric rules

- NaN means "missing": empty curves, an unavailable mark, a query before the
  first event. It propagates through float arithmetic, `min`, `max`, `clamp` and
  `rand`, and every NaN comparison is false. `clamp` with `min > max` also
  returns NaN.
- A NaN reaching a color becomes black: `rgb` or `hsv` with any NaN input, color
  `mix` or scaling by NaN, and sampling a gradient at NaN. That black is an
  ordinary color afterwards (inverting it gives white).
- `value_or` evaluates both arguments; use `if` when the replacement is
  expensive.
- Integer division produces a float. Integer `+`, `-`, `*` and negation wrap at
  32 bits; remainder by zero and `i32::MIN % -1` return zero. Integer comparisons
  stay in integers.
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
flat stretch at the value counts once, on arrival. A mark-triggered pulse:

```c
float age = seconds() - mark_last(beats, seconds());
return gradient[age / pulse_duration] * pulse_shape[age / pulse_duration];
```

Before the first mark, NaN reaches the color and the pixel is black. Each new
mark restarts the pulse.

`section_count(width)` and `section_index(width)` split the target into runs of
`width` pixels that respect fixture boundaries: a short final section belongs to
its fixture. Two 113-pixel fixtures with width 7 have 17 sections each, and the
second fixture starts at section 17. A per-fixture effect sections its own
fixture; a whole-target effect sections the target's fixtures in order.

## Limits

- A sequence has at most 250,000 prepared frames.
- A program may use at most 64 integer, 256 float and 64 bool registers; more is a
  compile error naming the bank.
- Loops are bounded as described above.
- Ordinary jumps go forward; loops are paired counted instructions. Admission
  rejects malformed bytecode before it runs.

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
| Dim | `input` | Scales by `amount = 0.5`, clamped to `[0, 1]` |
| Invert | `input` | Complements each channel |
| Colorize | `input` | Scales `tint = #ffffff` by the input's brightest channel |
| HueShift | `source` | Adds `shift = 0.0` turns to the hue |
| Delay | `input` | Samples `seconds = 0.1` earlier |
| Echo | `input` | Maximum of the input and `repeats = 3` copies spaced `seconds = 0.1` apart, scaled by powers of `decay = 0.5` |

Echo clamps repeats to `[1, 32]` and decay to `[0, 1]`; samples before the
sequence are black. The starter adds Gain and TimeWarp as separate operator
documents. `examples/stanford_room` carries its own copy of the library with
FreezeFrame and HueMap added.
