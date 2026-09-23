"""Capture strict PC records; report sampled leaf symbols from the exact ELF.

Run in the exported ESP tool environment via `uvx --from esptool python`.
This reports interrupted instruction locations, not reconstructed call stacks.
"""

import argparse
import bisect
import collections
import contextlib
import pathlib
import subprocess
import time

import serial


def symbols(elf):
    output = subprocess.check_output(
        ["xtensa-esp32-elf-nm", "-n", "-S", "-C", str(elf)], text=True
    )
    result = []
    for line in output.splitlines():
        parts = line.split(maxsplit=3)
        if len(parts) == 4 and parts[2] in ("t", "T"):
            result.append((int(parts[0], 16), int(parts[1], 16), parts[3]))
    if not result:
        raise RuntimeError("ELF has no text symbols")
    return sorted(result)


def report(pcs, table, elf):
    starts = [row[0] for row in table]
    counts = collections.Counter()
    for pc in pcs:
        index = bisect.bisect_right(starts, pc) - 1
        if index >= 0 and pc < table[index][0] + table[index][1]:
            name = table[index][2]
        else:
            name = f"unresolved@{pc:08x}"
        counts[name] += 1
    for name, count in counts.most_common(20):
        print(f"SYMBOL {count / len(pcs):.2%} samples={count} {name}", flush=True)

    # Resolve unique PCs in one process, outside device timing. DWARF identifies
    # inlined leaf functions that the linker symbol table folds into a large caller.
    unique = sorted(set(pcs))
    lines = subprocess.run(
        ["xtensa-esp32-elf-addr2line", "-a", "-f", "-C", "-e", str(elf)],
        input="".join(f"0x{pc:x}\n" for pc in unique),
        text=True, capture_output=True, check=True,
    ).stdout.splitlines()
    if len(lines) != len(unique) * 3:
        raise RuntimeError("Incomplete source attribution")
    locations = {}
    for index, pc in enumerate(unique):
        address, function, location = lines[index * 3:index * 3 + 3]
        if int(address, 16) != pc:
            raise RuntimeError("Source attribution address mismatch")
        locations[pc] = (function, location)
    for (function, location), count in collections.Counter(locations[pc] for pc in pcs).most_common(20):
        print(f"SOURCE {count / len(pcs):.2%} samples={count} {function} at {location}", flush=True)


def capture(elf, raw=None):
    table = symbols(elf)
    import hashlib

    identity = f"ELF sha256={hashlib.sha256(elf.read_bytes()).hexdigest()}"
    print(identity, flush=True)
    if raw is not None:
        print(identity, file=raw, flush=True)
    with serial.Serial(port=None, baudrate=115200, timeout=1) as port:
        port.port = "COM4"
        port.dtr = False
        port.rts = False
        port.open()
        # Leave room for host scheduling stalls while the board dumps its samples.
        port.set_buffer_size(rx_size=65536)
        port.rts = True
        time.sleep(0.1)
        port.reset_input_buffer()
        port.rts = False
        started = False
        pending = bytearray()
        cases = []
        profiles = []
        pcs = []
        expected = 0
        deadline = time.monotonic() + 600
        while time.monotonic() < deadline:
            pending.extend(port.read_until(b"\n"))
            if not pending.endswith(b"\n"):
                continue
            line = pending.decode("ascii", errors="strict").strip()
            pending.clear()
            if line.startswith("PROFILE PANIC:"):
                raise RuntimeError(line)
            if line.startswith("DONDER PC BEGIN"):
                if started:
                    raise RuntimeError("Board restarted")
                started = True
            if not started:
                continue
            if raw is not None:
                print(line, file=raw, flush=True)
            if not line.startswith("PC ") or line.startswith("PC CASE "):
                print(line, flush=True)
            if line.startswith("PC CASE ") or line == "DONDER PC END":
                if len(pcs) != expected:
                    raise RuntimeError("Missing PC samples")
                if pcs:
                    profiles.append((*cases[-1], pcs))
                pcs = []
                if line == "DONDER PC END":
                    if len(cases) != 84 or len({name for name, _ in cases}) != 21:
                        raise RuntimeError("Incomplete fixture coverage")
                    for index in range(0, len(cases), 4):
                        group = cases[index:index + 4]
                        if len({name for name, _ in group}) != 1 or [p for _, p in group] != [0, 997, 1999, 0]:
                            raise RuntimeError("Incorrect profiling controls")
                    # Symbol resolution can take seconds. Never stop draining UART
                    # for host-side analysis while the device is still sending.
                    for name, period, samples in profiles:
                        print(f"PROFILE effect={name} period_us={period}", flush=True)
                        report(samples, table, elf)
                    return
                fields = dict(part.split("=", 1) for part in line.split()[2:])
                period = int(fields["period_us"])
                expected = int(fields["samples"])
                frames = int(fields["frames"])
                if frames <= 0 or frames % 32 != 0 or int(fields["elapsed_us"]) < 2_000_000:
                    raise RuntimeError("Invalid measurement window")
                if (period == 0 and expected != 0) or (period != 0 and not 1 <= expected <= 4096):
                    raise RuntimeError("Invalid sample count")
                cases.append((fields["effect"], period))
            elif line.startswith("PC "):
                pcs.append(int(line[3:], 16))
                if len(pcs) > expected:
                    raise RuntimeError("Excess PC samples")
            elif not line.startswith("DONDER PC BEGIN"):
                raise RuntimeError(f"Unexpected record: {line!r}")
        raise TimeoutError("PC profiling did not finish")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("elf", type=pathlib.Path)
    parser.add_argument("--raw-output", type=pathlib.Path)
    args = parser.parse_args()
    with (args.raw_output.open("x", encoding="utf-8") if args.raw_output else contextlib.nullcontext()) as raw:
        capture(args.elf, raw)
