# Effect language

Effects and operators are written in Donder's effect language, stored in
script documents (`*.donder`, as opposed to `*.data.donder` data documents;
see [project language](project_language.md)), and compiled by
`donder-language` (see [effect compiler](effect_compiler.md)). The bundled
libraries are ordinary project files; `examples/starter` contains every bundled
effect and operator.

The language is pure: a definition computes the current pixel's color from
time, its parameters, its pixel and target, and (for operators) its input
signals. Nothing is mutable and nothing carries from one frame to the next, so
every frame is a function of its time alone, and every value can be computed
wherever and as rarely as its dependencies allow.

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

  sample { source[target.count - 1 - pixel.index] }
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
`float`, `bool`, `color`, `enum { ... }`, `curve`, `gradient`, `marks`, `text`,
`font`, `image` and `array<T>` of any of those but arrays, texts, fonts and
images. Required parameters have no default and must be supplied by every
instance.

Effects, operators and enum options are `PascalCase` (`Forward`,
`AcrossItems`); parameters, inputs, functions, arguments, `let` bindings and
reduction indexes are `snake_case`. A name in the wrong case is an error with a
fix. Parameters and inputs appear in data documents, so they also cannot be
`none`, `import` or `from`. An option is written bare where a value of that enum is
expected: as a parameter's default and in comparisons with a value of that
enum, such as `direction == Backward`. In data documents a clip's value for an
enum parameter is the same bare option.

### Options with fields

A parameter's enum options may carry fields: the settings that matter only
while that option is chosen.

```text
param speed_format: enum {
  RevolutionCount { revolutions: float in 0.01..1000.0 = 3.0 },
  RevolutionFrequency { frequency_hz: float in 0.01..1000.0 = 2.0 },
  FixedTime { revolution_time_ms: float in 1.0..600000.0 = 500.0 },
} = RevolutionCount;
```

- A field is written like a parameter, `name: type [in min..max] = default
  ["description"]`, and fields are separated by commas. A field is an `int`,
  `float`, `bool`, `color` or an `enum` without fields, and it always has a
  default, so an option chosen anew starts from its fields' defaults. Fields
  are `snake_case`; options of one enum may reuse a field name.
- A parameter's default names only the option: its fields take their defaults.
- In data documents a clip's value is the option with every one of its fields,
  in order, as in `speed_format: FixedTime { revolution_time_ms: 250.0 }`; an
  option without fields stays bare. The inspector shows the option and then
  only that option's fields.
- Only a parameter's own enum has fields: not a function argument, an
  annotated `let` or an array's items. An enum whose options carry fields
  cannot be automated or set live.
- `speed_format == FixedTime` still compares the option. Fields are read with
  [`match`](#match).
- Playback reads the option as an int and each field as a parameter of its
  own, so fields cost what plain parameters do.

`int`, `float`, `curve` and `array<curve>` parameters declare an inclusive range
with `in min..max`. A curve is stored normalized, its point values in 0..1, and
binding maps them onto a curve parameter's range (every item's, for an
`array<curve>`) before the program sees them, so one curve, a library curve
included, fits any curve parameter. The editor's curve axis shows the range,
and a new curve falls from the top of it to the bottom. Defaults, authored
values and edits outside the range (outside 0..1 for curve points) are
rejected, never clamped, so code can rely on it instead of re-validating its
inputs.

