#!/usr/bin/env python3
"""Headless Fase 20b: shell history (Up) and Tab complete."""

from __future__ import annotations

import json
import os
import socket
import subprocess
import sys
import tempfile
import time

PROMPT = "coeleo>"
INKERNEL_HELP = "help: type text; Enter runs it; Backspace deletes"

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SEED_README = os.path.join(ROOT, "disk-seed", "README.TXT")


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


def send_key(sock: socket.socket, keys: list[dict]) -> None:
    qmp_exec(
        sock,
        {"execute": "send-key", "arguments": {"keys": keys}},
    )
    time.sleep(0.05)


def type_chars(sock: socket.socket, text: str) -> None:
    for ch in text:
        if "a" <= ch <= "z" or "0" <= ch <= "9":
            send_key(sock, [{"type": "qcode", "data": ch}])
        elif ch == " ":
            send_key(sock, [{"type": "qcode", "data": "spc"}])
        else:
            raise ValueError(f"unsupported qcode char {ch!r}")


def type_line(sock: socket.socket, text: str) -> None:
    type_chars(sock, text)
    send_key(sock, [{"type": "qcode", "data": "ret"}])


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


def make_fat_image(path: str, sh_elf: str, hello_elf: str) -> None:
    if not os.path.isfile(SEED_README):
        raise FileNotFoundError("disk-seed/README.TXT")
    for p in (sh_elf, hello_elf):
        if not os.path.isfile(p):
            raise FileNotFoundError(p)
    env = os.environ.copy()
    env["MTOOLS_SKIP_CHECK"] = "1"
    subprocess.run(
        ["dd", "if=/dev/zero", f"of={path}", "bs=1M", "count=64", "status=none"],
        check=True,
    )
    subprocess.run(["mformat", "-i", path, "-F", "-c", "1", "-v", "COELEO", "::"], check=True, env=env)
    subprocess.run(["mcopy", "-i", path, SEED_README, "::README.TXT"], check=True, env=env)
    subprocess.run(["mcopy", "-i", path, sh_elf, "::sh"], check=True, env=env)
    subprocess.run(["mcopy", "-i", path, hello_elf, "::hello"], check=True, env=env)


def fail(msg: str, serial: str) -> int:
    print(f"test-phase20b: {msg}", file=sys.stderr)
    print(serial, file=sys.stderr)
    return 1


def main() -> int:
    if len(sys.argv) != 4:
        print(f"usage: {sys.argv[0]} coeleo.iso sh_elf hello_elf", file=sys.stderr)
        return 2
    iso, sh_elf, hello_elf = sys.argv[1:4]
    if not os.path.isfile(iso):
        print(f"test-phase20b: missing ISO {iso}", file=sys.stderr)
        return 1

    with tempfile.TemporaryDirectory(prefix="coeleo-p20b-") as tmp:
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
                serial = wait_prompt_count(serial_log, 1, 12)
                if serial.count(PROMPT) < 1:
                    return fail("timed out waiting for prompt", serial)
                if "pipe:" in serial:
                    return fail("new pipe boot line on serial", serial)
                if INKERNEL_HELP in serial:
                    return fail("in-kernel help on serial at boot", serial)
                prompts = 1

                type_line(sock, "ls")
                prompts += 1
                serial = wait_prompt_count(serial_log, prompts, 8)
                if serial.count(PROMPT) < prompts:
                    return fail("timed out after ls", serial)
                if "README.TXT" not in serial:
                    return fail("ls did not print README.TXT", serial)
                n0 = serial.count("README.TXT")

                send_key(sock, [{"type": "qcode", "data": "up"}])
                send_key(sock, [{"type": "qcode", "data": "ret"}])
                prompts += 1
                serial = wait_prompt_count(serial_log, prompts, 8)
                if serial.count(PROMPT) < prompts:
                    return fail("timed out after history Up+Enter", serial)
                if serial.count("README.TXT") != n0 + 1:
                    return fail(
                        f"Up+Enter did not run ls again (README.TXT {serial.count('README.TXT')} want {n0 + 1})",
                        serial,
                    )

                type_chars(sock, "he")
                send_key(sock, [{"type": "qcode", "data": "tab"}])
                send_key(sock, [{"type": "qcode", "data": "ret"}])
                prompts += 1
                serial = wait_prompt_count(serial_log, prompts, 8)
                if serial.count(PROMPT) < prompts:
                    return fail("timed out after he+Tab+Enter", serial)
                if "not found" in serial:
                    return fail("he+Tab ran a missing command", serial)
                if "hello" not in serial:
                    return fail("he+Tab+Enter did not run hello", serial)

                type_line(sock, "help")
                prompts += 1
                serial = wait_prompt_count(serial_log, prompts, 5)
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

        print("test-phase20b: ok")
        return 0


if __name__ == "__main__":
    raise SystemExit(main())
