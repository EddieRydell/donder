"""Reset COM4 into the profiling application and capture one complete run."""

import argparse
import contextlib
import hashlib
import pathlib
import time

import serial


parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument("elf", type=pathlib.Path)
parser.add_argument("--raw-output", type=pathlib.Path)
args = parser.parse_args()
identity = f"ELF sha256={hashlib.sha256(args.elf.read_bytes()).hexdigest()}"

with (
    args.raw_output.open("x", encoding="ascii")
    if args.raw_output is not None else contextlib.nullcontext()
) as raw, serial.Serial(port=None, baudrate=115200, timeout=1) as port:
    print(identity, flush=True)
    if raw is not None:
        print(identity, file=raw, flush=True)
    port.port = "COM4"
    port.dtr = False
    port.rts = False
    port.open()
    port.rts = True
    time.sleep(0.1)
    port.reset_input_buffer()
    port.rts = False
    deadline = time.monotonic() + 600
    started = False
    pending = bytearray()
    measurements = set()
    stage = None
    while time.monotonic() < deadline:
        pending.extend(port.read_until(b"\n"))
        if not pending.endswith(b"\n"):
            continue
        try:
            line = bytes(pending).decode("ascii", errors="strict").strip()
        except UnicodeDecodeError as error:
            raise RuntimeError(f"Corrupt serial line: {bytes(pending)!r}") from error
        pending.clear()
        if line.startswith("PROFILE PANIC:"):
            raise RuntimeError(line)
        if line.startswith("DAWN PROFILE BEGIN"):
            if started:
                raise RuntimeError("Board restarted during profiling")
            started = True
        if not started:
            continue
        print(line, flush=True)
        if raw is not None:
            print(line, file=raw, flush=True)
        if line.startswith("stage="):
            if stage is not None:
                raise RuntimeError("Measurement missing its first-frame result")
            fields = dict(field.split("=", 1) for field in line.split())
            key = (fields["stage"], fields["effect"], int(fields["pixels"]))
            if key in measurements:
                raise RuntimeError(f"Duplicate measurement: {key}")
            measurements.add(key)
            stage = fields["stage"]
            if int(fields["mismatched_frames"]) != 0:
                raise RuntimeError(f"Host checksum mismatch: {key}")
            if int(fields["alloc_calls"]) != 0:
                raise RuntimeError(f"Timed allocation: {key}")
        elif line.startswith("first_frame_us="):
            fields = dict(field.split("=", 1) for field in line.split())
            if stage is None:
                raise RuntimeError("First-frame result without a measurement")
            if stage != "vm" and int(fields["first_alloc_calls"]) != 0:
                raise RuntimeError(f"Prepared first-frame allocation: {stage}")
            stage = None
        elif line.startswith("DAWN PROFILE END"):
            if len(measurements) != 174:
                raise RuntimeError(f"Incomplete run: {len(measurements)} measurements")
            if stage is not None or line != "DAWN PROFILE END heap_free=163840":
                raise RuntimeError(f"Incomplete result or unrecovered heap: {line}")
            break
        elif not line.startswith(("DAWN PROFILE BEGIN", "heap_total=", "first_frame_us=")):
            raise RuntimeError(f"Unexpected profiling output: {line!r}")
    else:
        raise TimeoutError("Profiling did not finish within 600 seconds")
