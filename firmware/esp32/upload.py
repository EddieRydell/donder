"""Provision Wi-Fi, upload a prepared sequence over HTTP, and verify frames."""

import argparse
import getpass
import hashlib
import http.client
import json
import logging
import pathlib
import re
import socket
import struct
import subprocess
import sys
import time

import serial


parser = argparse.ArgumentParser()
parser.add_argument("sequence", type=pathlib.Path)
parser.add_argument("--port", default="COM4")
parser.add_argument("--ssid")
parser.add_argument(
    "--windows-profile",
    help="Provision this saved Windows Wi-Fi profile without logging its password",
)
parser.add_argument("--checksums", type=pathlib.Path, help="Verify frames against this checksum file after uploading")
parser.add_argument("--repeat", type=int, default=1, help="Repeat explicitly requested checksum verification")
parser.add_argument("--uploads", type=int, default=1)
parser.add_argument(
    "--monitor-seconds",
    type=float,
    default=0,
    help="Capture PLAYBACK records from USB serial after HTTP verification",
)
parser.add_argument(
    "--exercise-rejections",
    action="store_true",
    help="Test invalid uploads, authorization, and interrupted HTTP bodies",
)
parser.add_argument(
    "--log", type=pathlib.Path, help="New evidence file; existing files are not overwritten"
)
parser.add_argument(
    "--elf",
    type=pathlib.Path,
    help="Record the hash of this firmware ELF in verification evidence",
)
args = parser.parse_args()

if args.repeat < 1 or args.uploads < 1 or args.monitor_seconds < 0:
    parser.error("--repeat and --uploads must be positive; --monitor-seconds cannot be negative")
if not args.ssid and not args.windows_profile:
    parser.error("provide --ssid or --windows-profile")
if args.checksums is None and (args.repeat != 1 or args.exercise_rejections):
    parser.error("--repeat and --exercise-rejections require --checksums")

handlers = [logging.StreamHandler(sys.stdout)]
if args.log:
    handlers.append(logging.FileHandler(args.log, mode="x"))
logging.basicConfig(level=logging.INFO, format="%(message)s", handlers=handlers)

payload = args.sequence.read_bytes()
if len(payload) < 16 or payload[:4] != b"DOND":
    parser.error("sequence is not a Donder prepared-sequence file")
sequence_format, payload_bytes = struct.unpack_from("<II", payload, 4)
if payload_bytes != len(payload) - 16:
    parser.error("sequence payload length does not match its header")
expected = []
if args.checksums is not None:
    expected = [tuple(map(int, line.split())) for line in args.checksums.read_text().splitlines()]
    if not expected or any(len(frame) != 2 or any(value < 0 or value > 0xFFFFFFFF for value in frame) for frame in expected):
        parser.error("checksum files must contain nonempty rows of unsigned 32-bit tick and checksum pairs")
if args.elf is not None:
    logging.info("elf_sha256=%s", hashlib.sha256(args.elf.read_bytes()).hexdigest())
logging.info(
    "payload_bytes=%s payload_sha256=%s transport=http",
    len(payload),
    hashlib.sha256(payload).hexdigest(),
)


def credentials():
    if args.windows_profile:
        result = subprocess.run(
            [
                "netsh",
                "wlan",
                "show",
                "profile",
                f"name={args.windows_profile}",
                "key=clear",
            ],
            capture_output=True,
            check=True,
        )
        profile = result.stdout.decode(errors="replace")
        match = re.search(r"Key Content\s*:\s*(.+)", profile)
        if not match:
            raise RuntimeError("Saved Wi-Fi profile has no accessible personal-network key")
        ssid = (args.ssid or args.windows_profile).encode()
        password = match[1].strip().encode()
        del profile, result, match
    else:
        ssid = args.ssid.encode()
        password = getpass.getpass("Wi-Fi password: ").encode()
    if not 0 < len(ssid) <= 32 or len(password) > 64:
        raise ValueError("Invalid Wi-Fi credential lengths")
    return ssid, password


def provision():
    ssid, password = credentials()
    transcript = []
    with serial.Serial(port=None, baudrate=115200, timeout=0.1) as port:
        port.port = args.port
        port.dtr = False
        port.rts = False
        port.open()
        port.rts = True
        time.sleep(0.1)
        port.reset_input_buffer()
        port.rts = False

        deadline = time.monotonic() + 20
        while time.monotonic() < deadline:
            port.write(b"P")
            line = port.readline()
            if line:
                transcript.append(line)
                if line == b"DONDER PROVISION READY\n":
                    break
        else:
            recent = b"".join(transcript[-20:])
            raise RuntimeError(f"Provisioning handshake timed out: {recent!r}")

        port.write(b"W" + bytes([len(ssid), len(password)]) + ssid + password)
        del password

        token_line = port.readline().strip()
        if not token_line.startswith(b"TOKEN "):
            raise RuntimeError(f"Device did not return an HTTP token: {token_line!r}")
        token = token_line.removeprefix(b"TOKEN ").decode("ascii")
        if len(token) != 32 or any(character not in "0123456789abcdef" for character in token):
            raise RuntimeError("Device returned an invalid HTTP token")

        deadline = time.monotonic() + 60
        transcript.clear()
        while time.monotonic() < deadline:
            line = port.readline()
            if not line:
                continue
            transcript.append(line)
            marker = line.find(b"WIFI READY ")
            if marker >= 0:
                text = line[marker:].decode("ascii").strip()
                _, _, address, http_port, *_ = text.split()
                logging.info(text)
                return address, int(http_port), token
        recent = b"".join(transcript[-20:])
        raise RuntimeError(f"Wi-Fi did not become ready: {recent!r}")