Parameters of type float, int, bool, enum (without fields) and curve can be automated. Automation
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
  let age = time - mark_at(beats, mark_last_index(beats, time));
  guard age >= 0.0 && age < decay;
  gradient[age / decay]
  ```
- Array literals such as `[a, b, c]` may be indexed (`[a, b, c][i]`, clamped)
  or measured with `len()`; they hold numbers, bools or colors.
- `match` chooses by an enum's option; see [Match](#match).

### Match

```text
let revolution_ms = match speed_format {
  RevolutionCount { revolutions } => duration * 1000.0 / revolutions,
  RevolutionFrequency { frequency_hz } => 1000.0 / frequency_hz,
  FixedTime { revolution_time_ms } => revolution_time_ms,
};
```

- `match value { arms }` is an expression with one arm per option, `Option =>
  value`, in any order and separated by commas. `_ => value` covers every
  option no other arm names; an arm for an option that is not there, a second
  arm for one, a missing one or a `_` that covers nothing is an error.
- `Option { field, ... } =>` brings the option's named fields into scope in
  its arm, each under its own name. Only a match on the enum parameter itself
  binds fields; a `let` copy or a function argument has only the option.
- Arms are values like `if` branches: they produce one type, an int widening
  to a float, and a `guard` in an arm's block skips as it would in an `if`.
- A match is a chain of comparisons with the option, so it costs what the
  same `if` chain does.

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

### Layout and structure

Every pixel has a layout position, `pixel.x`, `pixel.y` and `pixel.z`, and a
place in its clip's target: `pixel.member` (which fixture or slice of the
target), `pixel.strand` (which strand: a shape of a fixture, or a row or
column of a grid as wired) and `pixel.along` (its place along that strand).
`target.members`, `target.strands` and `target.strand_length` (the longest
strand's pixels) size them. A whole target numbers its members' strands in
member order; per fixture, each fixture numbers its own. A program that reads
depth or structure costs the playback buffer four more rows, and its targets
a few words per pixel.

Other sequencers' render styles are a choice of coordinates:

| Style | x | y |
| --- | --- | --- |
| Layout, from the front | `pixel.x` | `pixel.y` |
| From above or the side | `pixel.x` or `pixel.z` | `pixel.z` or `pixel.y` |
| Strings (Vixen), per strand (xLights) | `pixel.along` of `target.strand_length` | `pixel.strand` of `target.strands` |
| Single line | `pixel.index` of `target.count` | 0 |
| Horizontal stack, per fixture | `pixel.member` plus the fixture's own position | its own position |
| As a pixel | 0 | 0 |

The standard library's `turned_x` and `turned_y` turn and zoom a point about a
pivot, and `ping_pong` runs time back and forth; `integral(speed)` changes an
effect's speed without jumps, and `min(time, t)` freezes it at `t`.

### Audio

`audio_level`, `audio_band` and `audio_wave` read the sequence's audio: its
loudness in a frequency range, one band of its spectrum, or its signal. Each
call names an analysis by its settings, numbers or parameters, so each clip
fixes them; preparation analyzes the audio once per distinct analysis, on the
host, and the program reads the samples, so the device never decodes audio.
A parameter that sets an analysis cannot be automated, and a function's
settings are checked where it is inlined. Until the analysis arrives, without
audio and in a console, the audio reads as silence.

```text
effect Bass {
  param decay: float in 0.0..2.0 = 0.2;
  sample { rgb(audio_level(time, 0.0, 200.0, 0.0, decay), 0.0, 0.0) }
}
```

Levels and bands are sampled 50 times a second and waveforms 2000 times, a
byte each, so an analysis costs show memory: 50 bytes a second for a level,
50 per band for a spectrum, 2000 for a waveform.

### Tables

A table is an array computed once per frame, before any pixel, and then read
by every pixel at an index of its own:

```text
let heights = table for i in 0..bars { levels[i / bars] };
let starts = table sum for i in 0..count { widths[i] };
heights[pixel.index % bars]
```

- `table for i in a..b { x }` holds `x` at each index: entry `k` is `x` at
  `i = a + k`. `table sum for` holds running sums, one entry more than the
  range: entry `k` sums the first `k` values, so entry 0 is zero and the last
  is the total.
- The body reads parameters, time, the target, its index and other tables. It
  cannot read the pixel, an operator input, another reduction or an
  enclosing reduction's index, and it cannot skip an index with `guard`.
- Entries are ints, floats, bools or colors (running sums: ints, floats or
  colors).
- A table spans at most 128 indices (a running sum at most 127), proven from
  literals and parameter ranges.
- Tables are indexed and measured like arrays: `t[k]` clamps `k` to the
  entries, an empty table reads the type's default, and `len(t)` counts the
  entries.

### Text

A `text` is up to 64 characters: a string literal (`"Merry Christmas"`), a
text parameter, a mark's label (`mark_text`), a number (`number_text`) or
two joined (`concat`). `t[k]` is the code of character `k` and
`text_length(t)` counts them. A `font` parameter names a font file and a line
height in pixels; preparation rasterizes it on the host, antialiased, for
printable ASCII and every character of the program's literals, its text
parameters and its sequence's mark labels. One text is one line.

`glyph_coverage(font, text, x, y)` is how much of the line covers the point
`x` pixels right of its start and `y` up from its bottom, 0 to 1, between the
font's pixels; `text_width` measures the line and `char_index` finds the
character at `x`. Scrolling, scaling and turning text are coordinates:

```text
effect Scroll {
  param message: text = "Happy Holidays";
  param font: font;
  param color: color = #ffffff;
  sample {
    let scale = font_height(font) / (target.max_y - target.min_y);
    let x = (pixel.x - target.min_x) * scale - progress * text_width(font, message);
    color * glyph_coverage(font, message, x, (pixel.y - target.min_y) * scale)
  }
}
```

A text computed during playback, such as the current mark's label, is a
table of the frame, so changing it costs nothing more than drawing it. Until
the font is rasterized, with no font and for a file that cannot be read,
every character is blank and takes no space. A font costs show memory: about
its height squared in bytes for each character it draws.

### Images

An `image` parameter names a picture or an animation (PNG, JPEG, GIF, BMP or
WebP, animated GIF, PNG and WebP included) and the longest side, in pixels, it
is scaled down to; preparation decodes it on the host, over black where it is
transparent. `image_color(image, frame, x, y)` is a frame's color `x` pixels
right of its left edge and `y` up from its bottom, blended between its
pixels' centers and black outside it. An animation plays at one rate,
`image_rate` frames a second (at most 50), each frame repeated for as long as
it lasts; frames repeat, so a frame number may count up forever, and a still
picture has one frame and rate 0. Fitting, scrolling, turning and tiling are
coordinates:

```text
effect Picture {
  param picture: image;
  sample {
    let scale = image_height(picture) / (target.max_y - target.min_y);
    let shown = int(time * image_rate(picture));
    image_color(picture, shown, (pixel.x - target.min_x) * scale, (pixel.y - target.min_y) * scale)
  }
}
```

The image is chosen once a frame, not per pixel. Until it is decoded, with no
image and for a file that cannot be read, it is black and measures 0. An
image costs show memory: three bytes per pixel per frame.

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
  `scan backward` the other way. The scanned signal is an input.
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
  spreads light both ways. A definition runs at most 256 scans.

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

The context names (`time`, `duration`, `progress`, `frame`, `frame_rate`,
`pixel`, `target`, `PI`, `TAU`) are reserved, and a function cannot take a builtin's name. In an effect,
time is measured over the clip; in an
operator, over the sequence. Spatial values use layout-space meters, after
fixture transforms.

**Colors** are three 32-bit floats, red, green and blue, where 0 is off and 1
  is full brightness, in programs and frames alike. Nothing clamps
  them between operations: light can add past 1 or go negative, and NaN
  follows IEEE 754 arithmetic. Color `+` adds, `*` of two colors multiplies
  channels, and `*` with a number scales. Authored hex colors are 8-bit per
  channel (`#ff8000` is 1, 0.502, 0). Only output to controllers converts:
  each channel is clamped to 0..1 and becomes a byte through the route's gamma
  and brightness, rounded or dithered, so NaN and negative light are off. Hue is measured in turns and `hsv` wraps it in both directions.
  `intensity` is HSV value (the maximum channel); grayscale colors have hue and
  saturation zero.

