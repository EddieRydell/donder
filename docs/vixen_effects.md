# Vixen effects

[`examples/starter/effects/vixen.donder`](../examples/starter/effects/vixen.donder)
holds approximate ports of Vixen 3 effects, for shows converted from Vixen. New
projects include it as `vixen`; definitions are prefixed `Vixen`, for example
`vixen.VixenPinwheel`. The ports follow the
[Vixen 3.13u2 sources](https://github.com/VixenLights/Vixen/tree/3.13u2/src/Vixen.Modules/Effect)
closely enough to look like the original, not to reproduce its output exactly.

Parameters keep Vixen's names, defaults and scales, so a converted effect
copies its Vixen settings across:

- Curves use positions `0..1` and Vixen's percentage values `0..100`; an empty
  curve reads as 100. Curves and gradients have no inline defaults, so a clip
  supplies them; each definition's comment names Vixen's default.
- `colors` and `levels` arrays pair by index, and a shorter `levels` array
  reuses its last curve.
- Vixen draws pixel effects on a grid of cells. The ports run its formulas on
  each pixel's exact position instead, in a frame spanning the target's bounds
  with y = 0 at the bottom. A frame unit is a hundredth of the target's larger
  extent, so a feature Vixen sizes in cells (a meteor's width, a garland's sag,
  a flake) is that many hundredths of the target. `scale_x` and `scale_y`
  (default 1) stretch the pattern around the target's center. Meteors and Snowflakes can
  instead use Vixen's Strings layout: `string_count` strings of
  `string_length` consecutive pixels.
- Pinwheel, Fireworks and Spirograph move a fixed amount per Vixen frame, so
  they take the source show's `frame_interval_ms`. The other ports run on
  continuous time.
- Random choices are seeded by `random_seed`. They are repeatable, but they are
  not Vixen's random stream.

| Group | Definitions |
| --- | --- |
| Basic | SetLevel, Pulse, Alternating, Strobe, Candle, Chase, Spin, Wipe, Twinkle, Dissolve |
| Pixel | Pinwheel, Butterfly, Colorwash, Plasma, Bars, Border, Spiral, Shockwave, Garlands, Curtain, Spirograph, Fire, Fireworks, Meteors, Snowflakes |

Each definition's comment lists the Vixen modes it covers. Modes no imported
show has needed yet (mark-driven Strobe and Alternating, Bounce/Wrap movement, snowflake fades)
are left out. A missing Vixen effect or mode is added the same way: a
reasonable likeness of the effect that is cheap enough to play on the
controller.
