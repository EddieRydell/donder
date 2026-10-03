# Vixen effect compatibility

The reference is Vixen **3.13u2**, published September 17, 2026. Its source
descriptors contain **12 Basic** and **34 Pixel** effects. Vixen's help category
omits Basic Strobe; the release descriptors, active editor properties, data
defaults, and renderer must all be consulted when establishing a contract.

References:

- [Published effect documentation](https://www.vixenlights.com/docs/usage/sequencer/effects/)
- [Pinned release](https://github.com/VixenLights/Vixen/releases/tag/3.13u2)
- [Pinned effect implementations](https://github.com/VixenLights/Vixen/tree/3.13u2/src/Vixen.Modules/Effect)

Matching an effect name or its default appearance does not establish Vixen
compatibility. A complete match includes controls, defaults, units, every mode,
group selection, matrix construction, timing, random choices, color handling,
and the result after splitting outputs among controllers. The existing Donder
standard library is not a Vixen compatibility library.

## Bundled library

[`examples/starter/effects/vixen.effect.donder`](../examples/starter/effects/vixen.effect.donder)
contains **20 procedural ports: 9 Basic and 11 Pixel effects**. The starter
imports it as `vixen`; the desktop new-project template embeds that same source
file. Definitions are prefixed `Vixen`, for example `vixen.VixenPinwheel`.
This is partial coverage of the 46-effect inventory, with explicit mode limits.
It is not a sequence importer or a claim of complete interface/render parity.

| Definition | Implemented scope |
| --- | --- |
| `VixenSetLevel` | RGB color and intensity. |
| `VixenPulse` | RGB gradient with a percentage intensity curve. |
| `VixenAlternating` | Time intervals/static mode, consecutive-pixel groups, gradient/level pairs and interval skips. |
| `VixenStrobe` | Simple time-interval mode, 50% duty and per-pulse gradient/intensity. |
| `VixenChase` | Ascending linear movement, millisecond overlap, four RGB color modes, default level and pulse extensions. |
| `VixenSpin` | Revolution count/frequency/time, three pulse-length formats, reverse, four RGB color modes and default level. |
| `VixenCandleFlicker` | Bounded random walk, linear transitions, frequency/change deviations and consecutive-pixel groups. |
| `VixenWipe` | Horizontal/vertical Count mode, pass count, pulse percentage, two color modes, reverse and wipe on/off. |
| `VixenDissolve` | One monotonic fill or clear over the effect duration, sequential/random ordering, starting node, flip, groups and random colors. |
| `VixenPinwheel` | Location geometry, both motion modes, all four blade/color modes, twist, hub, offsets and size basis. |
| `VixenButterfly` | All five formulas, iteration motion, gradient/rainbow, direction, repeat, background chunks/skips and base color. |
| `VixenColorwash` | Iteration motion, center/outer/invert fades and alternating-frame shimmer. |
| `VixenPlasma` | Seven-term plasma formula, style/density, gradient blending and four RGB presets. |
| `VixenBars` | Dense-grid flat bars, iteration motion, cardinal/alternate/compress/expand directions, highlight and 3D. |
| `VixenBorder` | Simple/advanced borders, shared/independent edges, offsets, size and five gradient placements. |
| `VixenSpiral` | Iteration motion, forward/backward/stationary, repeats, rotation, thickness, grow/shrink, blend and 3D. |
| `VixenShockwave` | Circular ring, radius/width curves, center, edge blend, scaled radius and four acceleration presets. |
| `VixenGarlands` | Five dense-grid patterns, four directions and iteration motion with fixed spacing/iteration count. |
| `VixenCurtain` | Location-based Position mode, open/close, six edges, swag and per-iteration intensity. |
| `VixenSpirograph` | Sampled hypotrochoid, radii, distance animation, range and standard/random/rainbow colors. |

### Authoring contract

Import the file in each sequence document that uses it; imports from
`project.donder` are not inherited by other documents:

```yaml
imports:
- from:
    documents:
    - effects/vixen.effect.donder
  as: vixen
```

Use the parameter declarations and adjacent comments as the supported interface.
The library intentionally omits controls for unimplemented modes. The following
adaptations apply throughout:

- Curve positions are normalized to `0..1`, but values retain Vixen's `0..100`
  percentage scale. A flat full-intensity curve therefore has value `100`, not
  `1`. Empty/NaN curve samples use Vixen's value of 100 before percentage clamping.
- Scalar controls such as Pinwheel `twist` and `size` use the percentage values
  of Vixen's control curves and can use Donder automation. Their defaults match
  Vixen's default flat curves. This replaces Vixen's curve-valued control UI;
  it does not import those curves automatically or quantize external automation
  to the source frame clock. Parameters sampled at different positions within
  one invocation, such as blade color levels, remain actual curve parameters.
- Curves and gradients are required inputs because the DSL cannot declare
  their resource defaults inline. File comments identify Vixen's default
  palettes/shapes. `colors` and `levels` are parallel arrays with equal lengths;
  each gradient is paired with the curve at the same index.
- `frame_interval_ms` defaults to 50 and quantizes the pixel-effect clock.
  Set it to the source show's interval. It is a port parameter, not a new
  runtime builtin. It does not change the sequence's output frame rate.
- `pixel_size` defaults to one layout unit. Pixel ports derive a zero-based
  integer grid and its dimensions from the complete target bounds. This does
  not reconstruct Vixen string orientation, sparse-row compaction or prop
  hierarchy. Grid-derived ports identify that choice in their comments.
- Group controls operate on consecutive target pixels. RGB effects have no
  discrete-color prop filtering or alpha. Donder's color/gradient rounding and
  32-bit floating math remain in use. Overlapping basic RGB pulses use maximum
  channel composition rather than reproducing Vixen's complete intent pipeline.
- Random modes use an explicit fixed `random_seed` and Donder random samples.
  They are repeatable for the same inputs, but do not reproduce Vixen's random
  stream. Dissolve uses sorted random keys with index tie-breaking to construct
  a permutation; it does not simply give every pixel an independent threshold.

### Limits and work per pixel

These files add no runtime, compiler, elaboration or resource capabilities.
All computation stays within the existing `sample()` interface. Reconstructing
an effect's own event history or geometric points does not read another pixel's
color. The definitions use target-wide indices/bounds, never controller-local
buffer dimensions. Split-output equivalence has not been exercised for these
new definitions.

Arrays and explicit iteration counts must fit the DSL's 10,000-iteration limit;
Spiral additionally requires `len(colors) * repeat <= 10000`. Invalid/unsupported
inputs guarded in the source return black; they are not diagnosed by a custom
parameter validator. In particular:

- Candle replays at most 10,000 transitions per pixel evaluation. A conservative
  supported duration in seconds is
  `10000 / flicker_frequency * (1 - floor(frequency_deviation) / 100)`:
  375 seconds at its defaults. It does progressively more work as the effect
  advances; split longer effects or add preparation support before extending
  this contract. Parameters affecting event generation are fixed.
- Dissolve accepts at most 10,000 target pixels and only a single monotonic
  `0..100` fill or `100..0` clear curve. Random rank selection scans the target's
  indices for each evaluated pixel. Full-frame work therefore grows quadratically
  with target size. This is an implementation cost, not cross-pixel color sampling.
- Spirograph may examine 5,400 parametric points per evaluated pixel. It preserves
  the last covering point's color by searching backward. Preparation of the point
  set would be useful for larger displays; no playback performance claim is made.
- Shockwave supports the source acceleration-curve values 50 (linear), 55
  (`gentle_in`, its default), 45 (`gentle_out`) and 0 (`strong_out`). The formulas
  are respectively `t`, `t * sqrt(t)`, `1 - (1-t) * sqrt(1-t)` and
  `1 - (1-t)^5`. Other acceleration values require arbitrary exponentiation.
- Chase implements a linear forward movement only. Spin/Chase retain the source
  movement sampling step, but their algebraic scheduling and RGB composition
  have not been compared against Vixen's generated lighting intents.

### Not included

The remaining **26 named effects** have no declarations in this file:

- Basic: LipSync, State, Twinkle.
- Pixel: Balls, Circles, Count Down, Fire, Fireworks, Glediator, Life, Liquid,
  Meteors, Morph, Pattern, Picture, Shapes, Snowflakes, Snowstorm, Text, Tree,
  Vertical Meter, Video, VU Meter, Wave, Waveform, Whirlpool.

This list includes both capability gaps and unfinished ports. It must not be
read as a claim that every omitted effect needs new runtime functionality.
Twinkle's exponential event intervals need `log` to port its distribution
directly; arbitrary Shockwave acceleration needs `pow`. Media/text/audio effects
need resource or analysis inputs, and LipSync/State need authored prop metadata.
Life, Fire, Liquid and particle effects need simulation or prepared data for
practical playback. Procedural subsets of Morph, Pattern, Shapes, Tree, Wave
and Whirlpool need further source/parameter mapping; they are not implemented
by a same-named substitute. Remaining modes of the included effects are also
outside this library's current contract, notably mark-driven/hierarchy modes,
accumulated speed, Bars zigzags/rotation, Border marquee, non-circular Shockwave,
non-cardinal Wipe, nonmonotonic Dissolve and arbitrary Chase movement.

The source formulas and defaults were compared while authoring. These ports are
experimental: compilation, project validity, renderer parity, split-output
equivalence and playback performance have not been validated.

## Shared contract differences

These differences apply even to effects whose main formula is procedural:

| Contract | Vixen | Current Donder DSL |
| --- | --- | --- |
| Pixel domain | `PixelEffectBase` constructs a virtual integer matrix from string/group structure or preview locations, with orientation, offsets and string lengths. | Pixel index/count, physical coordinates, target bounds and section queries; no Vixen matrix or original Vixen element hierarchy. |
| Pixel-effect clock | `BaseEffect.FrameTime` comes from the update interval. `GetNumberFrames()` truncates duration/frame interval; progress is frame time divided by duration. | Arbitrary sample time and progress; no frame interval/count builtin. A manually supplied interval would be an additional authoring parameter, not automatic compatibility. |
| Curves | Percentage positions and values; `Curve.GetValue` clamps to 0–100 and substitutes 100 for NaN. | Normalized positions, unrestricted scalar values, and NaN for an empty curve. Percentage values can be represented explicitly, but must not be mistaken for existing normalized starter curves. |
| Gradients | Includes alpha and interpolation behavior; `ColorGradient.Interpolate` truncates interpolated RGB channels. | RGB only; gradient channel interpolation rounds to nearest. A black-to-white midpoint is 127 in Vixen and 128 in Donder. |
| Lighting intensity | `LightingValue.FullColor` truncates channels after applying intensity. | Ordinary color multiplication rounds. An effect can explicitly calculate truncated channels using existing scalar arithmetic. |
| Parameter interface | Display labels, units, bounds, conditionally visible controls, resource defaults, and structured collections such as gradient/level pairs. | Named typed parameters and literal defaults; no equivalent complete metadata/structured-collection contract. Curve and gradient defaults cannot be declared as literals in an effect file. |
| Randomness | Renderers consume Vixen's random stream, often while creating or stepping objects. | Deterministic scalar random sampling; substituting it changes the generated realization. |
| Persistent state | Several renderers evolve grids, particles or accumulated motion across frames. | One `sample()` entrypoint, without persistent effect state or simulation-step entrypoints. |

Sources: [PixelEffectBase](https://github.com/VixenLights/Vixen/blob/3.13u2/src/Vixen.Modules/Effect/Effect/PixelEffectBase.cs),
[BaseEffect](https://github.com/VixenLights/Vixen/blob/3.13u2/src/Vixen.Modules/Effect/Effect/BaseEffect.cs),
[Curve](https://github.com/VixenLights/Vixen/blob/3.13u2/src/Vixen.Modules/App/Curves/Curve.cs),
[ColorGradient](https://github.com/VixenLights/Vixen/blob/3.13u2/src/Vixen.Modules/App/ColorGradients/ColorGradient.cs),
[LightingValue](https://github.com/VixenLights/Vixen/blob/3.13u2/src/Vixen.Core/Data/Value/LightingValue.cs).

## Basic effects

This is a contract inventory, not a list of completed ports. The limitations
below are additional to the shared differences above. "Needs" identifies a
capability required for the full contract, not authorization to add it.

| Effect | Behavior and additional requirements for a complete port |
| --- | --- |
| Alternating | Gradient/level-pair collections, time-interval or mark modes, static mode, interval skips, group level and effect depth. Needs original group and mark semantics. |
| Candle Flicker | Random frequency and brightness changes, deviation caps and group-level behavior. Needs matching random event generation. |
| Chase | Pulse curve, movement curve, overlap, color-handling modes, optional default level, leading/trailing extensions, depth and target selection. Needs original element hierarchy and lighting-intent semantics. |
| Dissolve | Fill/clear progression, randomized ordering/colors, paired direction modes, group/depth selection, and mark-driven variants. Needs those event and hierarchy semantics. |
| LipSync | Phonemes, face/image mappings, outline and eye controls, mark collections and gaps. Needs prop metadata, labeled marks and image resources. |
| Pulse | Level curve plus color gradient; RGB and discrete-color render paths. Needs gradient/intent compatibility and discrete-color prop information for the full contract. |
| Set Level | Color and intensity, including filtering for discrete-color elements. RGB intensity calculation is expressible; discrete-color filtering needs prop metadata. |
| Spin | Revolution count/frequency/time, pulse overlap/time/percentage, color modes, reverse, default level and depth. Needs hierarchy and pulse/intent behavior. |
| State | Named state definitions, individual state selection, mark/cycle playback and custom state items. Needs state-property metadata and labeled marks. |
| Strobe | Simple/advanced pulse modes, cycle time/variation, on-time and intensity curves, time or mark-label/duration sources, group level. Needs mark labels/durations and group context for all modes. |
| Twinkle | Coverage, pulse duration/variation, brightness variation, channel grouping and color modes. Needs matching random pulse scheduling and group semantics. |
| Wipe | Direction, offsets, movement, pulse timing, pass count, wipe on/off, color modes, depth and target selection. Needs Vixen spatial/group mapping and all movement rules; some calculations use square roots/powers. |

Each row refers to its same-named directory in the pinned effect source tree;
Candle Flicker is `Candle`, Set Level is `SetLevel`, and LipSync is `LipSync`.

## Pixel effects

The raster effects depend on the pixel-domain and clock contracts above;
audio-driven effects also have their own preprocessing and grouping contracts.
A mathematical effect is not necessarily a simulation, but it can still require
missing math, resource types or preparation data.

| Effect | Additional requirements or implementation boundary |
| --- | --- |
| Balls | Moving object state, bounce/wrap, random movement/size and collision/color changes. |
| Bars | Direction and alternate/compress/expand modes, gradient lists, highlight/3D, zigzag and rotated rendering; speed accumulation and square-root calculations in supported variants. |
| Border | Simple/advanced/marquee modes, independently animated edge geometry and rendering/color modes. |
| Butterfly | Five formulas, gradient/rainbow, direction, repeat, background chunks/skips, iterations or accumulated speed. Type 2 uses square root. |
| Circles | Animated circle objects, radial direction, fill/edge modes and offsets. |
| Colorwash | Center/outer/invert fades, shimmer, iterations or accumulated speed, gradient and intensity. Iterations alone does not implement the speed-mode contract. |
| Count Down | Text/fonts, time formatting and spinner variants; graphics and additional math. |
| Curtain | Direction/edge, swag geometry, positioning and per-iteration intensity. Needs the Vixen integer raster and motion behavior. |
| Fire | Random base heat row and row-to-row heat propagation. This is not Life's previous-generation update rule. Needs the raster generation and random sequence. |
| Fireworks | Particle bursts, lifetime/motion/fading, random/mark/audio triggers. Needs particle/event state and audio data for its audio modes. |
| Garlands | Integer row/spacing patterns, direction, iterations and accumulated speed. |
| Glediator | Recorded-frame file loading, frame selection and playback. |
| Life | Previous-generation grid, neighbor rules, initialization and growth types. |
| Liquid | LiquidFun physics, particle/emitters, barriers, warm-up, blending and optional audio/mark flow control. |
| Meteors | Particle motion, tails, direction/speed variation, explosions and ground interaction. |
| Morph | Structured polygon/path data, head/tail/fill geometry and rendering modes. |
| Pattern | Multiple pattern-specific structured controls, motion, weave/stripe geometry and additional math in variants. |
| Picture | Image/GIF resources, decoding, transforms, scaling and color operations. |
| Pinwheel | Polar radius/angle, twist, blade shading, motion, radius basis, offsets, gradient/level pairs and random color modes. See the detailed contract below. |
| Plasma | Square roots, trigonometric formulas, styles, density, presets and multi-gradient blending with Vixen channel rounding. |
| Shapes | Shape/image resources, drawing/rasterization, structured shape controls and additional math. |
| Shockwave | Radial distance, width/radius curves, edge blending and acceleration powers. |
| Snowflakes | Particle motion, shape/color variants and snow buildup. |
| Snowstorm | Persistent moving particles and trails. |
| Spiral | Integer strand placement, thickness/rotation, growth/shrink, gradient/3D and accumulated speed modes. |
| Spirograph | Parametric path rasterization, radii/range/distance animation and additional math. |
| Text | Strings/fonts, glyph rasterization, text modes and transforms. |
| Tree | Branch geometry, directional reveal, stateful color traversal and optional random twinkle. |
| Vertical Meter | Audio decoding, filtering, normalization, attack/decay, color thresholds and group depth. |
| Video | Video decoding, timing, scaling, transforms, fit-to-time and resource handling. |
| VU Meter | Audio analysis, filtering, normalization, attack/decay and color thresholds. |
| Wave | Structured waveform settings, motion/direction, mark modes, thickness/frequency/height and windowing. |
| Waveform | Audio analysis, filtering, normalization, attack/decay and scrolling. |
| Whirlpool | Raster border/path traversal, geometry and motion controls; inspect `Whirlpool` including its auxiliary data types. |

## Pinwheel contract

[Published controls](https://www.vixenlights.com/docs/usage/sequencer/effects/pixel/pinwheel/)
and the [3.13u2 implementation](https://github.com/VixenLights/Vixen/blob/3.13u2/src/Vixen.Modules/Effect/PinWheel/PinWheel.cs)
must be read together. Current defaults come from
[PinWheelData](https://github.com/VixenLights/Vixen/blob/3.13u2/src/Vixen.Modules/Effect/PinWheel/PinWheelData.cs).

| Control | Reference behavior/default |
| --- | --- |
| Arms | 8; editor range 1–20. Arm separation uses **integer** `360 / Arms`, not floating-point division. |
| Motion | Iterations by default; speed mode uses the Vixen frame count. |
| Movement | Speed curve flat at 5 on the 0–100 curve scale; mapped to 1–50 and multiplied by frame interval/50 ms, then clamped to at least 1. This is not a default of 5 revolutions. |
| Thickness | Curve flat at 15; rounded to an integer, minimum 1. Half-width is that percentage of half the arm separation. |
| Size | Curve flat at 23; mapped from 0–100 to 1–400 percent. |
| Twist | Curve flat at 56; mapped to -500–500 degrees, so the default is +60 degrees. Twist increases with radius/max-radius. |
| Rotation | Rotation curve flat at 100, mapped to -1–1; determines the direction and amount of rotation. |
| Offsets | X/Y curves flat at 50, meaning zero displacement. Percentage-offset mode is enabled. |
| Size basis | Largest virtual-buffer dimension by default; also height and width. |
| Center hub | Curve flat at 0; pixels at or inside the hub are excluded, including the exact center. |
| Blade | Flat, 3D, inverted 3D, Fan. Flat is the default. |
| Color | Standard, Random, Rainbow, Gradient. Default pairs are red/green/blue with flat-100 level curves. Gradient mode samples along radius; Standard samples across effect time. |
| Intensity | Flat-100 curve, applied alongside the per-color level curve. |
| Orientation | Vertical strings by default; location-based rendering also exists. |

The renderer computes Euclidean distance and an angle using `sqrt` and `atan2`
in `PixelEffectBase`. Donder exposes both operations as `sqrt(value)` and
`atan2(y, x)`, with the angle in radians. A location-based Pinwheel can compute
its output from the current pixel's position, target bounds, time and effect
parameters without sampling another pixel. Vixen's virtual-matrix construction
is a coordinate-mapping concern, not a dependency on neighboring pixel colors.
Exact parity still requires matching its coordinate quantization, timing,
color modes and numerical precision; the math builtins alone do not establish
that parity.

Supporting only straight flat blades, replacing the twist with another formula,
substituting a new random generator, or changing palette behavior must not be
presented as a complete Pinwheel port.

## Verification boundary

A Donder project check establishes DSL and document validity, not agreement
with Vixen. A complete port needs reference output comparisons at defaults and
non-default settings, across all modes, dimensions/orientations, effect starts
and ends, arbitrary seeks, and split controller outputs. Differences in numerical
precision, quantization or random realization must be reported, not hidden by
a visual-similarity claim.

Full Basic and Pixel parity is not available in the current DSL. An effect-only
library cannot close all of the gaps above. Unsupported effects must not be
added as empty declarations, renamed generic effects or silent substitutes.
