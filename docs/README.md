# Documentation

These pages are for people working on Donder and for anyone who wants a quick
picture of how it works. They describe current behavior; plans, journals and
superseded measurements don't belong here.

**How it fits together**
- [Architecture](architecture.md): the pipeline from documents to LED frames,
  crate ownership, editing and playback.
- [Project format](project_format.md): documents, imports, ownership, sequences
  (layers, composition graph, automation clips), validity and the save contract.
- [Effect language](effect_language.md): declarations, builtins, numeric rules,
  limits and the standard effect and operator libraries.
- [Effect compiler](effect_compiler.md): source to dataflow IR, the global
  signal graph, scheduling and bytecode.
- [Output selection](output_selection.md): preparing a sequence for chosen
  controllers and ports.

**Hardware and performance**
- [ESP32 controllers](esp32_loading.md): install, upload, admission, editor
  playback, clock sync and output pins.
- [Performance](performance.md): compiler, preparation and interpreter design,
  timings and memory placement.

**Working on it**
- [Testing and benchmarks](testing.md): the gate, where tests live, key contracts
  and Criterion workflows.
- [Firmware](../firmware/esp32/README.md): toolchain, builds, flashing and device
  measurement.

**Authoring references**
- [Create your first LED show](first_show.md): a two-pixel project from scratch.
- [Fixture authoring](fixture_authoring.md): shapes, layouts, groups and routing.
- [Vixen effects](vixen_effects.md): what the Vixen ports cover and where they
  differ.
