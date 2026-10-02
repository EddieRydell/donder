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
    let mut workspace = sequence.workspace()?;
    let mut buffers = sequence.outputs().iter()
        .map(|output| vec![0; output.width as usize])
        .collect::<Vec<_>>();
    sequence.evaluate(time, &mut buffers, &mut workspace)?;
}
```

`PreparedSequence` is owned by runtime and re-exported by elaboration. Its fields
are private; elaboration assembles it through `PreparedSequence::new`, and callers
inspect it through read-only accessors. Selection and compaction run entirely in
elaboration's private modules. The runtime has no device-selection branches,
alternate executor, or fragment type.

An empty selection produces no outputs or retained fixture/effect data. An
unpatched but valid port produces its normal zero-filled buffer. `None` means the
selection cannot be resolved: for example, a missing sequence, a controller or
port outside the active setup, or a sequence targeting another layout. Internal
preparation failures must not be disguised as an absent selection. Playback
workspace creation and evaluation still have their own `Result` contracts.

## What gets retained

Preparation retains LED routes for the selected ports. Their pixel spans
determine which fixture pixels to keep. Storage addresses become dense, including
disjoint wiring ranges of one instance. Packing uses those compact addresses
directly; there are no generic patch sources, filters, or intermediate values.

Reachable spatial signal queries widen that retained domain: local pixel queries
keep each selected fixture instance in full, while global pixel queries keep the full
rig color domain. Unpatched pixels may therefore remain as signal dependencies.
Global indices do not change with controller selection. Packing still writes only
selected ports; retained upstream pixels increase evaluation and memory costs.

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

The host still elaborates the complete authored signal graph before compacting
it. Generators therefore see their original target and layout. This favors a
simple implementation and correct sampling semantics over host preparation time.

## Measurement and checks

See [performance and hardware evidence](performance.md) for measurement and
retention policy. The starter has 30 instances of 113 pixels each;
selecting its first port retains 113 output pixels instead of 3,390. Spatial
operator dependencies can retain additional unpatched pixels.

`output_selection` tests compare selected bytes with complete evaluation across
every starter port, reordered selections, nonsequential times, effect boundaries,
whole-target/per-fixture contexts, marks, temporal operators, split fixtures,
mirrored routes, multiple controllers, and shared definitions.
`controller_allocations` verifies prepared fragment frames do not allocate and
measures workspace storage.
