# Effect compiler

How [effect language](effect_language.md) source becomes the programs that
playback runs. Every stage below lives in `donder-language/src/compiler` except the
global signal graph, which preparation builds in `donder-elaboration`.

```text
source ─► syntax (lexer, parser, AST)
       ─► check: names and types, building definition IR          check.rs
       ─► instance: bound values, decided control flow             instance.rs
       ─► global signal graph: black inputs, fusion                donder-elaboration
       ─► prepare: fixed values become parameter slots             lower/mod.rs
       ─► schedule: stages and regions                             lower/schedule.rs
       ─► emit: strip bytecode                                    lower/emit.rs
```

The IR is the center: a typed, hash-consed, pure dataflow graph whose
dependencies stay visible until bytecode emission. Domains, reductions, signal
samples, uses and purity are structural facts of the graph, not results of an
analysis over instructions.

## Syntax and checking

`syntax` holds the lexer, the recursive-descent parser and an untyped AST with
byte spans. A syntax error abandons its declaration and parsing resumes at the
next, so one source can report several errors. `param`, `input` and `sample`
are reserved only where a declaration member starts.

`check` resolves names and types and builds the definition IR directly; every
diagnostic points at the AST span that produced it. Integers widen to floats in
arithmetic, arguments, annotated lets and `if` branches. A guard or `if` arm in
a skippable position carries an *outcome*: whether it holds a value, and the
value. A skipped branch has no value at all, so a reduction filter reads the
guarded value without a redundant choice, and a `sample` block chooses black.
Array literals exist only while they are indexed or measured.

Builtins resolve through one table, `builtins.rs`, which also generates the
[builtin reference](effect_builtins.md). Builtins that keep ints as ints, such
as `min` and `clamp`, choose int operations when every argument is an int; a few
(`round`, `fract`, `sign`, `step`) are built from other operations rather than
having instructions of their own.

User functions have no IR or bytecode of their own. A call is checked by binding
the function's arguments, checked in the caller, as the body's only names and
building the body's IR in place, so equal calls share nodes by hash-consing and
bounds are proven per call. Every function body is also checked once on its own
with arguments of unknown value, skipping the bound proofs that need values, so
an unused function cannot hide errors. Recursion is reported when a function
would inline itself.

Reduction bounds are proved by interval analysis (`ir/interval.rs`) over
literals, declared ranges and array lengths. A bound that depends on a length is
kept and rechecked by `check_values` when an instance supplies its values.

## IR

One arena per graph (`ir/mod.rs`). Building an operation that already exists
returns the existing node; commutative operations order their operands first,
so equal expressions share a node by construction.

| Operation | Meaning |
| --- | --- |
| `Constant`, `Param`, `Context` | Leaves; constants carry their type |
| `Unary`, `Binary`, `Ternary` | Scalar, color, resource, mark and section operations |
| `Select` | Pure choice. `&&` and `\|\|` are selects too |
| `Reduce` with `LoopIndex` | A reduction; its bounds, body, filter and default live in its loop |
| `Sample` | An input signal of this frame, or the node's previous frame (`previous`), at the current pixel, a shift of it, or a pixel of its fixture |
| `Items`, `Pick` | An array literal, and its item at a clamped index |

Each node records its type, its domain and the reductions whose index it reads,
computed when it is built:

| Domain bit | Varies with |
| --- | --- |
| `PARAM` | the instance: fixed parameters, clip duration, uniform pixel count |
| `TIME` | the query: `time`, `progress`, `frames_elapsed`, automated parameters |
| `TARGET` | the target run: pixel count of mixed fixtures and target bounds |
| `PIXEL` | the pixel |
| `SIGNAL` | an upstream sample |

A node's domain is the union of its operands' domains. A loop index has the
domain of its bounds; a reduction has its parts' domains minus its own loop.

Construction folds (`ir/fold.rs`):
- constants, by the host evaluator (`ir/eval.rs`), which uses the runtime's math
  from `crate::sampling`. Transcendental functions, context, sections and
  samples are never evaluated on the host;
- identities that cannot hide a missing value: `x + 0`, `x * 1`, black under
  color max, add or scale, and choices with equal or constant arms;
- black from a color multiplied by black, scaled by zero, or an `hsv` with zero
  value; colors have no missing value to hide;
- division by a constant into multiplication by its reciprocal when that
  reciprocal is normal;
- a choice on `a && b` or `a || b` becomes nested choices on `a` and then `b`, so
  the second test is evaluated only when it decides;
- reductions over an empty range, with an identity body, or with a body that
  does not read the index.

`ir/rebuild.rs` copies a graph into another with leaves replaced; building in
the target refolds every node. Instantiation, black inputs, fusion and slot
preparation are all rebuilds.

## Instances

