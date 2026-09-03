#!/usr/bin/env python3
"""Headless Plasma P3: run widgets; QMP click on the button; serial has ui: clicked."""

from __future__ import annotations

import json
import os
import socket
import subprocess
import sys
import tempfile
import time

PROMPT = "coeleo>"

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SEED_README = os.path.join(ROOT, "disk-seed", "README.TXT")
SEED_HELLO = os.path.join(ROOT, "disk-seed", "docs", "HELLO.TXT")


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


def type_line(sock: socket.socket, text: str) -> None:
    keys: list[str] = []
    for ch in text:
        if "a" <= ch <= "z" or "0" <= ch <= "9":
            keys.append(ch)
        elif ch == " ":
            keys.append("spc")
        elif ch == "/":
            keys.append("slash")
        elif ch == ".":
            keys.append("dot")
        else:
            raise ValueError(f"unsupported qcode char {ch!r}")
    keys.append("ret")
    send_keys(sock, keys)


def send_rel(sock: socket.socket, dx: int, dy: int) -> None:
    events = []
    if dx:
        events.append({"type": "rel", "data": {"axis": "x", "value": dx}})
    if dy:
        events.append({"type": "rel", "data": {"axis": "y", "value": dy}})
    if not events:
        return
    qmp_exec(sock, {"execute": "input-send-event", "arguments": {"events": events}})


def send_click(sock: socket.socket) -> None:
    qmp_exec(
        sock,
        {
            "execute": "input-send-event",
            "arguments": {
                "events": [
                    {"type": "btn", "data": {"down": True, "button": "left"}},
                ]
            },
        },
    )
    time.sleep(0.05)
    qmp_exec(
        sock,
        {
            "execute": "input-send-event",
            "arguments": {
                "events": [
                    {"type": "btn", "data": {"down": False, "button": "left"}},
                ]
            },
        },
    )


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


def wait_prompt_count(path: str, n: int, timeout: float) -> str:
    deadline = time.time() + timeout
    data = ""
    while time.time() < deadline:
        try:
            with open(path, "rb") as fh:
                data = fh.read().decode("utf-8", "replace")
        except FileNotFoundError:
            data = ""
        if data.count(PROMPT) >= n:
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


def make_fat_image(path: str, sh_elf: str, widgets_elf: str) -> None:
    if not os.path.isfile(SEED_README) or not os.path.isfile(SEED_HELLO):
        raise FileNotFoundError("disk-seed/README.TXT or disk-seed/docs/HELLO.TXT")
    if not os.path.isfile(sh_elf):
        raise FileNotFoundError(sh_elf)
    if not os.path.isfile(widgets_elf):
        raise FileNotFoundError(widgets_elf)
    env = os.environ.copy()
    env["MTOOLS_SKIP_CHECK"] = "1"
    subprocess.run(
        ["dd", "if=/dev/zero", f"of={path}", "bs=1M", "count=64", "status=none"],
        check=True,
    )
    subprocess.run(["mformat", "-i", path, "-F", "-c", "1", "-v", "COELEO", "::"], check=True, env=env)
    subprocess.run(["mcopy", "-i", path, SEED_README, "::README.TXT"], check=True, env=env)
    subprocess.run(["mmd", "-i", path, "::docs"], check=True, env=env)
    subprocess.run(["mcopy", "-i", path, SEED_HELLO, "::docs/HELLO.TXT"], check=True, env=env)
    subprocess.run(["mcopy", "-i", path, sh_elf, "::sh"], check=True, env=env)
    subprocess.run(["mcopy", "-i", path, widgets_elf, "::widgets"], check=True, env=env)


def fail(msg: str, serial: str) -> int:
    print(f"test-plasma-p3: {msg}", file=sys.stderr)
    print(serial, file=sys.stderr)
    return 1


def main() -> int:
    if len(sys.argv) != 4:
        print(f"usage: {sys.argv[0]} coeleo.iso sh_elf widgets_elf", file=sys.stderr)
        return 2
    iso, sh_elf, widgets_elf = sys.argv[1], sys.argv[2], sys.argv[3]
    if not os.path.isfile(iso):
        print(f"test-plasma-p3: missing ISO {iso}", file=sys.stderr)
        return 1

    with tempfile.TemporaryDirectory(prefix="coeleo-p3-") as tmp:
        serial_log = os.path.join(tmp, "serial.log")
        qmp_path = os.path.join(tmp, "qmp.sock")
        disk = os.path.join(tmp, "disk.img")
        make_fat_image(disk, sh_elf, widgets_elf)
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
                "-boot",
                "d",
                "-drive",
                f"file={disk},if=none,format=raw,id=vd0",
                "-device",
                "virtio-blk-pci,drive=vd0",
                "-device",
                "piix3-usb-uhci,id=uhci",
                "-device",
                "usb-mouse,bus=uhci.0",
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
                serial = wait_prompt_count(serial_log, 1, 20)
                if serial.count(PROMPT) < 1:
                    return fail("timed out waiting for prompt", serial)

                type_line(sock, "run widgets")
                serial = wait_file_contains(serial_log, "win: create id=1", 8)
                if "win: create id=1" not in serial:
                    return fail("missing win: create id=1", serial)

                # Clamp to (0,0). Button is at screen (72, 112):
                # frame (16, 8) + DECO_H(32) + client (8+48, 48+16).
                for _ in range(30):
                    send_rel(sock, -40, -40)
                    time.sleep(0.05)
                for _ in range(9):
                    send_rel(sock, 8, 0)
                    time.sleep(0.05)
                for _ in range(14):
                    send_rel(sock, 0, 8)
                    time.sleep(0.05)
                send_click(sock)
                serial = wait_file_contains(serial_log, "ui: clicked", 8)
                if "ui: clicked" not in serial:
                    return fail("missing ui: clicked", serial)
            finally:
                sock.close()
        finally:
            qemu.terminate()
            try:
                qemu.wait(timeout=3)
            except subprocess.TimeoutExpired:
                qemu.kill()
                qemu.wait()

        print("test-plasma-p3: ok")
        return 0


if __name__ == "__main__":
    raise SystemExit(main())
