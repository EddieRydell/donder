# Preparing a sequence for selected outputs

Elaboration has one public preparation function, declared in its `lib.rs`:

```rust,ignore
prepare(&DonderProject, &SequenceId, PrepareOutputs) -> Option<PreparedSequence>
```

The project supplies its active setup. `PrepareOutputs::All` retains every output
and logical preview fixture; `Controllers(&[ControllerId])` selects all ports of
the requested controllers; `Ports(&[(ControllerId, ControllerPortId)])` selects
individual ports. Explicit lists preserve first-occurrence order and ignore
duplicates. Select a controller's independent playback fragment with:

```rust,ignore
use donder_elaboration::{PrepareOutputs, prepare};

if let Some(sequence) = prepare(
    &project,
    &sequence_id,
    PrepareOutputs::Controllers(&[controller_id]),
) {
    let mut playback = sequence.into_playback();
    let frame = playback.evaluate(time);
    for output in frame.outputs() {
        // Send output.bytes to output.output's controller and port.
    }
}
```

`PreparedSequence` is owned by runtime and re-exported by elaboration. Its fields
are private; elaboration assembles it through `PreparedSequence::build`, and callers
inspect it through read-only accessors. Cloning preserves the accepted sequence;
`effect_windows()` exposes only retained start/duration metadata.
Neither operation exposes execution-plan or storage addresses.
The builder issues owner-bound handles for
fixtures, targets, effects and signal nodes; callers cannot supply mismatched graph
indices. Project admission binds typed programs, parameter values, automation,
timing and geometry before preparation.

Elaboration resolves authored output selections and retained pixel domains before
calling the runtime-owned builder. The builder assigns dense storage from those
domains; per-frame evaluation has no device-selection branches, alternate executor,
or fragment type. `SequencePlayback` owns the prepared
image and its playback buffer together. `evaluate` returns borrowed
frame views directly, so the caller cannot mismatch a sequence and workspace.

An empty selection produces no outputs or retained fixture/effect data. An
unpatched but valid port produces its normal zero-filled buffer. `None` means the
selection cannot be resolved: for example, a missing sequence, a controller or
port outside the active setup, or a sequence targeting another layout. Internal
preparation failures must not be disguised as an absent selection. Accepted
preparation and playback do not return ordinary errors. Text loading and checked
GUI edits remain validation boundaries. Image admission checks integrity, every
cross-reference and every program, so an admitted image cannot index outside
its records. Memory exhaustion and other system
resource failures are outside this guarantee.

## Boundaries and ownership

Elaboration and runtime declare their public facades in `lib.rs`; implementation
modules are not alternate entry points. The boundaries have different contracts:

| Boundary | Input and output | Checks belong here |
| --- | --- | --- |
| Project IO | Text and source ownership to an accepted `DonderProject` | Syntax, references, authored values and compiled definitions |
| Project editing | A typed edit batch applied to an accepted project | Affected relationships and playback inputs; rejection leaves the accepted project unchanged |
| Elaboration | Accepted project plus selection to `Option<PreparedSequence>` | Resolving the selection, not repeating text validation |
| Runtime construction | Accepted inputs and builder-issued handles to a private sequence | Handles preserve graph/storage ownership |
| Playback | Owned sequence plus sample time to borrowed frame views | No ordinary failure; scratch storage cannot be paired with another sequence |
| Image admission | Image bytes to a borrowed `Show` | Header/version, checksum, section table, record cross-references, program well-formedness, and limits on pixels, graph nodes and buffer bytes |

Image records are runtime-private: callers build them only through the builder
and read them through accessors. Image bytes, pixels, graph nodes and the exact
playback buffer size are independent limits.

Bytecode instructions, slots, validated programs and parameter bindings belong
to `donder-runtime-types`, which the compiler targets and the runtime and
firmware consume. Neither the language nor the model depends on the runtime.
Raw bytecode cannot execute directly. Parameter binding pairs values with their
admitted program before execution; evaluation does not accept an independently
replaceable parameter bank.

Authored parameter declarations, defaults, names, compiled effect/operator
declarations, and layout geometry units belong to `donder-language` and
`donder-model`. Runtime
stores executable programs and positional parameter schemas, not source-level
declarations. Each authored clip references one sample effect; mark-triggered
effects query marks and curve crossings inside that same sample program.

Runtime's `lib.rs` is its only public facade. It exposes sequence playback and
read-only frames/metadata, prepared clip sampling, the owner-bound sequence
constructor, and image encoding and admission. It does not re-export language types
or expose the VM, raw instruction execution, binding caches, registers, signal
providers or caller-managed execution workspaces. Mathematical sampling primitives
shared with authoring live in `donder-runtime-types`; VM instruction execution
stays in runtime.
Image admission stays with the private record layouts, separate from project IO,
network transport and device storage. Admission is fallible for format,
corruption, consistency and limit errors; playback is infallible.

In elaboration, `selection.rs` resolves identities and output ordering, while
`sequence.rs` lowers accepted fixtures and effects. Its `composition`
and `routing` modules connect signals and physical outputs. Its `retention` module
keeps pixels needed by selected routes and reachable spatial signal queries.
Language-owned fixture geometry records original positions together with retained
cells; selection cannot detach storage from its sampling domain. Runtime's builder
owns numeric graph addresses and the evaluation storage plan, not output selection.
These details are internal rather than independently callable preparation stages.

## What gets retained

Preparation retains LED routes for the selected ports. Their pixel spans
determine which fixture pixels to keep. Storage addresses become dense, including
disjoint wiring ranges of one instance. Packing uses those compact addresses
directly; there are no generic patch sources, filters, or intermediate values.

Reachable spatial signal queries widen that retained domain: reads of other
pixels of a fixture (by index, by offset, or along it in a scan) keep each
selected fixture instance in full. Unpatched pixels of those instances may
therefore remain as signal dependencies. Packing still writes only selected
ports; retained upstream pixels increase evaluation and memory costs.

Effects targeting no retained pixels are removed, as are effects in disabled or
unreachable layers. Referenced signal inputs remain connected, including temporal
operator inputs. An empty layer feeding an operator remains a valid input: an
operator can produce a nonblack result from black. Retained programs, targets,
automation slots, graph nodes, and frame/VM slots are compacted or replanned.

Only pixel storage addresses change. Original effect/operator `pixel_index`,
`pixel_count`, and `pixel_fraction` values remain intact, so splitting a strip or
whole-target effect across devices preserves the appearance. Resources referenced
by retained code (curves, marks, gradients, target metadata) retain their contents;
their meaning cannot be changed merely because fewer output pixels are retained.

The host resolves retention against the complete authored graph and original
geometry before assembling the selected graph. Sample contexts and section
membership retain their original target and layout coordinates.

## Measurement and checks

See [performance and hardware evidence](performance.md) for measurement
practice. The starter has 30 instances of 113 pixels each;
selecting its first port retains 113 output pixels instead of 3,390. Spatial
operator dependencies can retain additional unpatched pixels.

`output_selection` tests compare selected bytes with complete evaluation across
every starter port, reordered selections, nonsequential times, effect boundaries,
whole-target/per-fixture contexts, marks, temporal operators, split fixtures,
mirrored routes, multiple controllers, and shared definitions.
`controller_allocations` verifies prepared fragment frames do not allocate and
measures workspace storage.
