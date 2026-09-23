# Preparing a sequence for selected outputs

`donder_elaboration::PreparedSequenceOutput::prepare_selected` takes the project,
setup, sequence, and a slice of `(ControllerId, ControllerPortId)` pairs. Output
buffers follow that slice's order. Select every port of a controller to prepare
its independent playback fragment:

```rust,ignore
let outputs = project.controllers[controller_id]
    .ports
    .iter()
    .map(|port| (controller_id.clone(), port.id))
    .collect::<Vec<_>>();
let prepared = PreparedSequenceOutput::prepare_selected(
    &project, setup_id, sequence_id, &outputs,
)?;
let sequence = prepared.sequence;
let mut workspace = sequence.workspace();
let mut buffers = sequence.output_widths.iter()
    .map(|&width| vec![0; width as usize])
    .collect::<Vec<_>>();
sequence.evaluate(time, &mut buffers, &mut workspace)?;
```

The returned `sequence` is the ordinary `donder-runtime::sequence::PreparedSequence`.
Selection and compaction run entirely in elaboration. The runtime has no device
selection branches, alternate executor, or fragment type.

An empty selection produces no outputs or retained fixture/effect data. An
unpatched but valid port produces its normal zero-filled buffer. Duplicate ports
and ports outside the selected setup return explicit preparation errors. The
existing `prepare` method still prepares the complete setup, including its logical
preview fixtures.

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
