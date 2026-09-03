#!/usr/bin/env python3
"""Headless Plasma P5: panel alpha/opaque, minimize, launcher spawn widgets."""

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

# files frame (48, 48), cw=480. Max slot centre (480, 64); min (448, 64).
# Floating bar: y = fb.h - PANEL_MARGIN - PANEL_H; launcher at
# PANEL_MARGIN + PANEL_INSET + SLOT/2 (7+4+18 = 29), KRunner next,
# files pin is the third 36px slot (x = 7+4+72+18 = 101).


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
    print(f"test-plasma-p5: {msg}", file=sys.stderr)
    print(serial, file=sys.stderr)
    return 1


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


def clamp_origin(sock: socket.socket) -> None:
    # 1280×800 (and 1920) need more than 30×40px after a clock shove to the corner.
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
    if len(sys.argv) != 4:
        print(f"usage: {sys.argv[0]} coeleo.iso sh_elf widgets_elf", file=sys.stderr)
        return 2
    iso, sh_elf, widgets_elf = sys.argv[1], sys.argv[2], sys.argv[3]
    if not os.path.isfile(iso):
        print(f"test-plasma-p5: missing ISO {iso}", file=sys.stderr)
        return 1

    with tempfile.TemporaryDirectory(prefix="coeleo-p5-") as tmp:
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
                if "wm: windows" not in serial:
                    return fail("missing wm: windows", serial)
                if "panel: h=32" not in serial:
                    return fail("missing panel: h=32", serial)
                if "panel: alpha" not in serial:
                    return fail("missing panel: alpha at boot", serial)

                clamp_origin(sock)
                go_bar(sock)
                for _ in range(40):
                    send_rel(sock, 40, 0)
                    time.sleep(0.05)
                send_rel(sock, -16, 0)
                time.sleep(0.05)
                send_click(sock)
                time.sleep(0.2)
                serial = read_serial(serial_log)
                if "focus: files" in serial:
                    return fail("clock click focused files", serial)
                if "fm: README.TXT" in serial:
                    return fail("clock click opened a file", serial)

                clamp_origin(sock)
                go_bar(sock)
                move_by_steps(sock, 29, 0)
                send_click(sock)
                serial = wait_file_contains(serial_log, "launcher: open", 5)
                if "launcher: open" not in serial:
                    return fail("missing launcher: open to spawn files", serial)
                send_keys(sock, ["ret"])
                serial = wait_file_contains(serial_log, "focus: files", 5)
                if "focus: files" not in serial:
                    return fail("launcher did not open files", serial)

                clamp_origin(sock)
                move_by_steps(sock, 480, 64)
                send_click(sock)
                serial = wait_file_contains(serial_log, "panel: opaque", 5)
                if "panel: opaque" not in serial:
                    return fail("maximize did not log panel: opaque", serial)

                move_by_steps(sock, 0, -48)
                for _ in range(40):
                    send_rel(sock, 40, 0)
                    time.sleep(0.05)
                send_rel(sock, -40, 0)
                time.sleep(0.05)
                send_click(sock)
                serial = wait_count(serial_log, "panel: alpha", 2, 5)
                if serial.count("panel: alpha") < 2:
                    return fail("restore did not log panel: alpha again", serial)

                clamp_origin(sock)
                move_by_steps(sock, 448, 64)
                send_click(sock)
                serial = wait_file_contains(serial_log, "wm: min", 5)
                if "wm: min" not in serial:
                    return fail("minimize deco did not log wm: min", serial)

                clamp_origin(sock)
                move_by_steps(sock, 72, 112)
                send_click(sock)
                time.sleep(0.2)
                serial = read_serial(serial_log)
                if "fm: README.TXT" in serial:
                    return fail("click hit files list while minimized", serial)

                clamp_origin(sock)
                go_bar(sock)
                move_by_steps(sock, 101, 0)
                send_click(sock)
                time.sleep(0.3)
                before_min = read_serial(serial_log).count("wm: min")
                send_click(sock)
                serial = wait_count(serial_log, "wm: min", before_min + 1, 5)
                if serial.count("wm: min") < before_min + 1:
                    return fail("task click did not minimize files", serial)

                clamp_origin(sock)
                go_bar(sock)
                move_by_steps(sock, 29, 0)
                send_click(sock)
                serial = wait_count(serial_log, "launcher: open", 2, 5)
                if serial.count("launcher: open") < 2:
                    return fail("missing launcher: open", serial)

                # files → sh → widgets (Restart/Power off sit under the apps).
                send_keys(sock, ["down", "down", "ret"])
                serial = wait_file_contains(serial_log, "win: create", 8)
                if "win: create" not in serial:
                    return fail("launcher did not spawn widgets", serial)
                if "launcher: close" not in serial:
                    return fail("missing launcher: close", serial)
            finally:
                sock.close()
        finally:
            qemu.terminate()
            try:
                qemu.wait(timeout=3)
            except subprocess.TimeoutExpired:
                qemu.kill()
                qemu.wait()

        print("test-plasma-p5: ok")
        return 0


if __name__ == "__main__":
    raise SystemExit(main())
