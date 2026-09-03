#!/usr/bin/env python3
"""Headless phase 16: RTC date, ACPI reboot/poweroff, launcher confirm."""

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
DATE_RE = re.compile(r"[12][0-9]{3}-[0-1][0-9]-[0-3][0-9]")

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


def send_btn(sock: socket.socket, down: bool) -> None:
    qmp_exec(
        sock,
        {
            "execute": "input-send-event",
            "arguments": {
                "events": [{"type": "btn", "data": {"down": down, "button": "left"}}]
            },
        },
    )


def send_click(sock: socket.socket) -> None:
    send_btn(sock, True)
    time.sleep(0.05)
    send_btn(sock, False)


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
    print(f"test-phase16: {msg}", file=sys.stderr)
    print(serial, file=sys.stderr)
    return 1


def qemu_cmd(iso: str, disk: str, serial_log: str, qmp_path: str, *, reboot: bool, shutdown: bool) -> list[str]:
    cmd = [
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
        "-qmp",
        f"unix:{qmp_path},server,nowait",
    ]
    if not reboot:
        cmd.append("-no-reboot")
    if not shutdown:
        cmd.append("-no-shutdown")
    return cmd


def stop_qemu(qemu: subprocess.Popen) -> None:
    qemu.terminate()
    try:
        qemu.wait(timeout=3)
    except subprocess.TimeoutExpired:
        qemu.kill()
        qemu.wait()