## Operator inputs

An operator reads its input signals in the current frame:

- `source` used as a color reads the current pixel.
- `source[pixel]` (an int index) reads a zero-based pixel of the current
  fixture instance. Local coordinates never wrap across fixtures. To read neighbors,
  use an `around` reduction.

Out-of-range pixels return black. Every input's whole frame is computed before
the operator runs, so a read at any pixel is one load. A signal is read only
at the current frame. A spatial read keeps the pixels it needs when a controller
fragment is prepared, even if they are patched elsewhere; see
[output selection](output_selection.md).

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

## Events

Event queries take explicit coordinates and return the event's coordinate, not
elapsed time. Curve positions are normalized; mark times are seconds.

| Query | Result |
| --- | --- |
| `mark_at(marks, mark_last_index(marks, time))` | Latest mark at or before `time`, or NaN |
| `mark_last_index(marks, time)` | Its index, or `-1` |
| `mark_at(marks, index)` | Time of that mark, or NaN for a missing index |
| `mark_duration(marks, index)` | How long that mark lasts (zero for a beat or a live mark), or NaN |
| `len(marks)` | Number of marks |
| `curve_first_crossing(curve, value)` | First position where the curve reaches `value`, or NaN |
| `curve_last_crossing(curve, value, position)` | Latest crossing at or before `position`, or NaN |

