# Effect language builtins

<!-- Generated from crates/donder-language/src/dsl/builtins.rs by `pnpm generate:builtins`; do not edit. -->

Every builtin function and context value of the [effect language](effect_language.md). An int argument is accepted wherever a float is expected. Signatures listing both int and float forms keep ints as ints when every argument is an int.

## Context values

| Name | Type | Meaning |
| --- | --- | --- |
| `time` | float | Seconds since the effect's clip started; in an operator, since the sequence started. |
| `duration` | float | The clip's duration in seconds; in an operator, the sequence's. |
| `progress` | float | `time / duration`, in 0..1. |
| `pixel.index` | int | The pixel's zero-based index within the target; in an operator, within its fixture instance. |
| `pixel.fraction` | float | The pixel's index scaled to 0..1 across the target; in an operator, across its fixture instance. |
| `pixel.x` | float | The pixel's layout x coordinate in meters, after fixture transforms. |
| `pixel.y` | float | The pixel's layout y coordinate in meters. |
| `target.count` | int | How many pixels the target has; in an operator, its fixture instance. |
| `target.min_x` | float | The smallest x coordinate of the target's pixels. |
| `target.min_y` | float | The smallest y coordinate of the target's pixels. |
| `target.max_x` | float | The largest x coordinate of the target's pixels. |
| `target.max_y` | float | The largest y coordinate of the target's pixels. |
| `PI` | float | π, half a turn in radians. |
| `TAU` | float | 2π, a full turn in radians. |

## Math

### `sin`

```text
sin(x: float) -> float
```

Sine of an angle in radians.
NaN gives NaN. Never folded at compile time, so host and device agree.

```text
let wave = sin(time * TAU);
```

### `cos`

```text
cos(x: float) -> float
```

Cosine of an angle in radians.
NaN gives NaN.

```text
let wave = cos(pixel.fraction * PI);
```

### `tan`

```text
tan(x: float) -> float
```

Tangent of an angle in radians.
Grows without bound near odd multiples of PI / 2. NaN gives NaN.

```text
let slope = tan(angle);
```

### `atan2`

```text
atan2(y: float, x: float) -> float
```

Angle of the point (x, y) from the positive x axis, in radians from -PI to PI.
Note the argument order: y first.

```text
let angle = atan2(pixel.y - 1.0, pixel.x - 1.0);
```

### `sqrt`

```text
sqrt(x: float) -> float
```

Square root.
Negative numbers and NaN give NaN.

```text
let distance = sqrt(dx * dx + dy * dy);
```

### `exp`

```text
exp(x: float) -> float
```

e raised to `x`.
Large inputs give infinity. NaN gives NaN.

```text
let decay = exp(-age * 4.0);
```

### `log`

```text
log(x: float) -> float
```

Natural logarithm.
Zero gives negative infinity; negative numbers and NaN give NaN.

```text
let octave = log(frequency / 440.0) / log(2.0);
```

### `pow`

```text
pow(base: float, exponent: int) -> float
pow(base: float, exponent: float) -> float
```

`base` raised to `exponent`.
An int exponent that the compiler can bound to 0..10,000 multiplies `base` into one that many times, exactly. Any other exponent, including negative ints, uses the general power function: `pow(2, -1)` is 0.5. A negative base with a fractional exponent is NaN.

```text
let falloff = pow(1.0 - progress, 3);
```

### `abs`

```text
abs(x: int) -> int
abs(x: float) -> float
```

Absolute value; an int stays an int.
The int minimum wraps to itself, like negation.

```text
let distance = abs(pixel.index - center);
```

### `sign`

```text
sign(x: int) -> int
sign(x: float) -> float
```

-1, 0 or 1 by the sign of `x`; an int stays an int.
A float zero keeps its sign and NaN stays NaN.

```text
let direction = sign(velocity);
```

### `min`

```text
min(a: int, b: int) -> int
min(a: float, b: float) -> float
```

The smaller argument. Two ints give an int; otherwise both are floats.
NaN in either argument gives NaN.

```text
let last = min(radius, target.count - 1 - pixel.index);
```

### `max`

```text
max(a: int, b: int) -> int
max(a: float, b: float) -> float
max(a: color, b: color) -> color
```

The larger argument. Two ints give an int; two colors take the larger of each channel.
NaN in either float argument gives NaN.

```text
let first = max(-radius, -pixel.index);
```

### `clamp`

