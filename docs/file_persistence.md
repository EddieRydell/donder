# File persistence

Donder uses `donder_package::atomic_write` for package metadata, authored project
sources, path-refactor and rollback writes, CLI release artifacts, and desktop
preferences. It writes and syncs a complete temporary file beside the
destination before replacing that destination. This prevents a failed content
write from truncating one file; it is not a crash-atomic multi-file transaction
and does not promise synchronized directory entries.

The typed `DonderProject` is authoritative after loading. `SourceProject` records
document ownership, imports, original non-YAML DSL source, and referenced
assets. Saving derives canonical YAML from typed state. It does not mutate an
editable YAML model, preserve incidental formatting, or reload text to validate
a GUI edit.

Project workflows own ordering, external-edit preconditions, rollback, and
publication into desktop state. A completed rename publishes its new paths even
if a later source-buffer or preference refresh fails. A failed staging restore
retains the original payload and reports its location. Selected render identity
uses the same document-path remapping as the typed project.

Desktop edits create one candidate session, publish only an accepted snapshot,
and schedule save work through the single latest-request scheduler in
`state_tasks`. Tests of desktop persistence use the production `save_all`
barrier before reloading. Do not add retrying direct-write fallbacks or a second
atomic-write implementation to conceal ordering or handle-lifetime defects.

Desktop state accepts only the current format marker. There is no historical
file discovery, migration, field alias, or missing-field fallback. Curve points
and gradient stops use their shared Serde schemas, while
`serde_path_to_error` attaches field paths to the existing YAML source index.
Domain validation, reference linking, and document ownership remain with their
owning modules.
