#!/usr/bin/env python3
"""Phase 25: xHCI HID keyboard and mouse with no UHCI and no i8042."""

from __future__ import annotations

import json
import os
import socket
import subprocess
import sys
import tempfile
import time

HELP = "help: type text; Enter runs it; Backspace deletes"
PROMPT = "coeleo>"


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
                "execute": "input-send-event",
                "arguments": {
                    "events": [
                        {
                            "type": "key",
                            "data": {"down": True, "key": {"type": "qcode", "data": key}},
                        }
                    ]
                },
            },
        )
        time.sleep(0.02)
        qmp_exec(
            sock,
            {
                "execute": "input-send-event",
                "arguments": {
                    "events": [
                        {
                            "type": "key",
                            "data": {"down": False, "key": {"type": "qcode", "data": key}},
                        }
                    ]
                },
            },
        )
        time.sleep(0.05)


def send_rel(sock: socket.socket, dx: int, dy: int) -> None:
    events = []
    if dx:
        events.append({"type": "rel", "data": {"axis": "x", "value": dx}})
    if dy:
        events.append({"type": "rel", "data": {"axis": "y", "value": dy}})
    if not events:
        return
    qmp_exec(sock, {"execute": "input-send-event", "arguments": {"events": events}})


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


def fail(msg: str, serial: str) -> int:
    print(f"test-phase25: {msg}", file=sys.stderr)
    print(serial, file=sys.stderr)
    return 1


def main() -> int:
    if len(sys.argv) != 2:
        print(f"usage: {sys.argv[0]} coeleo.iso", file=sys.stderr)
        return 2
    iso = sys.argv[1]
    if not os.path.isfile(iso):
        print(f"test-phase25: missing ISO {iso}", file=sys.stderr)
        return 1

    with tempfile.TemporaryDirectory(prefix="coeleo-p25-") as tmp:
        serial_log = os.path.join(tmp, "serial.log")
        qmp_path = os.path.join(tmp, "qmp.sock")
        qemu = subprocess.Popen(
            [
                "qemu-system-x86_64",
                "-M",
                "q35,i8042=off",
                "-m",
                "512M",
                "-cdrom",
                iso,
                "-boot",
                "d",
                "-device",
                "qemu-xhci,id=xhci",
                "-device",
                "usb-hub,bus=xhci.0,port=1,id=hub",
                "-device",
                "usb-kbd,bus=xhci.0,port=1.1",
                "-device",
                "usb-mouse,bus=xhci.0,port=1.2",
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
            wait_socket(qmp_path, qemu, 8)
            sock = connect_qmp(qmp_path)
            try:
                recv_reply(sock)
                qmp_exec(sock, {"execute": "qmp_capabilities"})
                serial = wait_file_contains(serial_log, PROMPT, 25)
                if serial.count(PROMPT) < 1:
                    return fail("timed out waiting for prompt", serial)
                if "mouse: usb" not in serial:
                    return fail("missing mouse: usb (xHCI HID)", serial)
                if "xhci:" in serial:
                    return fail("unexpected xhci: boot line", serial)

                send_keys(sock, ["h", "e", "l", "p", "ret"])
                serial = wait_file_contains(serial_log, HELP, 8)
                if HELP not in serial:
                    return fail("USB keyboard did not produce in-kernel help", serial)

                for _ in range(40):
                    send_rel(sock, 0, 40)
                    time.sleep(0.05)
                serial = wait_file_contains(serial_log, "cursor:", 5)
                if "cursor:" not in serial:
                    return fail("USB mouse move did not log cursor:", serial)
            finally:
                sock.close()
        finally:
            qemu.terminate()
            try:
                qemu.wait(timeout=3)
            except subprocess.TimeoutExpired:
                qemu.kill()
                qemu.wait()

        print("test-phase25: ok")
        return 0


if __name__ == "__main__":
    raise SystemExit(main())