```text
clamp(x: int, low: int, high: int) -> int
clamp(x: float, low: float, high: float) -> float
```

`x` limited to `low..=high`. Three ints give an int.
Float clamping with `low > high`, or NaN in any argument, gives NaN. Int clamping applies `low` first, so `low > high` gives `high`.

```text
let level = clamp(age / fade, 0.0, 1.0);
```

### `mix`

```text
mix(a: float, b: float, t: float) -> float
mix(a: color, b: color, t: float) -> color
```

Linear interpolation from `a` at `t = 0` to `b` at `t = 1`.
`t` outside 0..1 extrapolates; color channels stay within 0..255. A NaN `t` gives NaN, or black for colors.

```text
let color = mix(#ff0000, #0000ff, pixel.fraction);
```

### `smoothstep`

```text
smoothstep(low: float, high: float, x: float) -> float
```

0 below `low`, 1 above `high`, and a smooth cubic between.
`low == high` divides by zero and gives NaN.

```text
let edge = smoothstep(0.4, 0.6, pixel.fraction);
```

### `step`

```text
step(edge: float, x: float) -> float
```

0 when `x < edge`, otherwise 1.
NaN in either argument gives NaN.

```text
let on = step(0.5, progress);
```

## Rounding and conversion

### `floor`

```text
floor(x: float) -> float
```

The largest whole number at most `x`, as a float.
Use `x // 1` for the same value as an int when `x` is an int expression, or `int(floor(x))` for a float.

```text
let step = floor(time * 4.0);
```

### `ceil`

```text
ceil(x: float) -> float
```

The smallest whole number at least `x`, as a float.
NaN gives NaN.

```text
let bars = ceil(height / 3.0);
```

### `trunc`

```text
trunc(x: float) -> float
```

`x` without its fractional part, rounding toward zero, as a float.
NaN gives NaN.

```text
let whole = trunc(value);
```

### `round`

```text
round(x: float) -> float
```

The nearest whole number, halves away from zero, as a float.
`round(2.5)` is 3 and `round(-2.5)` is -3. NaN gives NaN.

```text
let frame = round(time * 40.0);
```

### `round_even`

```text
round_even(x: float) -> float
```

The nearest whole number, halves to the even neighbor, as a float.
`round_even(2.5)` is 2 and `round_even(3.5)` is 4.

```text
let frame = round_even(time * 40.0);
```

### `fract`

```text
fract(x: float) -> float
```

The fractional part `x - floor(x)`, in `[0, 1)`.
Negative numbers wrap: `fract(-0.25)` is 0.75. Infinity and NaN give NaN.

```text
let phase = fract(time * 0.5 + pixel.fraction);
```

### `int`

```text
int(x: float) -> int
```

A float converted to an int, truncating toward zero.
Saturates at the int range and turns NaN into zero. Write `int(floor(x))` to floor negative values.

```text
let index = int(progress * len(colors));
```

## Missing values

### `is_nan`

```text
is_nan(x: float) -> bool
```

Whether `x` is NaN, the missing value.
Comparisons with NaN are always false, so test missing values with this.

```text
guard !is_nan(last_beat);
```

### `value_or`

```text
value_or(x: float, replacement: float) -> float
```

`x`, or `replacement` when `x` is NaN.
Both arguments are always computed; use an `if` when the replacement is expensive.

```text
let age = value_or(time - mark_last(beats, time), 1000.0);
```

## Color

### `rgb`

```text
rgb(red: float, green: float, blue: float) -> color
```

A color from channels in 0..1.
Channels clamp to 0..1 and round to 8 bits. NaN in any channel gives black.

```text
let warm = rgb(1.0, 0.6, 0.2);
```

### `hsv`

```text
hsv(hue: float, saturation: float, value: float) -> color
```

A color from hue in turns, saturation and value in 0..1.
Hue wraps in both directions, so 1.25 is 0.25. NaN in any argument gives black.

```text
let rainbow = hsv(pixel.fraction + time * 0.1, 1.0, 1.0);
```

### `hue`

```text
hue(color: color) -> float
```

A color's hue in turns, in `[0, 1)`.
Grays have hue zero.

```text
let shifted = hsv(hue(source) + 0.5, saturation(source), intensity(source));
```

### `saturation`

```text
saturation(color: color) -> float
```

A color's HSV saturation in 0..1.
Grays have saturation zero.

```text
let pastel = saturation(source) < 0.5;
```

### `intensity`

```text
intensity(color: color) -> float
```