def case_date(iso: str, sh_elf: str) -> int:
    with tempfile.TemporaryDirectory(prefix="coeleo-p16-date-") as tmp:
        serial_log = os.path.join(tmp, "serial.log")
        qmp_path = os.path.join(tmp, "qmp.sock")
        disk = os.path.join(tmp, "disk.img")
        make_fat_image(disk, sh_elf)
        qemu = subprocess.Popen(
            qemu_cmd(iso, disk, serial_log, qmp_path, reboot=False, shutdown=False),
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        serial = ""
        try:
            wait_socket(qmp_path, qemu, 5)
            sock = connect_qmp(qmp_path)
            try:
                recv_reply(sock)
                qmp_exec(sock, {"execute": "qmp_capabilities"})
                serial = wait_prompt_count(serial_log, 1, 20)
                if serial.count(PROMPT) < 1:
                    return fail("date: timed out waiting for prompt", serial)
                type_line(sock, "date")
                serial = wait_prompt_count(serial_log, 2, 8)
                if serial.count(PROMPT) < 2:
                    return fail("date: timed out after date", serial)
                if not DATE_RE.search(serial):
                    return fail("date did not print a civil YYYY-MM-DD", serial)
                if "uptime:" in serial and DATE_RE.search(serial) is None:
                    return fail("date printed only uptime", serial)
            finally:
                sock.close()
        finally:
            stop_qemu(qemu)
    return 0


def case_reboot(iso: str, sh_elf: str) -> int:
    with tempfile.TemporaryDirectory(prefix="coeleo-p16-reboot-") as tmp:
        serial_log = os.path.join(tmp, "serial.log")
        qmp_path = os.path.join(tmp, "qmp.sock")
        disk = os.path.join(tmp, "disk.img")
        make_fat_image(disk, sh_elf)
        qemu = subprocess.Popen(
            qemu_cmd(iso, disk, serial_log, qmp_path, reboot=True, shutdown=False),
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        serial = ""
        try:
            wait_socket(qmp_path, qemu, 5)
            sock = connect_qmp(qmp_path)
            try:
                recv_reply(sock)
                qmp_exec(sock, {"execute": "qmp_capabilities"})
                serial = wait_prompt_count(serial_log, 1, 20)
                if serial.count(PROMPT) < 1:
                    return fail("reboot: timed out waiting for prompt", serial)
                type_line(sock, "reboot")
                serial = wait_count(serial_log, "Coeleo OS", 2, 25)
                if serial.count("Coeleo OS") < 2:
                    return fail("reboot did not print Coeleo OS twice", serial)
            finally:
                sock.close()
        finally:
            stop_qemu(qemu)
    return 0


def case_poweroff(iso: str, sh_elf: str) -> int:
    with tempfile.TemporaryDirectory(prefix="coeleo-p16-off-") as tmp:
        serial_log = os.path.join(tmp, "serial.log")
        qmp_path = os.path.join(tmp, "qmp.sock")
        disk = os.path.join(tmp, "disk.img")
        make_fat_image(disk, sh_elf)
        qemu = subprocess.Popen(
            qemu_cmd(iso, disk, serial_log, qmp_path, reboot=True, shutdown=True),
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        serial = ""
        try:
            wait_socket(qmp_path, qemu, 5)
            sock = connect_qmp(qmp_path)
            try:
                recv_reply(sock)
                qmp_exec(sock, {"execute": "qmp_capabilities"})
                serial = wait_prompt_count(serial_log, 1, 20)
                if serial.count(PROMPT) < 1:
                    return fail("poweroff: timed out waiting for prompt", serial)
                type_line(sock, "poweroff")
            finally:
                sock.close()
            try:
                rc = qemu.wait(timeout=15)
            except subprocess.TimeoutExpired:
                serial = wait_file_contains(serial_log, "poweroff", 1)
                stop_qemu(qemu)
                return fail("poweroff: QEMU did not exit", serial)
            if rc != 0:
                serial = wait_file_contains(serial_log, "poweroff", 1)
                return fail(f"poweroff: QEMU exit {rc}", serial)
        except Exception:
            stop_qemu(qemu)
            raise
    return 0


def case_menu_poweroff(iso: str, sh_elf: str) -> int:
    with tempfile.TemporaryDirectory(prefix="coeleo-p16-menu-") as tmp:
        serial_log = os.path.join(tmp, "serial.log")
        qmp_path = os.path.join(tmp, "qmp.sock")
        disk = os.path.join(tmp, "disk.img")
        make_fat_image(disk, sh_elf)
        qemu = subprocess.Popen(
            qemu_cmd(iso, disk, serial_log, qmp_path, reboot=True, shutdown=True),
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        serial = ""
        try:
            wait_socket(qmp_path, qemu, 5)
            sock = connect_qmp(qmp_path)
            try:
                recv_reply(sock)
                qmp_exec(sock, {"execute": "qmp_capabilities"})
                serial = wait_prompt_count(serial_log, 1, 20)
                if serial.count(PROMPT) < 1:
                    return fail("menu: timed out waiting for prompt", serial)
                clamp_origin(sock)
                go_bar(sock)
                move_by_steps(sock, 29, 0)
                send_click(sock)
                serial = wait_file_contains(serial_log, "launcher: open", 5)
                if "launcher: open" not in serial:
                    return fail("menu: missing launcher: open", serial)
                send_keys(sock, ["down", "down", "down", "ret"])
                serial = wait_file_contains(serial_log, "power: confirm poweroff", 5)
                if "power: confirm poweroff" not in serial:
                    return fail("menu: missing power: confirm poweroff", serial)
                send_keys(sock, ["right", "ret"])
            finally:
                sock.close()
            try:
                rc = qemu.wait(timeout=15)
            except subprocess.TimeoutExpired:
                serial = wait_file_contains(serial_log, "power:", 1)
                stop_qemu(qemu)
                return fail("menu poweroff: QEMU did not exit", serial)
            if rc != 0:
                serial = wait_file_contains(serial_log, "power:", 1)
                return fail(f"menu poweroff: QEMU exit {rc}", serial)
        except Exception:
            stop_qemu(qemu)
            raise
    return 0


def main() -> int:
    if len(sys.argv) != 3:
        print(f"usage: {sys.argv[0]} coeleo.iso sh_elf", file=sys.stderr)
        return 2
    iso, sh_elf = sys.argv[1], sys.argv[2]
    if not os.path.isfile(iso):
        print(f"test-phase16: missing ISO {iso}", file=sys.stderr)
        return 1
    for case in (case_date, case_reboot, case_poweroff, case_menu_poweroff):
        rc = case(iso, sh_elf)
        if rc != 0:
            return rc
    print("test-phase16: ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