An `Invocation` binds a definition to validated values and automation. An
`Instance` places it in a sequence: automated parameters become `TIME` leaves,
fixed ones `PARAM` leaves, and the clip duration and a uniform pixel count
become extra `PARAM` slots. A choice whose condition is fixed for the instance,
but whose result still varies at playback, is decided and only its taken arm is
kept. A choice fixed as a whole is left for preparation to evaluate, so it does
not multiply programs. Fixed reduction bounds become constants.

A rig's invocations (`Definition::invoke_live`) also name *live* marks
parameters: collections that playback rebinds as marks arrive. They are `TIME`
leaves like automated parameters, so nothing reads their contents or length at
preparation; a reduction bounded by a live collection's length is rejected when
the invocation is made. `Instance::sample_live` reports each live parameter's
slot in the lowered program, and an instance with live parameters never fuses.
A rig's controls are ordinary automation bindings whose value playback sets.

## Global signal graph

Preparation (`donder-elaboration/src/sequence/composition.rs`) builds one graph
per sequence selection from layers, operator instances and the output. Layers
stay runtime nodes: the active clip set changes with time.

1. **Black signals.** A disabled or empty layer is black. Sampling a black input
   folds to black (`Instance::with_black_input`), and an operator that becomes
   constant black is dropped from its consumers and the output.
2. **Fusion.** An operator consumed once is substituted into its consumer
   (`Instance::fuse_input`) at its only sample site, on the current pixel, and
   runs at the consumer's time. A site a scan reads, a program with live marks,
   and a source or consumer that reads `previous` keep the boundary, since a
   node's previous frame is its own. A fused program beyond the row or nesting
   limits keeps the boundary too.
3. Every remaining instance is lowered and equal programs are interned.

## Preparation

`lower::prepare` evaluates every node fixed for the instance. The fixed nodes
that a playback-time node reads, the *frontier*, become parameter slots holding
their values, after the automated parameters. Division by a fixed value
multiplies by a prepared reciprocal slot. Nothing else fixed survives, so
playback never recomputes per-instance constants and differently configured
instances still share one program.

## Scheduling

`lower::schedule` is global code motion over a region tree.

- **Stages.** A node's domain decides its outermost stage: once per query, once
  per target run, or per strip. Pixel context, section queries, samples and
  reductions, and their users, run in the body; everything else that is fixed
  for a query or a target shape runs before the strips.
- **Regions.** In the body, branch arms and reduction iterations are regions; a
  filtered reduction's contribution is a region inside its iteration. Each node
  is placed where its uses meet, then hoisted out of every reduction whose index
  it does not read.
- **Choices.** A select on a uniform condition branches whenever an arm has
  exclusive work, since only the taken arm runs. On a per-pixel condition, arms
  with at most two exclusive nodes, and no reduction or sample, become a
  branch-free choice; heavier arms branch and keep their work inside them.

The plan (stages, regions, branches, uses) is independent of the bytecode;
`Instance::explain` prints it with domains next to the emitted program.

## Bytecode

`lower::emit` targets the strip interpreter in `donder-runtime`
(`donder-runtime-types/src/bytecode.rs` defines the format):
- Every instruction is one 8-byte word (`word.rs`): an opcode, a byte of
  small enum operands, then its slots, one byte each, and its constants and
  lengths in a layout its opcode fixes. A reduction and a stencil continue into
  a second word; lengths of nested code and block ends count words. A slot
  byte names a scalar (0 to 126), a row (up to 124) or a pixel input, so a
  program holds at most 127 scalars and 124 rows of each bank.
- A color is three floats: a color operand names the first of three
  consecutive float slots, red, green and blue. Only instructions that produce
  or consume a whole color (`hsv`, recoloring, hue, saturation and intensity,
  gradients, samples, stencils, scans and color array items) name one. Float
  arithmetic and mixes mark which operands are colors, read channel by
  channel while a float operand serves every channel, so `c * level` is one
  instruction; choices and copies take a color bank. A channel read is the
  channel's own slot.
- A float, int or bool constant is an instruction's 32-bit immediate; a
  resource constant indexes the program's resource list. Instructions the
  word set leaves out are lowered: `power` with an int count raises to its
  float, `!` compares with false, and a section position is float arithmetic
  on the pixel index.
- A program runs over strips of up to 128 pixels. Its query block runs once per
  program and query, its target block again whenever the target's pixel count or
  bounds change, and its body once per strip.
- A node that varies by pixel or signal is a row of the strip; every other node
  is a scalar. Pixel context is read straight from the strip's inputs.
  Constants, and resource and enum parameters, load once before the query block.