Marks are kept in time order; at equal times the last index wins. Crossings work
on increasing, decreasing and nonmonotonic curves. An exact touch counts, and a
flat stretch at the value counts once, on arrival. Before the first mark, NaN
reaches the color and the pixel is black; each new mark restarts a pulse.

## Limits

- A sequence has at most 250,000 prepared frames.
- A definition has at most 64 reductions. Its most general program (every
  parameter automated) may hold at most 192 bytes of per-pixel values at once
  and nest at most 16 per-pixel choices and reductions, and run at most 256
  scans; more is a compile error.
- Reductions and tables are bounded as described above.

Preparation and playback never report errors at frame time. Programs are
checked once when they compile; a show is trusted when it loads.

## Standard library

`effects/standard.donder` is the one effect library new projects include:

- Pulse, Chase, Spin and Wipe, and the mark-driven MarkPulse, MarkChase and
  MarkWipe.
- RadialWipe, FieldWipe and MarkFieldWipe.
- Effects ported from Vixen 3: SetLevel, Alternating, Strobe, Candle, Twinkle,
  Dissolve, Pinwheel, Butterfly, Colorwash, Plasma, Bars, Border, Spiral,
  Shockwave, Garlands, Curtain, Spirograph, Fire, Fireworks, Meteors and
  Snowflakes. [Vixen effects](vixen_effects.md) maps Vixen's effects and
  settings onto them.

- **Spin** wraps a chase position extended by `revolutions` onto the strand.
- **Wipe** projects pixel positions onto `direction_angle` (degrees; 0 is +X,
  90 is +Y) across the target's bounding rectangle, independent of pixel order.
  `pulse_width` is the fraction of the clip each pixel's pulse lasts, and
  `passes` sweeps the pulse across that many times, one after another. `mode`
  `Pass` lights a pixel only while its pulse crosses it, `On` keeps it lit
  after the last pass, and `Off` lights it before the first.
- **MarkPulse and MarkWipe** restart from the latest mark.
  MarkPulse lights random clusters, placed anew on every mark. MarkWipe cycles
  its gradients and positions by mark index; its `direction_angle` curve is in
  turns, sampled at the mark's time.
- **MarkChase** shares one chase curve across marks: a pixel's travel delay is
  where the curve first reaches it, so a new mark replaces the old pulse only when
  its chase arrives.
- **RadialWipe** sends a pulse outward from `center_x`, `center_y` (each
  -1..1 across the target) by Circle, Diamond or Burst distance;
  `pulse_length` is its share of the clip. **FieldWipe** and **MarkFieldWipe**
  light pixels by a field of arrival times over their location (linear,
  saddle and radial terms), so a wipe's shape follows where pixels are, not
  which fixture holds them.
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
| Grit | `input` | Rough texture: scales each group of `grain_pixels` by a random amount down to `1 - grain` (`grain = 0.6`), redrawn every `hold_frames = 3` frames; multiplies by `drive = 1.0`, clips each channel at full brightness and rounds it to `levels = 256` steps; with chance `tear` per hold, shifts each band of `tear_pixels = 16` up to `tear_shift = 8` pixels along the fixture |

The starter adds Gain as a separate operator document. `examples/stanford_room`
carries its own copy of the library with HueMap added.
