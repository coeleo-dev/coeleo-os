#!/usr/bin/env python3
"""Headless Plasma P7: wallpaper, KRunner panel button, fm jpg/png preview."""

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
SAMPLE = os.path.join(ROOT, "docs", "image", "sample.png")

# KRunner is the second 36px slot
# (PANEL_MARGIN + PANEL_INSET + SLOT + SLOT/2 = 7+4+36+18 = 65).


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


def send_rel(sock: socket.socket, dx: int, dy: int) -> None:
    events = []
    if dx:
        events.append({"type": "rel", "data": {"axis": "x", "value": dx}})
    if dy:
        events.append({"type": "rel", "data": {"axis": "y", "value": dy}})
    if not events:
        return
    qmp_exec(sock, {"execute": "input-send-event", "arguments": {"events": events}})


def send_btn(sock: socket.socket, down: bool) -> None:
    qmp_exec(
        sock,
        {
            "execute": "input-send-event",
            "arguments": {
                "events": [
                    {"type": "btn", "data": {"down": down, "button": "left"}},
                ]
            },
        },
    )


def send_click(sock: socket.socket) -> None:
    send_btn(sock, True)
    time.sleep(0.05)
    send_btn(sock, False)


def wait_file_contains(path: str, needle: str, timeout: float) -> str:
    deadline = time.time() + timeout
    data = ""
    while time.time() < deadline:
        data = read_serial(path)
        if needle in data:
            return data
        time.sleep(0.05)
    return data


def wait_prompt_count(path: str, n: int, timeout: float) -> str:
    deadline = time.time() + timeout
    data = ""
    while time.time() < deadline:
        data = read_serial(path)
        if data.count(PROMPT) >= n:
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
    if not os.path.isfile(WALL) or not os.path.isfile(SAMPLE):
        raise FileNotFoundError("docs/image/wallpaper.jpg or sample.png")
    env = os.environ.copy()
    env["MTOOLS_SKIP_CHECK"] = "1"
    subprocess.run(
        ["dd", "if=/dev/zero", f"of={path}", "bs=1M", "count=64", "status=none"],
        check=True,
    )
    subprocess.run(["mformat", "-i", path, "-F", "-c", "1", "-v", "COELEO", "::"], check=True, env=env)
    subprocess.run(["mcopy", "-i", path, WALL, "::wallpaper.jpg"], check=True, env=env)
    subprocess.run(["mcopy", "-i", path, SAMPLE, "::sample.png"], check=True, env=env)
    subprocess.run(["mcopy", "-i", path, SEED_README, "::README.TXT"], check=True, env=env)
    subprocess.run(["mmd", "-i", path, "::docs"], check=True, env=env)
    subprocess.run(["mcopy", "-i", path, SEED_HELLO, "::docs/HELLO.TXT"], check=True, env=env)
    subprocess.run(["mcopy", "-i", path, sh_elf, "::sh"], check=True, env=env)


def fail(msg: str, serial: str) -> int:
    print(f"test-plasma-p7: {msg}", file=sys.stderr)
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


def go_bottom(sock: socket.socket) -> None:
    for _ in range(40):
        send_rel(sock, 0, 40)
        time.sleep(0.05)


def go_bar(sock: socket.socket) -> None:
    go_bottom(sock)
    move_by_steps(sock, 0, -24)


def main() -> int:
    if len(sys.argv) != 3:
        print(f"usage: {sys.argv[0]} coeleo.iso sh_elf", file=sys.stderr)
        return 2
    iso, sh_elf = sys.argv[1], sys.argv[2]
    if not os.path.isfile(iso):
        print(f"test-plasma-p7: missing ISO {iso}", file=sys.stderr)
        return 1

    with tempfile.TemporaryDirectory(prefix="coeleo-p7-") as tmp:
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
                serial = wait_prompt_count(serial_log, 1, 20)
                if serial.count(PROMPT) < 1:
                    return fail("timed out waiting for prompt", serial)
                if "wm: windows" not in serial:
                    return fail("missing wm: windows", serial)
                if "panel: h=32" not in serial:
                    return fail("missing panel: h=32", serial)
                if "desk: wallpaper" not in serial:
                    return fail("missing desk: wallpaper", serial)
                if "desk: wallpaper fail" in serial:
                    return fail("wallpaper decode failed", serial)

                clamp_origin(sock)
                go_bar(sock)
                move_by_steps(sock, 65, 0)
                send_click(sock)
                serial = wait_file_contains(serial_log, "krunner: open", 5)
                if "krunner: open" not in serial:
                    return fail("KRunner panel button did not log krunner: open", serial)

                send_keys(sock, ["esc"])
                serial = wait_file_contains(serial_log, "krunner: close", 5)
                if "krunner: close" not in serial:
                    return fail("Esc did not close KRunner", serial)

                clamp_origin(sock)
                go_bar(sock)
                move_by_steps(sock, 29, 0)
                send_click(sock)
                serial = wait_file_contains(serial_log, "launcher: open", 5)
                if "launcher: open" not in serial:
                    return fail("missing launcher: open for files", serial)
                send_keys(sock, ["ret"])
                serial = wait_file_contains(serial_log, "focus: files", 5)
                if "focus: files" not in serial:
                    return fail("launcher did not open files", serial)

                clamp_origin(sock)
                # Files deco (oy=48, DECO_H=32). Not the navbar (path would steal keys).
                move_by_steps(sock, 100, 64)
                send_click(sock)
                send_keys(sock, ["w", "a", "l", "l", "ret"])
                serial = wait_file_contains(serial_log, "fm: image", 8)
                if "fm: image" not in serial and "fm: wallpaper.jpg" not in serial:
                    return fail("did not open jpg preview", serial)
                if "fm: not image" in serial:
                    return fail("jpg decode failed", serial)
            finally:
                sock.close()
        finally:
            qemu.terminate()
            try:
                qemu.wait(timeout=3)
            except subprocess.TimeoutExpired:
                qemu.kill()
                qemu.wait()

        print("test-plasma-p7: ok")
        return 0


if __name__ == "__main__":
    raise SystemExit(main())
