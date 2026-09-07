"""Collector contract tests; no board is opened or reset."""

import contextlib
import io
import pathlib
import subprocess
import unittest
from unittest.mock import patch

import capture_pc


class Port:
    def __init__(self, lines):
        self.lines = iter(lines)
        self.finished = False

    def __enter__(self):
        return self

    def __exit__(self, *args):
        pass

    def open(self):
        pass

    def set_buffer_size(self, rx_size):
        assert rx_size >= 4096 * len(b"PC 40000000\n")

    def reset_input_buffer(self):
        pass

    def read_until(self, delimiter):
        line = next(self.lines)
        self.finished = line == b"DAWN PC END\n"
        return line


def records():
    lines = [b"DAWN PC BEGIN\n"]
    for effect in range(21):
        for period in [0, 997, 1999, 0]:
            count = int(period != 0)
            lines.append(f"PC CASE effect=effect{effect} period_us={period} frames=128 elapsed_us=2000000 samples={count}\n".encode())
            if count:
                lines.append(b"PC 40000010\n")
    return lines + [b"DAWN PC END\n"]


class CollectorTests(unittest.TestCase):
    def collect(self, lines):
        output = io.StringIO()
        port = Port(lines)
        def resolve(*args, **kwargs):
            self.assertTrue(port.finished, "symbolization must not stall serial reception")
            return subprocess.CompletedProcess([], 0, "0x40000010\nsample_effect\neffect.rs:12\n")
        with patch.object(capture_pc.serial, "Serial", return_value=port), patch.object(capture_pc, "symbols", return_value=[(0x40000000, 256, "sample_effect")]), patch.object(capture_pc.time, "sleep"), contextlib.redirect_stdout(output):
            with patch.object(capture_pc.subprocess, "run", side_effect=resolve):
                capture_pc.capture(pathlib.Path(__file__))
        return output.getvalue()

    def test_complete_capture_and_symbolization(self):
        result = self.collect(records())
        self.assertEqual(result.count("SYMBOL 100.00% samples=1 sample_effect"), 42)
        self.assertEqual(result.count("SOURCE 100.00% samples=1 sample_effect at effect.rs:12"), 42)

    def test_source_attribution_rejects_wrong_addresses(self):
        with patch.object(capture_pc.subprocess, "run", return_value=subprocess.CompletedProcess([], 0, "0x40000020\nwrong\nwrong.rs:1\n")), contextlib.redirect_stdout(io.StringIO()):
            with self.assertRaisesRegex(RuntimeError, "address mismatch"):
                capture_pc.report([0x40000010], [], pathlib.Path(__file__))

    def test_missing_sample(self):
        lines = records()
        lines.remove(b"PC 40000010\n")
        with self.assertRaisesRegex(RuntimeError, "Missing PC"):
            self.collect(lines)

    def test_partial_show_cycle_is_rejected(self):
        lines = records()
        lines[1] = lines[1].replace(b"frames=128", b"frames=127")
        with self.assertRaisesRegex(RuntimeError, "Invalid measurement window"):
            self.collect(lines)

    def test_corruption_is_not_silently_removed(self):
        with self.assertRaises(UnicodeDecodeError):
            self.collect([b"DAWN PC BEGIN\n", b"\xffPC 40000010\n"])

    def test_incomplete_fixture_coverage(self):
        with self.assertRaisesRegex(RuntimeError, "Incomplete fixture"):
            self.collect([b"DAWN PC BEGIN\n", b"DAWN PC END\n"])


if __name__ == "__main__":
    unittest.main()
