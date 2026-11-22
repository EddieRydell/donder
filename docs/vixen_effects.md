# Vixen effects

Effects ported from Vixen 3 live in the one effect library,
[`effects/standard.donder`](../examples/starter/effects/standard.donder), beside
Donder's own. They follow the
[Vixen 3.13u2 sources](https://github.com/VixenLights/Vixen/tree/3.13u2/src/Vixen.Modules/Effect)
closely enough to look like the original, not to reproduce its output exactly.
This page maps a Vixen effect and its settings onto a library effect, for
converting a Vixen sequence.

## Effects

| Vixen effect | Library effect |
| --- | --- |
| Set Level, Alternating, Strobe, Candle Flicker, Twinkle, Dissolve | SetLevel, Alternating, Strobe, Candle, Twinkle, Dissolve |
| Pinwheel, Butterfly, Colorwash, Plasma, Bars, Border, Spiral, Shockwave, Garlands, Curtain, Spirograph, Fire, Fireworks, Meteors, Snowflakes | the same names |
| Pulse, Chase, Spin, Wipe | Pulse, Chase, Spin, Wipe; their parameters are Donder's, so read each definition |
| Circle, Diamond and Burst wipes | RadialWipe, with `shape` |

Each definition's comment lists the Vixen modes it covers. Modes no converted
show has needed yet (mark-driven Strobe and Alternating, Bounce/Wrap movement,
snowflake fades) are left out. A missing Vixen effect or mode is added the same
way: a reasonable likeness of the effect that is cheap enough to play on the
controller.

## Settings

A ported effect's parameters keep Vixen's names, except:

- Vixen's `ColorGradient` is `gradient`.
- Levels are `0..1`, not Vixen's percentages: divide by 100. This covers every
  `intensity`, Twinkle's `minimum_level`, `maximum_level`,
  `pulse_time_variation` and `average_coverage`, Pinwheel's `x_offset`,
  `y_offset` and `center_hub`, Shockwave's `center_x` and `center_y`, and
  Snowflakes' `initial_build_up`. RadialWipe's `pulse_length` is Vixen's pulse
  percentage over 100, and its `center_x` and `center_y` are Vixen's -100..100
  center over 100.
- Curves are stored with values in `0..1` like every curve: divide Vixen's curve
  percentages by 100. An empty curve reads as 1 (Vixen's 100). This covers
  every `levels` array, Strobe's `intensity_curve`, Dissolve's `dissolve_curve`,
  Shockwave's `radius_curve` and `width_curve`, Curtain's `position_curve` and
  `intensity_curve`, Fire's `height` and `hue_shift`, and Meteors'
  `tail_taper`.
- Sliders Vixen sets in its own 0..100 units keep them: speeds, Pinwheel's
  `movement`, `thickness`, `size`, `twist` and `rotation`, Spiral's `rotation`
  and `thickness`, Plasma's `line_density`, Garlands' `spacing`, Spirograph's
  radii and ranges, Fireworks' counts and fade, Meteors' and Snowflakes'
  counts and lengths, and the Advanced Border's thicknesses, size and offsets.

How the ports read their settings:

- Curves and gradients have no inline defaults, so a clip supplies them; each
  definition's comment names Vixen's default.
- `colors` and `levels` arrays pair by index, and a shorter `levels` array
  reuses its last curve.
- Vixen draws pixel effects on a grid of cells. The ports run its formulas on
  each pixel's exact position instead, in a frame spanning the target's bounds
  with y = 0 at the bottom. A frame unit is a hundredth of the target's larger
  extent, so a feature Vixen sizes in cells (a meteor's width, a garland's sag,
  a flake) is that many hundredths of the target. `scale_x` and `scale_y`
  (default 1) stretch the pattern around the target's center. Meteors and
  Snowflakes can instead use Vixen's Strings layout: `positioning: Strings {
  ... }` has `string_count` strings of `string_length` consecutive pixels.
- A Vixen setting that matters only in one mode is a field of that mode's
  option, under Vixen's name, so the inspector shows it only there: the
  `static_color` of Twinkle's `SingleColor`, Butterfly's `motion`
  (`iterations` and `direction`, or `speed`), Border's `border_mode`
  (`simple_border_width`, or the `Advanced` border's type, thicknesses, size
  and offsets), the directions of Meteors' `meteor_effect` and Snowflakes'
  `snowflake_effect`, and the Strings layout's `orientation`, `string_count`
  and `string_length`.
- Pinwheel, Fireworks and Spirograph move a fixed amount per Vixen frame, so
  they take the source show's `frame_interval_ms`. The other ports run on
  continuous time.
- Random choices are seeded by `random_seed`. They are repeatable, but they are
  not Vixen's random stream.
