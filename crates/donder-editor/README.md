# Shared Donder editor

Typed GUI projection, edits, selection, clipboard and model conversion shared by
the desktop and browser hosts, and in `dto` the editor contract exported to
TypeScript. Hosts own transactions, history, playback refresh and platform IO
and do not duplicate this authoring logic. See
[architecture](../../docs/architecture.md).
