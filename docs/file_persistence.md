# File persistence

Dawn uses `dawn_package::atomic_write` for package metadata, authored project
sources, path-refactor writes and rollback writes, CLI release artifacts, and
desktop preferences.
The helper uses the existing `tempfile` dependency to write and sync a complete
temporary file beside the destination before replacing it. This prevents a
failed content write from truncating the destination. It does not promise a
crash-atomic multi-file transaction or synchronized directory entries.

Project operations still own their ordering, external-edit preconditions,
rollback, and publication into desktop state. A completed rename must publish
the new paths even if refreshing source buffers or saving view preferences
fails. Failed refreshes invalidate the loaded project; preference failures are
reported without undoing the committed project state. A failed staging restore
retains the original payload and reports its location. The selected render
target uses the same document-identity remapping as the typed project.

Desktop state decoding accepts only the current format marker; there is no
historical-file discovery or field migration. Curve points and gradient stops
use shared Serde schemas for loading and saving. `serde_path_to_error` supplies
structured field paths to the existing YAML source index. Domain validation,
reference linking, and document ownership retain their existing owners.

## Transaction dependency evaluation, 2026-09-07

Evaluated the published `fs-transaction` 0.2.1 crate, which offers ordered writes,
renames, expected-content checks, and journal recovery. It requires a single
writer and one filesystem per root. See its
[upstream documentation](https://docs.rs/fs-transaction/0.2.1/fs_transaction/).

The published archive was unpacked under `target/dependency-review`. An empty
workspace declaration was added only to that evaluation copy to isolate it
from Dawn's Cargo workspace. Production manifests and dependencies were not
changed. Its existing tests were run on Windows:

```text
cargo test --manifest-path target/dependency-review/fs-transaction-0.2.1/Cargo.toml
82 passed; 44 failed

cargo test --manifest-path target/dependency-review/fs-transaction-0.2.1/Cargo.toml fs::tests::sync_answers_both_strengths_on_files_and_directories -- --exact --nocapture
PermissionDenied: Access is denied (OS error 5), src/fs.rs:1503
```

The published implementation opens files with `File::open` before calling
`sync_all`; directory synchronization is skipped on non-Unix platforms. This
candidate does not meet Dawn's Windows requirement. Do not introduce an adapter
or fork to repair it as part of this consolidation.

`atomic-write-file` was also reviewed. It addresses single-file replacement,
which the existing `tempfile` helper already provides. Adding a second library
for the same primitive would preserve competing paths rather than simplify
them.

## Validation observation

Earlier gate runs hit intermittent Windows OS error 5 while the desktop
fixed-parameter test called raw `save_project` with a live desktop watcher. That
call bypassed the desktop save barrier. The test now exercises `save_all`, the
production desktop persistence entrypoint, before reloading and comparing
typed meaning. The specific conflicting OS handle was not captured. No retry
or direct-write fallback was added.

After the final changes, `cargo fmt` and the full `pnpm check` gate passed,
including binding generation, frontend checks, workspace tests, and Clippy.
This is automated verification; no interactive desktop or power-loss test was
performed.
