#!/usr/bin/env python3
"""Headless compositor: stacked VT+files windows, Plasma strut, USB mouse, ELF sh."""

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
README_BODY = "README from FAT"
INKERNEL_HELP = "help: type text; Enter runs it; Backspace deletes"

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


def go_bar(sock: socket.socket) -> None:
    for _ in range(40):
        send_rel(sock, 0, 40)
        time.sleep(0.05)
    move_by_steps(sock, 0, -24)


def wait_count(path: str, needle: str, n: int, timeout: float) -> str:
    deadline = time.time() + timeout
    data = ""
    while time.time() < deadline:
        try:
            with open(path, "rb") as fh:
                data = fh.read().decode("utf-8", "replace")
        except FileNotFoundError:
            data = ""
        if data.count(needle) >= n:
            return data
        time.sleep(0.05)
    return data


def open_launcher(sock: socket.socket, serial_log: str, n: int = 1) -> str:
    clamp_origin(sock)
    go_bar(sock)
    move_by_steps(sock, 29, 0)
    send_click(sock)
    return wait_count(serial_log, "launcher: open", n, 5)


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


def make_fat_image(path: str, sh_elf: str) -> None:
    if not os.path.isfile(SEED_README) or not os.path.isfile(SEED_HELLO):
        raise FileNotFoundError("disk-seed/README.TXT or disk-seed/docs/HELLO.TXT")
    if not os.path.isfile(sh_elf):
        raise FileNotFoundError(sh_elf)
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


def fail(msg: str, serial: str) -> int:
    print(f"test-phase13: {msg}", file=sys.stderr)
    print(serial, file=sys.stderr)
    return 1


def small_blit(serial: str) -> bool:
    ok = False
    for m in re.finditer(r"blit: (\d+)x(\d+)", serial):
        w, h = int(m.group(1)), int(m.group(2))
        if w <= 64 and h <= 64:
            ok = True
        if w >= 640 or h >= 400:
            return False
    return ok


def main() -> int:
    if len(sys.argv) != 3:
        print(f"usage: {sys.argv[0]} coeleo.iso sh_elf", file=sys.stderr)
        return 2
    iso, sh_elf = sys.argv[1], sys.argv[2]
    if not os.path.isfile(iso):
        print(f"test-phase13: missing ISO {iso}", file=sys.stderr)
        return 1

    with tempfile.TemporaryDirectory(prefix="coeleo-p13-") as tmp:
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
                if "panel: h=32" not in serial:
                    return fail("missing panel: h=32", serial)

                for _ in range(40):
                    send_rel(sock, 0, 40)
                    time.sleep(0.05)
                serial = wait_file_contains(serial_log, "cursor:", 5)
                if "cursor:" not in serial:
                    return fail("mouse move did not log cursor:", serial)
                serial = wait_file_contains(serial_log, "blit:", 3)
                if not small_blit(serial):
                    return fail("cursor blit was missing or not a dirty rect", serial)

                # Clock is inset on the floating bar (PANEL_MARGIN + PAD from the
                # bar's right), not the screen corner.
                for _ in range(40):
                    send_rel(sock, 40, 0)
                    time.sleep(0.05)
                for _ in range(3):
                    send_rel(sock, 0, -8)
                    time.sleep(0.05)
                send_rel(sock, -16, 0)
                time.sleep(0.05)

                send_click(sock)
                time.sleep(0.2)
                serial = wait_file_contains(serial_log, "cursor:", 5)
                if "focus: files" in serial:
                    return fail("click on strut focused files", serial)
                if "fm: README.TXT" in serial:
                    return fail("click on strut opened a file", serial)

                serial = open_launcher(sock, serial_log)
                if "launcher: open" not in serial:
                    return fail("missing launcher: open for sh", serial)
                send_keys(sock, ["down", "ret"])
                time.sleep(0.3)
                serial = open_launcher(sock, serial_log, 2)
                if serial.count("launcher: open") < 2:
                    return fail("missing launcher: open for files", serial)
                send_keys(sock, ["ret"])
                serial = wait_file_contains(serial_log, "focus: files", 5)
                if "focus: files" not in serial:
                    return fail("launcher did not open files", serial)

                # Clamp to (0,0), then first Files row:
                # frame (48, 48) + DECO_H(32) + NAV_H(32) + COL_H(FONT_H+GAP) + 8
                # list x = ox + SIDE_W(160) + PAD(8) → (216, ~142) on 8px grid (216, 144).
                for _ in range(40):
                    send_rel(sock, -40, -40)
                    time.sleep(0.05)
                for _ in range(27):
                    send_rel(sock, 8, 0)
                    time.sleep(0.05)
                for _ in range(18):
                    send_rel(sock, 0, 8)
                    time.sleep(0.05)

                send_click(sock)
                serial = wait_file_contains(serial_log, "focus: files", 5)
                if "focus: files" not in serial:
                    return fail("click did not focus files", serial)
                send_click(sock)
                serial = wait_file_contains(serial_log, "fm: README.TXT", 5)
                if "fm: README.TXT" not in serial:
                    return fail("double-click did not open README.TXT", serial)
                if README_BODY not in serial:
                    return fail("README body missing after click", serial)

                send_keys(sock, ["tab"])
                time.sleep(0.2)
                type_line(sock, "help")
                serial = wait_prompt_count(serial_log, 2, 8)
                if INKERNEL_HELP in serial:
                    return fail("in-kernel help on serial; expected ELF sh", serial)
            finally:
                sock.close()
        finally:
            qemu.terminate()
            try:
                qemu.wait(timeout=3)
            except subprocess.TimeoutExpired:
                qemu.kill()
                qemu.wait()

        print("test-phase13: ok")
        return 0


if __name__ == "__main__":
    raise SystemExit(main())
