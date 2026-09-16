#!/usr/bin/env python3
"""Headless desk menu + Settings: right-click, panel Float/Full, wallpaper jpg."""

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
WALL = os.path.join(ROOT, "docs", "image", "wallpaper.jpg")

# Empty work, away from launcher/KRunner/task slots (29/65/101) and boot frames.
EMPTY = (400, 200)
# Settings frame origin (60, 36) + DECO_H (32) -> content at (60, 68). Coordinates
# mirror kernel/src/ui/deskset.rs::layout(520, 400): mode row at y=37, grid at
# y=119 with 109-tall cards, so the buttons/cards below are their centres.
FULL_BTN = (446, 117)
FLOAT_BTN = (194, 117)
# Card row 0, col 0 ("Default"); whole card is the hit target.
WALL_CARD = (154, 241)


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


def send_rel(sock: socket.socket, dx: int, dy: int) -> None:
    events = []
    if dx:
        events.append({"type": "rel", "data": {"axis": "x", "value": dx}})
    if dy:
        events.append({"type": "rel", "data": {"axis": "y", "value": dy}})
    if not events:
        return
    qmp_exec(sock, {"execute": "input-send-event", "arguments": {"events": events}})


def send_btn(sock: socket.socket, down: bool, button: str = "left") -> None:
    qmp_exec(
        sock,
        {
            "execute": "input-send-event",
            "arguments": {
                "events": [
                    {"type": "btn", "data": {"down": down, "button": button}},
                ]
            },
        },
    )


def send_click(sock: socket.socket, button: str = "left") -> None:
    send_btn(sock, True, button)
    time.sleep(0.05)
    send_btn(sock, False, button)


def wait_file_contains(path: str, needle: str, timeout: float) -> str:
    deadline = time.time() + timeout
    data = ""
    while time.time() < deadline:
        data = read_serial(path)
        if needle in data:
            return data
        time.sleep(0.05)
    return data


def wait_count(path: str, needle: str, n: int, timeout: float) -> str:
    deadline = time.time() + timeout
    data = ""
    while time.time() < deadline:
        data = read_serial(path)
        if data.count(needle) >= n:
            return data
        time.sleep(0.05)
    return data


def read_serial(path: str) -> str:
    try:
        with open(path, "rb") as fh:
            return fh.read().decode("utf-8", "replace")
    except FileNotFoundError:
        return ""


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


def make_fat_image(path: str, sh_elf: str) -> None:
    if not os.path.isfile(SEED_README) or not os.path.isfile(SEED_HELLO):
        raise FileNotFoundError("disk-seed/README.TXT or disk-seed/docs/HELLO.TXT")
    if not os.path.isfile(WALL):
        raise FileNotFoundError("docs/image/wallpaper.jpg")
    env = os.environ.copy()
    env["MTOOLS_SKIP_CHECK"] = "1"
    subprocess.run(
        ["dd", "if=/dev/zero", f"of={path}", "bs=1M", "count=64", "status=none"],
        check=True,
    )
    subprocess.run(["mformat", "-i", path, "-F", "-c", "1", "-v", "COELEO", "::"], check=True, env=env)
    subprocess.run(["mcopy", "-i", path, WALL, "::wallpaper.jpg"], check=True, env=env)
    subprocess.run(["mcopy", "-i", path, SEED_README, "::README.TXT"], check=True, env=env)
    subprocess.run(["mmd", "-i", path, "::docs"], check=True, env=env)
    subprocess.run(["mcopy", "-i", path, SEED_HELLO, "::docs/HELLO.TXT"], check=True, env=env)
    subprocess.run(["mcopy", "-i", path, sh_elf, "::sh"], check=True, env=env)


def fail(msg: str, serial: str) -> int:
    print(f"test-desk: {msg}", file=sys.stderr)
    print(serial, file=sys.stderr)
    return 1


def clamp_origin(sock: socket.socket) -> None:
    for _ in range(50):
        send_rel(sock, -40, -40)
        time.sleep(0.05)


def move_by_steps(sock: socket.socket, dx: int, dy: int, step: int = 8) -> None:
    while dx >= step:
        send_rel(sock, step, 0)
        time.sleep(0.05)
        dx -= step
    while dx <= -step:
        send_rel(sock, -step, 0)
        time.sleep(0.05)
        dx += step
    while dy >= step:
        send_rel(sock, 0, step)
        time.sleep(0.05)
        dy -= step
    while dy <= -step:
        send_rel(sock, 0, -step)
        time.sleep(0.05)
        dy += step
    if dx or dy:
        send_rel(sock, dx, dy)
        time.sleep(0.05)


def go(sock: socket.socket, x: int, y: int) -> None:
    clamp_origin(sock)
    move_by_steps(sock, x, y)


def main() -> int:
    if len(sys.argv) != 3:
        print(f"usage: {sys.argv[0]} coeleo.iso sh_elf", file=sys.stderr)
        return 2
    iso, sh_elf = sys.argv[1], sys.argv[2]
    if not os.path.isfile(iso):
        print(f"test-desk: missing ISO {iso}", file=sys.stderr)
        return 1

    with tempfile.TemporaryDirectory(prefix="coeleo-desk-") as tmp:
        serial_log = os.path.join(tmp, "serial.log")
        qmp_path = os.path.join(tmp, "qmp.sock")
        disk = os.path.join(tmp, "disk.img")
        make_fat_image(disk, sh_elf)
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
                serial = wait_file_contains(serial_log, PROMPT, 20)
                if serial.count(PROMPT) < 1:
                    return fail("timed out waiting for prompt", serial)
                if "wm: windows" not in serial:
                    return fail("missing wm: windows", serial)
                if "mouse: usb" not in serial:
                    return fail("missing mouse: usb (UHCI HID mouse)", serial)
                wall0 = serial.count("desk: wallpaper")

                go(sock, EMPTY[0], EMPTY[1])
                send_click(sock, "right")
                serial = wait_file_contains(serial_log, "desk: menu", 5)
                if "desk: menu" not in serial:
                    return fail("right-click empty work did not log desk: menu", serial)

                send_click(sock, "left")
                serial = wait_file_contains(serial_log, "focus: desk", 5)
                if "focus: desk" not in serial:
                    return fail("desk menu did not open Settings (focus: desk)", serial)
                if "focus: files" in serial.split("focus: desk")[-1]:
                    return fail("Settings focused files instead of desk", serial)

                go(sock, FULL_BTN[0], FULL_BTN[1])
                send_click(sock, "left")
                serial = wait_file_contains(serial_log, "panel: full", 5)
                if "panel: full" not in serial:
                    return fail("Full button did not log panel: full", serial)

                go(sock, FLOAT_BTN[0], FLOAT_BTN[1])
                send_click(sock, "left")
                serial = wait_file_contains(serial_log, "panel: float", 5)
                if "panel: float" not in serial:
                    return fail("Float button did not log panel: float", serial)

                go(sock, WALL_CARD[0], WALL_CARD[1])
                send_click(sock, "left")
                serial = wait_count(serial_log, "desk: wallpaper", wall0 + 1, 8)
                if serial.count("desk: wallpaper") < wall0 + 1:
                    return fail("jpg wallpaper click did not log desk: wallpaper", serial)
                if "desk: wallpaper fail" in serial:
                    return fail("wallpaper decode failed", serial)
            finally:
                sock.close()
        finally:
            qemu.terminate()
            try:
                qemu.wait(timeout=3)
            except subprocess.TimeoutExpired:
                qemu.kill()
                qemu.wait()

        print("test-desk: ok")
        return 0


if __name__ == "__main__":
    raise SystemExit(main())
