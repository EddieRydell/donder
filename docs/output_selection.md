# Preparing a sequence for selected outputs

`dawn_elaboration::PreparedSequenceOutput::prepare_selected` takes the project,
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

The returned `sequence` is the ordinary `dawn-runtime::sequence::PreparedSequence`.
Selection and compaction run entirely in elaboration. The runtime has no device
selection branches, alternate executor, or fragment type.

An empty selection produces no outputs or retained element/effect data. An
unpatched but valid port produces its normal zero-filled buffer. Duplicate ports
and ports outside the selected setup return explicit preparation errors. The
existing `prepare` method still prepares the complete setup, including its logical
preview elements.

## What gets retained

Preparation walks backward from selected patch sinks. Shared sources and filters
are retained once, and the existing patch lowering assigns buffer slots only to
the retained branches. Source cell spans determine the element cells to keep.
Their storage addresses become dense, including disjoint ranges of one element.

Reachable spatial signal queries widen that retained domain: local pixel queries
keep each selected color element in full, while global pixel queries keep the full
rig color domain. Unpatched pixels may therefore remain as signal dependencies.
Global indices do not change with controller selection. Packing still writes only
selected ports; retained upstream pixels increase evaluation and memory costs.

Effects targeting no retained cells are removed, as are effects in disabled or
unreachable layers. Referenced signal inputs remain connected, including temporal
operator inputs. An empty layer feeding an operator remains a valid input: an
operator can produce a nonblack result from black. Retained programs, targets,
automation slots, graph nodes, and frame/VM slots are compacted or replanned.
Control addresses and fixture rules are remapped to the retained element table.

Only pixel storage addresses change. Original effect/operator `pixel_index`,
`pixel_count`, and `pixel_fraction` values remain intact, so splitting a strip or
whole-target effect across devices preserves the appearance. Resources referenced
by retained code (curves, marks, gradients, target metadata) retain their contents;
their meaning cannot be changed merely because fewer output pixels are retained.

The host still elaborates the complete authored signal graph before compacting
it. Generators therefore see their original target and layout. This favors a
simple implementation and correct sampling semantics over host preparation time.

## Measured reduction

The starter has 30 ports of 113 RGB pixels. Selecting its first port gave:

| Layered starter data | Complete setup | One port |
| --- | ---: | ---: |
| Rendered pixels | 3,390 | 113 |
| Target pixel records | 10,170 | 339 |
| Effects | 36 | 7 |
| Bytecode programs | 1 | 1 |
| Patch steps | 90 | 3 |
| Graph color buffer bytes | 30,510 | 1,017 |
| Runtime workspace heap bytes on the laptop | 43,666 | 2,510 |

Workspace bytes measure allocator-requested live storage created by
`PreparedSequence::workspace`, excluding prepared data, caller output buffers,
allocator overhead, and stack storage. Desktop pointer sizes affect this figure;
it is not an ESP32 RAM measurement. Output bytes fall from 10,170 to 339 separately.
These are in-memory measurements. The selected layered starter now serializes to
7,856 bytes, including its header. See [ESP32 sequence loading](esp32_loading.md)
for the wire format and measured serial/Wi-Fi execution proof.

`output_selection` tests compare selected bytes with complete evaluation across
every starter port, reordered selections, nonsequential times, effect boundaries,
whole-target/per-fixture contexts, marks, temporal operators, split elements,
shared patch branches, multiple controllers, indexed controls, and fixture rules.
`controller_allocations` verifies prepared fragment frames do not allocate and
measures workspace storage.