A color's HSV value: its brightest channel divided by 255.
Black is zero and any fully bright channel is one.

```text
let masked = source * intensity(mask);
```

### `red`

```text
red(color: color) -> float
```

A color's red channel in 0..1, the scale `rgb` takes.
`rgb(red(c), green(c), blue(c))` is `c` again.

```text
let warmth = red(source) - blue(source);
```

### `green`

```text
green(color: color) -> float
```

A color's green channel in 0..1.

```text
let swapped = rgb(green(source), red(source), blue(source));
```

### `blue`

```text
blue(color: color) -> float
```

A color's blue channel in 0..1.

```text
let gray = (red(source) + green(source) + blue(source)) / 3.0;
```

### `invert`

```text
invert(color: color) -> color
```

Each channel complemented: `255 - channel`.
Inverting black gives white.

```text
invert(input)
```

## Curves and gradients

### `curve_clamped`

```text
curve_clamped(curve: curve, position: float, low: float, high: float) -> float
```

`clamp(curve[position], low, high)` as one operation.
Indexing a curve, `curve[position]`, samples it at a position in 0..1; an empty curve gives NaN.

```text
let level = curve_clamped(envelope, progress, 0.0, 1.0);
```

### `gradient_color_scaled`

```text
gradient_color_scaled(gradient: gradient, position: float, scale: float) -> color
```

`gradient[position] * clamp(scale, 0, 1)` as one operation.
Indexing a gradient, `gradient[position]`, samples its color at a position in 0..1; an empty gradient or a NaN position gives black.

```text
gradient_color_scaled(colors, pixel.fraction, level)
```

## Marks and crossings

### `mark_last`

```text
mark_last(marks: marks, time: float) -> float
```

The time in seconds of the last mark at or before `time`.
NaN before the first mark.

```text
let age = time - mark_last(beats, time);
```

### `mark_last_index`

```text
mark_last_index(marks: marks, time: float) -> int
```

The zero-based index of the last mark at or before `time`.
-1 before the first mark.

```text
let beat = mark_last_index(beats, time);
```

### `mark_at`

```text
mark_at(marks: marks, index: int) -> float
```

The time in seconds of the mark at `index`.
NaN outside `0..mark_count(marks)`.

```text
let next = mark_at(beats, beat + 1);
```

### `mark_count`

```text
mark_count(marks: marks) -> int
```

How many marks the collection holds; the same as `len(marks)`.

```text
guard mark_count(beats) > 0;
```

### `curve_first_crossing`

```text
curve_first_crossing(curve: curve, value: float) -> float
```

The first position in 0..1 where the curve reaches `value`.
NaN when it never does, or for an empty curve.

```text
let arrival = curve_first_crossing(chase, pixel.fraction);
```

### `curve_last_crossing`

```text
curve_last_crossing(curve: curve, value: float, before: float) -> float
```

The last position at or before `before` where the curve reaches `value`.
NaN when there is none.

```text
let left = curve_last_crossing(chase, pixel.fraction, progress);
```

## Sections

### `section_count`

```text
section_count(width: int) -> int
```

How many runs of `width` pixels the target is split into.
A per-fixture effect sections each fixture; a whole-target effect sections the target's fixtures in order. Widths below one count as one.

```text
let sections = section_count(10);
```

### `section_index`

```text
section_index(width: int) -> int
```

The zero-based run of `width` pixels the current pixel is in.

```text
let lit = section_index(10) % 2 == 0;
```

### `section_position`

```text
section_position(width: float) -> float
```

The current pixel's position within its run of `width` pixels, in `[0, 1)`.
Widths below one count as one.

```text
let ramp = section_position(10.0);
```

## Randomness

### `rand`

```text
rand(seed: float) -> float
```

A pseudo-random number in `[0, 1)` hashed from `seed`.
Pure: the same seed always gives the same value, on every pixel, frame, controller and seek. Combine what identifies a decision (pixel, time bucket, mark) with a user seed parameter. Integer seeds above 2^24 lose precision. NaN gives NaN.

```text
let twinkle = rand((seed * 31.0 + pixel.index) * 31.0 + floor(time * 8.0));
```

## Collections

### `len`

```text
len(array: array<T>) -> int
len(marks: marks) -> int
```

How many items an array or marks collection holds.
Indexing an array, `array[i]`, clamps `i` to the first or last item and floors a float `i`; an empty array gives the item type's default.

```text
let color = colors[pixel.index % len(colors)];
```
