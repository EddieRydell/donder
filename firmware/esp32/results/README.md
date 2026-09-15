# ESP32 evidence

This directory retains only reviewed evidence that supports a documented claim.
Ordinary serial logs, failed runs, profiling captures, generated archives, and
checksum sidecars are ignored and should remain under `target` or another ignored
working directory.

The `accepted` directory contains the September 6, 2026 baseline:

- `2026-09-06-audit-i2s-verified.txt` records authenticated upload rejection
  checks, 200 matching frame checksums, and 13,080 continuous I2S playback frames
  with no missed deadline.
- The six `2026-09-06-audit-final-*.txt` files record controller-shaped fixture
  checks. Together they verify 576 requested frames with zero evaluation
  allocations.

Each capture identifies the firmware image and payload it measured. These files
do not prove the behavior or performance of later source. See
[`docs/performance.md`](../../../docs/performance.md) for the qualified summary
and measurement boundaries.

Promote a new capture only after the collector completes and every requested
hash/checksum check succeeds. Keep the smallest evidence needed for the claim,
replace superseded captures, and update the summary. Never make a malformed
capture appear valid by dropping or rewriting bytes.
