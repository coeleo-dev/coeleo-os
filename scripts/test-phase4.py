#!/usr/bin/env python3
"""Headless Fase 4: two `uptime` samples (delta >= 1s) then `help`."""

from __future__ import annotations

import json
import os
import re
import socket
import subprocess
import sys
import tempfile
import time

PROMPT = "coeleo>"
HELP = "help: type text; Enter runs it; Backspace deletes"
UPTIME_RE = re.compile(r"uptime: (\d+):(\d{2})")
UPTIME_KEYS = ["u", "p", "t", "i", "m", "e", "ret"]
HELP_KEYS = ["h", "e", "l", "p", "ret"]


def recv_json(sock: socket.socket) -> dict:
    buf = b""
    while True:
        chunk = sock.recv(4096)
        if not chunk:
            raise EOFError("QMP socket closed")
        buf += chunk
        while b"\n" in buf:
            line, _, rest = buf.partition(b"\n")
            buf = rest
            line = line.strip()
            if not line:
                continue
            return json.loads(line)


def recv_reply(sock: socket.socket) -> dict:
    while True:
        msg = recv_json(sock)
        if "error" in msg:
            raise RuntimeError(f"QMP error: {msg}")
        if "return" in msg or "QMP" in msg:
            return msg


def qmp_exec(sock: socket.socket, payload: dict) -> dict:
    sock.sendall((json.dumps(payload) + "\n").encode())
    return recv_reply(sock)


def send_keys(sock: socket.socket, keys: list[str]) -> None:
    for key in keys:
        qmp_exec(
            sock,
            {
                "execute": "send-key",
                "arguments": {"keys": [{"type": "qcode", "data": key}]},
            },
        )
        time.sleep(0.05)


def wait_file_contains(path: str, needle: str, timeout: float) -> str:
    deadline = time.time() + timeout
    data = ""
    while time.time() < deadline:
        try:
            with open(path, "rb") as fh:
                data = fh.read().decode("utf-8", "replace")
        except FileNotFoundError:
            data = ""
        if needle in data:
            return data
        time.sleep(0.05)
    return data


def wait_uptime_count(path: str, count: int, timeout: float) -> str:
    deadline = time.time() + timeout
    data = ""
    while time.time() < deadline:
        try:
            with open(path, "rb") as fh:
                data = fh.read().decode("utf-8", "replace")
        except FileNotFoundError:
            data = ""
        if data.count("uptime:") >= count:
            return data
        time.sleep(0.05)
    return data


def wait_socket(path: str, proc: subprocess.Popen, timeout: float) -> None:
    deadline = time.time() + timeout
    while time.time() < deadline:
        if proc.poll() is not None:
            raise RuntimeError(f"QEMU exited early ({proc.returncode})")
        if os.path.exists(path):
            return
        time.sleep(0.05)
    raise TimeoutError(f"QMP socket not created: {path}")


def connect_qmp(path: str) -> socket.socket:
    last_err: Exception | None = None
    for _ in range(50):
        sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        try:
            sock.settimeout(5)
            sock.connect(path)
            return sock
        except OSError as exc:
            last_err = exc
            sock.close()
            time.sleep(0.05)
    raise RuntimeError(f"could not connect to QMP: {last_err}")


def parse_uptimes(serial: str) -> list[int]:
    times = []
    for match in UPTIME_RE.finditer(serial):
        mins = int(match.group(1))
        secs = int(match.group(2))
        times.append(mins * 60 + secs)
    return times


def main() -> int:
    if len(sys.argv) != 2:
        print(f"usage: {sys.argv[0]} coeleo.iso", file=sys.stderr)
        return 2
    iso = sys.argv[1]
    if not os.path.isfile(iso):
        print(f"test-phase4: missing ISO {iso}", file=sys.stderr)
        return 1

    with tempfile.TemporaryDirectory(prefix="coeleo-p4-") as tmp:
        serial_log = os.path.join(tmp, "serial.log")
        qmp_path = os.path.join(tmp, "qmp.sock")
        serial = ""
        qemu = subprocess.Popen(
            [
                "qemu-system-x86_64",
                "-M",
                "q35",
                "-m",
                "512M",
                "-cdrom",
                iso,
                "-serial",
                f"file:{serial_log}",
                "-display",
                "none",
                "-no-reboot",
                "-no-shutdown",
                "-qmp",
                f"unix:{qmp_path},server,nowait",
            ],
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        try:
            wait_socket(qmp_path, qemu, 5)
            sock = connect_qmp(qmp_path)
            try:
                recv_reply(sock)
                qmp_exec(sock, {"execute": "qmp_capabilities"})
                serial = wait_file_contains(serial_log, PROMPT, 12)
                if PROMPT not in serial:
                    print("test-phase4: timed out waiting for prompt", file=sys.stderr)
                    print(serial, file=sys.stderr)
                    return 1
                send_keys(sock, UPTIME_KEYS)
                serial = wait_uptime_count(serial_log, 1, 5)
                times = parse_uptimes(serial)
                if len(times) < 1:
                    print("test-phase4: serial did not contain first uptime", file=sys.stderr)
                    print(serial, file=sys.stderr)
                    return 1
                t0 = times[-1]
                time.sleep(2.2)
                send_keys(sock, UPTIME_KEYS)
                serial = wait_uptime_count(serial_log, 2, 5)
                times = parse_uptimes(serial)
                if len(times) < 2:
                    print("test-phase4: serial did not contain second uptime", file=sys.stderr)
                    print(serial, file=sys.stderr)
                    return 1
                t1 = times[-1]
                if t1 < t0 + 1:
                    print(
                        f"test-phase4: uptime did not advance ({t0}s -> {t1}s)",
                        file=sys.stderr,
                    )
                    print(serial, file=sys.stderr)
                    return 1
                send_keys(sock, HELP_KEYS)
                serial = wait_file_contains(serial_log, HELP, 5)
            finally:
                sock.close()
        finally:
            qemu.terminate()
            try:
                qemu.wait(timeout=3)
            except subprocess.TimeoutExpired:
                qemu.kill()
                qemu.wait()

        if HELP not in serial:
            print("test-phase4: serial did not contain help (keyboard)", file=sys.stderr)
            print(serial, file=sys.stderr)
            return 1
        print("test-phase4: ok")
        return 0


if __name__ == "__main__":
    raise SystemExit(main())
