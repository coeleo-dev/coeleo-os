#!/usr/bin/env python3
"""Headless Plasma P6: Alt+Space KRunner prefix filter launches hello."""

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


def send_alt_space(sock: socket.socket) -> None:
    qmp_exec(
        sock,
        {
            "execute": "send-key",
            "arguments": {
                "keys": [
                    {"type": "qcode", "data": "alt"},
                    {"type": "qcode", "data": "spc"},
                ]
            },
        },
    )


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


def make_fat_image(path: str, sh_elf: str, hello_elf: str) -> None:
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
    subprocess.run(["mcopy", "-i", path, hello_elf, "::hello"], check=True, env=env)


def fail(msg: str, serial: str) -> int:
    print(f"test-plasma-p6: {msg}", file=sys.stderr)
    print(serial, file=sys.stderr)
    return 1


def main() -> int:
    if len(sys.argv) != 4:
        print(f"usage: {sys.argv[0]} coeleo.iso sh_elf hello_elf", file=sys.stderr)
        return 2
    iso, sh_elf, hello_elf = sys.argv[1], sys.argv[2], sys.argv[3]
    if not os.path.isfile(iso):
        print(f"test-plasma-p6: missing ISO {iso}", file=sys.stderr)
        return 1

    with tempfile.TemporaryDirectory(prefix="coeleo-p6-") as tmp:
        serial_log = os.path.join(tmp, "serial.log")
        qmp_path = os.path.join(tmp, "qmp.sock")
        disk = os.path.join(tmp, "disk.img")
        make_fat_image(disk, sh_elf, hello_elf)
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

                send_alt_space(sock)
                serial = wait_file_contains(serial_log, "krunner: open", 5)
                if "krunner: open" not in serial:
                    return fail("missing krunner: open after Alt+Space", serial)
                if "run: hello" in serial:
                    return fail("spawned hello before typing", serial)

                send_keys(sock, ["h", "e", "l", "l", "o", "ret"])
                serial = wait_file_contains(serial_log, "run: hello", 8)
                if "run: hello" not in serial:
                    return fail("missing run: hello", serial)
                serial = wait_count(serial_log, "hello", 2, 8)
                if serial.count("hello") < 2:
                    return fail("missing hello ELF stdout", serial)
                serial = wait_file_contains(serial_log, "krunner: close", 5)
                if "krunner: close" not in serial:
                    return fail("missing krunner: close after Enter", serial)
                if serial.count("run: hello") != 1:
                    return fail("expected one run: hello after first launch", serial)

                send_alt_space(sock)
                serial = wait_count(serial_log, "krunner: open", 2, 5)
                if serial.count("krunner: open") < 2:
                    return fail("missing second krunner: open", serial)
                send_keys(sock, ["esc"])
                serial = wait_count(serial_log, "krunner: close", 2, 5)
                if serial.count("krunner: close") < 2:
                    return fail("missing krunner: close after Esc", serial)
                if serial.count("run: hello") != 1:
                    return fail("Esc launched a second hello", serial)
            finally:
                sock.close()
        finally:
            qemu.terminate()
            try:
                qemu.wait(timeout=3)
            except subprocess.TimeoutExpired:
                qemu.kill()
                qemu.wait()

        print("test-plasma-p6: ok")
        return 0


if __name__ == "__main__":
    raise SystemExit(main())