- Control flow is structured. A branch is an instruction followed by its arms:
  on a scalar condition one arm runs, on a row condition each arm runs for its
  own pixels. A reduction is an instruction followed by its loop and
  contribution parts, with an accumulator; `first` and `last` stop for each
  pixel that decides, and `last` counts down. Over rows, `any` and `all`
  combine every iteration branch-free and stop once every pixel has decided.
- A program declares its parameters' types. An invocation binds them to two
  storages, each numbered in declaration order: one 32-bit word per float,
  int, bool, color or enum parameter (a color's word indexes the image's
  colors), and one shared resource per curve,
  gradient, marks or array parameter. Equal resources bound anywhere in a
  sequence share one allocation; a marks value is a window of its
  collection's shared track.
- Enum values are indices into the program's names, which list every option of
  its parameters' and values' enums; an enum parameter is bound as its index
  and read as an int. Resources are references to parameters, constants or
  array items, never copies.
- Single-use patterns become fused instructions: scaled gradient samples, and
  hue replacements, `hsv(h, saturation(c), intensity(c))`
  or the same with `hue(c) + t`, which compute the color's components once.
- A reduction whose bounds vary by pixel but stay within a strip's width of
  indices, as the instance's values prove, shares one index: it runs the
  union of the pixels' ranges, each pixel taking part in its own, so work that
  depends only on the index (a kernel weight) runs once per iteration.
- `input.at(pixel.index + offset)` is a shifted read, and an `around`
  neighbor is one with the reduction's edges.
- An `around` reduction that reads its neighbor is a stencil. Checking proves
  its offsets span at most a strip (reaching at most half a strip each way when
  extended or mirrored) and splits its contribution: the neighbor, scaled by
  factors and filtered by guards that each depend on one of the neighbor and
  query values (a source weight), the offset and strip values (an offset
  scale), or neither (a pixel factor); anything else is an error. Playback
  reads the strip's neighborhood of the input once, computes the source weight
  once per neighbor rather than once per offset that reads it, runs the offset
  code once per offset, and accumulates every offset in one instruction. With
  skipped edges a neighbor counts where it is in the pixel's own fixture: the
  neighborhood and the strip are runs of consecutive fixture pixels, and each
  offset combines whole runs without testing pixels. A failed source guard is
  a zero weight and contributes nothing. Factor products are reassociated,
  which may change rounding. A strip whose whole neighborhood is black skips
  the offsets. An `around` that ignores its neighbor is an ordinary reduction
  over the offsets that have one.
- Work inside a reduction that depends only on its index and on query or
  target values is the same for every strip, so it becomes a table: a
  `Tabulate` in the query or target block runs it once per index into a row,
  and the loop reads it with `Lookup`; a stencil whose offset scale is a table
  reads it per offset and runs no offset code. A reduction qualifies when its
  bounds are constants spanning at most a strip, or it is a stencil, and the
  work is at least two instructions.
- A sum of such work over the offsets of an `around` that ignores its
  neighbor, `[max(start, -pixel), min(end, count - pixel))`, is a difference
  of two entries of a table of prefix sums: a renormalized kernel's weight
  total costs a few instructions per pixel whatever its radius. Float prefix
  sums round differently from summing in order.
- A filtered reduction with no code behind its filter combines under the
  filter instead of narrowing its selection every iteration.
- A scan is one instruction with a source block, as a stencil's, computing the
  light's weight. It runs forward or backward along each fixture. Playback
  computes the scan over the whole input frame the first time the program reads
  it in a frame, into a scan frame of its own (at most 8 per program); each
  strip copies its pixels from there.
- A sample reads the whole frame its input already wrote this frame, or the
  node's state buffer for `previous`.
- Every value starts in its own slot; liveness over the structured code then
  shares slots per bank and kind. A value read inside a reduction but defined
  before it lives through the whole loop. A value of the query or target block
  read after its block lives to the end and is never shared, since its block
  does not run again before every read; one read only within its block frees
  its slot there. A color's three slots are allocated together.

Programs are trusted: only the compiler writes them, and neither playback nor
the host checks them again. A definition's most general instance, with every
parameter left to playback, must fit the slot, row and nesting limits; that is
checked at compilation.

## Interpreter

`donder-runtime/src/dsl/vm/strip.rs` runs a program over one strip. Each bank
stores its scalars and then its rows in one array, and every operand is a
row-sized window of it with a mask: a scalar's window starts at the scalar and
masks every pixel to it, a row's masks nothing. Reading a pixel is one
branch-free load whatever the operand's kind, so one loop serves every
operation; the cheapest operations also have loops of their own for rows, and
for a row with a scalar. The selected pixels are ascending ranges: a whole strip
is one range, so dense code runs one plain loop, and a branch partitions the
ranges by its condition. Costly per-pixel work (color-space conversion, curve,
gradient and mark queries) is called, not inlined, so the interpreter fits the
ESP32's instruction RAM.
