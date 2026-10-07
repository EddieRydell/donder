# Effect language tutorial

This tutorial builds effects step by step, from a solid color to a beat-reactive
effect and a first operator. Each step adds one idea. The [language
reference](effect_language.md) has the full rules and the [builtin
reference](effect_builtins.md) describes every function.

Effects and operators live in script documents, files ending in `.donder`
(data documents end in `.data.donder`). Open one in Donder's source editor, or
start from `examples/starter/effects/standard.donder`. A script may hold several
declarations of either kind; each effect becomes a choice when you place an
effect on the timeline.

## 1. A solid color

```text
effect Solid {
  param color: color = #ff8800;

  sample { color }
}
```

An effect declares parameters and a `sample` block. Donder runs `sample` for
every pixel of the effect's target in every frame, and the block's last
expression is that pixel's color. `param` adds a control to the effect's
inspector; `= #ff8800` is its default.

## 2. A gradient across the target

```text
effect Spread {
  param colors: gradient;

  sample { colors[pixel.fraction] }
}
```

`pixel.fraction` goes from 0 at the target's first pixel to 1 at its last.
Indexing a gradient with a position in 0..1 samples its color there. A gradient
parameter has no literal default, so every placed effect chooses one.

## 3. Motion

```text
effect Rainbow {
  -- Turns of hue per second.
  param speed: float in 0.0..4.0 = 0.25;

  sample { hsv(pixel.fraction + time * speed, 1.0, 1.0) }
}
```

`time` is seconds since the clip started. `hsv` takes a hue in turns, so adding
`time * speed` scrolls the rainbow; hues wrap past 1. Comments start with `--`.
Numeric parameters declare a range with `in`, and values outside it are
rejected rather than clamped.

## 4. Shaping over the clip

```text
effect Swell {
  param colors: gradient;
  param level: curve in 0.0..1.0;

  sample { colors[pixel.fraction] * level[progress] }
}
```

`progress` goes from 0 to 1 over the clip. A curve, sampled like a gradient,
gives a number; multiplying a color by a number scales its brightness. Curve
parameters are drawn in the editor and can be automated.

## 5. Choices

```text
effect Sweep {
  param colors: gradient;
  param direction: enum { Forward, Backward } = Forward;

  sample {
    let position = if direction == Forward { progress } else { 1.0 - progress };
    colors[fract(pixel.fraction + position)]
  }
}
```

`let` names a value. `if` is an expression, so it always has an `else`. An enum
parameter offers its options in the inspector and compares by name. Options are
`PascalCase`.

## 6. Guards: drawing only some pixels

```text
effect Dot {
  param color: color = #ffffff;
  param width: float in 0.01..1.0 = 0.1;
  -- Trips along the target per second.
  param speed: float in 0.0..4.0 = 0.5;

  sample {
    let head = fract(time * speed);
    let distance = abs(pixel.fraction - head);
    guard distance < width;
    color * (1.0 - distance / width)
  }
}
```

`guard condition;` continues only when the condition holds; otherwise the pixel
is black. `fract` keeps the head moving through 0..1 forever.

## 7. Whole numbers

```text
effect Stripes {
  param a: color = #ff0000;
  param b: color = #ffffff;
  param width: int in 1..64 = 4;

  sample {
    let band = (pixel.index + int(time * 8.0)) // width;
    if band % 2 == 0 { a } else { b }
  }
}
```

`pixel.index` counts pixels from zero. `//` divides and rounds down, so every
`width` pixels share a band; `%` is the remainder, here alternating between two
colors. `/` always divides exactly, so `7 / 2` is 3.5 while `7 // 2` is 3.
`int(...)` turns a float into a whole number.

## 8. Randomness

```text
effect Twinkle {
  param color: color = #ffffff;
  param seed: float in 0.0..1000.0 = 1.0;
  param rate: float in 0.5..30.0 = 6.0;

  sample {
    let moment = floor(time * rate);
    let chance = rand((seed * 31.0 + pixel.index) * 31.0 + moment);
    guard chance > 0.9;
    color
  }
}
```

`rand` hashes its seed to a number in `[0, 1)`. The same seed always gives the
same number, so the twinkle looks identical on every controller and every replay.
Mixing in the pixel and a time bucket gives each pixel its own pattern.

## 9. Repetition

```text
effect Dots {
  param color: color = #00aaff;
  param count: int in 1..8 = 3;
  param width: float in 0.01..0.5 = 0.05;

  sample {
    max for dot in 0..count {
      let head = fract(time * 0.25 + dot / count);
      let distance = abs(pixel.fraction - head);
      guard distance < width;
      color * (1.0 - distance / width)
    }
  }
}
```

A reduction repeats a block over a range and combines the results. `max`
keeps the brightest; `sum`, `min`, `any`, `all`, `first` and `last` combine in
other ways. Inside a reduction, `guard` skips one iteration. The compiler must
prove a reduction's length, here from `count`'s range.

## 10. Reacting to marks

```text
effect Flash {
  param color: color = #ffffff;
  param beats: marks;
  param decay: float in 0.05..2.0 = 0.3;

  sample {
    let age = time - mark_last(beats, time);
    guard age < decay;
    color * (1.0 - age / decay)
  }
}
```

A marks parameter is a list of moments, such as beats tapped along the audio.
`mark_last` gives the latest mark at or before a time. Before the first mark it
is NaN, the missing value; any comparison with NaN is false, so the guard keeps
those pixels black.

## 11. Functions

```text
fn glow "Brightness of a soft dot of `width` around `head`." (position: float, head: float, width: float) -> float {
  max(0.0, 1.0 - abs(position - head) / width)
}

effect TwoDots {
  param color: color = #ffcc00;

  sample {
    let a = glow(pixel.fraction, fract(time * 0.5), 0.1);
    let b = glow(pixel.fraction, fract(time * 0.5 + 0.5), 0.1);
    color * max(a, b)
  }
}
```

A function names a calculation you use more than once. Its arguments and result
have types, and it sees only its arguments and context values like `time`, so
pass parameters in. Calls are expanded where they appear, so functions cost
nothing at playback.

The string after `glow` is an optional description. Effects, operators and
parameters take one too (`effect TwoDots "Two dots chasing." {`,
`param color: color = #ffcc00 "Dot color.";`), and the inspector shows them.

## 12. A first operator

```text
operator Trail {
  input source;
  param seconds: float in 0.0..2.0 = 0.2;
  param fade: float in 0.0..1.0 = 0.5;

  sample { max(source, source.at(time - seconds) * fade) }
}
```

An operator transforms other signals instead of drawing from nothing. It
declares `input`s, which you connect in the sequence's composition graph. Using
an input as a color samples the current pixel now; `source.at(time)` samples it
at another time, and `source.at(time, index)` at another pixel of the same
fixture. `max` of two colors keeps the brighter of each channel.

## Next steps

- The [builtin reference](effect_builtins.md) lists every function, with edge
  cases and examples.
- The [language reference](effect_language.md) covers numbers, NaN, reductions
  and limits precisely.
- `examples/starter/effects` and `examples/starter/operators` hold complete
  effects to read and adapt, including [ports of Vixen's
  effects](vixen_effects.md).