address, port, token = provision()
connection = http.client.HTTPConnection(address, port, timeout=20)


def request(method, path, body, supplied_token=token):
    connection.request(
        method,
        path,
        body=body,
        headers={
            "Content-Type": "application/octet-stream",
            "X-Donder-Token": supplied_token,
        },
    )
    response = connection.getresponse()
    response_body = response.read().decode("ascii").strip()
    return response.status, response_body


def upload(data, status=200, prefix="LOADED "):
    actual_status, response = request("PUT", "/sequence", data)
    if actual_status != status or not response.startswith(prefix):
        raise RuntimeError(
            f"Expected HTTP {status} and {prefix!r}, received HTTP {actual_status}: {response!r}"
        )
    logging.info(response)


status, response = request("GET", "/capabilities", b"")
if status != 200:
    raise RuntimeError(f"Device capabilities failed with HTTP {status}: {response!r}")
capabilities = json.loads(response)
if capabilities["sequenceFormat"] != sequence_format:
    raise RuntimeError(
        f"Device accepts sequence format {capabilities['sequenceFormat']}; "
        f"this file uses {sequence_format}. Rebuild the firmware and re-export the sequence together."
    )
if payload_bytes > capabilities["maxPayloadBytes"]:
    raise RuntimeError(
        f"Sequence payload is {payload_bytes} bytes; device limit is "
        f"{capabilities['maxPayloadBytes']} bytes. Export fewer outputs or simplify the sequence."
    )
logging.info("CAPABILITIES %s", json.dumps(capabilities, sort_keys=True))

for _ in range(args.uploads):
    start = time.monotonic()
    upload(payload)
    logging.info("transfer_seconds=%.3f", time.monotonic() - start)

if args.exercise_rejections:
    wrong_token = "0" * 32 if token != "0" * 32 else "1" * 32
    status, response = request("PUT", "/sequence", b"", wrong_token)
    assert (status, response) == (401, "Missing or invalid X-Donder-Token"), (status, response)
    logging.info("VERIFIED HTTP authorization rejection")

    version = bytearray(payload)
    version[4:8] = struct.pack("<I", 999)
    upload(version, 422, "REJECT Version")

    oversized = bytearray(payload[:16])
    oversized[8:12] = struct.pack("<I", 32 * 1024 + 1)
    upload(oversized, 422, "REJECT Limit")

    corrupt = bytearray(payload)
    corrupt[-1] ^= 0x80
    upload(corrupt, 422, "REJECT Checksum")

    connection.close()
    interrupted = socket.create_connection((address, port), timeout=10)
    headers = (
        f"PUT /sequence HTTP/1.1\r\nHost: {address}\r\n"
        f"X-Donder-Token: {token}\r\nContent-Type: application/octet-stream\r\n"
        f"Content-Length: {len(payload)}\r\nConnection: close\r\n\r\n"
    ).encode("ascii")
    interrupted.sendall(headers + payload[:32])
    time.sleep(0.1)
    concurrent = http.client.HTTPConnection(address, port, timeout=20)
    concurrent.request(
        "PUT",
        "/sequence",
        body=payload,
        headers={
            "Content-Type": "application/octet-stream",
            "X-Donder-Token": token,
        },
    )
    concurrent_response = concurrent.getresponse()
    concurrent_body = concurrent_response.read().decode("ascii").strip()
    assert (concurrent_response.status, concurrent_body) == (
        409,
        "Another upload is in progress",
    ), (concurrent_response.status, concurrent_body)
    concurrent.close()
    logging.info("VERIFIED concurrent HTTP upload rejection")
    interrupted.close()
    time.sleep(0.2)
    connection = http.client.HTTPConnection(address, port, timeout=20)
    logging.info("VERIFIED interrupted HTTP upload disconnect")

for ticks, checksum in expected * args.repeat:
    status, line = request("POST", "/frame", struct.pack("<I", ticks))
    if status != 200 or not line.startswith("FRAME "):
        raise RuntimeError(f"Frame request failed with HTTP {status}: {line!r}")
    _, actual_ticks, actual_crc, micros, allocations, global_allocations = line.split()
    assert (int(actual_ticks), int(actual_crc), int(allocations)) == (
        ticks,
        checksum,
        0,
    ), line
    logging.info(line)

if args.checksums is not None:
    logging.info("VERIFIED %s frames; zero evaluation allocations", len(expected) * args.repeat)
else:
    logging.info("UPLOADED sequence; frame checksum verification was not requested")
connection.close()

if args.monitor_seconds:
    playback_records = 0
    with serial.Serial(port=None, baudrate=115200, timeout=0.25) as monitor:
        monitor.port = args.port
        monitor.dtr = False
        monitor.rts = False
        monitor.open()
        deadline = time.monotonic() + args.monitor_seconds
        while time.monotonic() < deadline:
            line = monitor.readline()
            marker = line.find(b"PLAYBACK ")
            if marker >= 0:
                logging.info(line[marker:].decode("ascii").strip())
                playback_records += 1
    if playback_records == 0:
        raise RuntimeError("No PLAYBACK records received during monitor window")
    logging.info("CAPTURED %s playback windows", playback_records)
