#!/usr/bin/env python3
"""Headless Fase 20: argv, redirect, and a two-process pipe."""

from __future__ import annotations

import json
import os
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


def send_key(sock: socket.socket, keys: list[dict]) -> None:
    qmp_exec(
        sock,
        {"execute": "send-key", "arguments": {"keys": keys}},
    )
    time.sleep(0.05)


def type_line(sock: socket.socket, text: str) -> None:
    for ch in text:
        if "a" <= ch <= "z" or "0" <= ch <= "9":
            send_key(sock, [{"type": "qcode", "data": ch}])
        elif "A" <= ch <= "Z":
            send_key(
                sock,
                [
                    {"type": "qcode", "data": "shift"},
                    {"type": "qcode", "data": ch.lower()},
                ],
            )
        elif ch == " ":
            send_key(sock, [{"type": "qcode", "data": "spc"}])
        elif ch == "/":
            send_key(sock, [{"type": "qcode", "data": "slash"}])
        elif ch == ".":
            send_key(sock, [{"type": "qcode", "data": "dot"}])
        elif ch == ">":
            send_key(
                sock,
                [
                    {"type": "qcode", "data": "shift"},
                    {"type": "qcode", "data": "dot"},
                ],
            )
        elif ch == "|":
            send_key(
                sock,
                [
                    {"type": "qcode", "data": "shift"},
                    {"type": "qcode", "data": "backslash"},
                ],
            )
        else:
            raise ValueError(f"unsupported qcode char {ch!r}")
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


def make_fat_image(path: str, sh_elf: str, hello_elf: str, cat_elf: str) -> None:
    if not os.path.isfile(SEED_README) or not os.path.isfile(SEED_HELLO):
        raise FileNotFoundError("disk-seed/README.TXT or disk-seed/docs/HELLO.TXT")
    for p in (sh_elf, hello_elf, cat_elf):
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
    subprocess.run(["mmd", "-i", path, "::docs"], check=True, env=env)
    subprocess.run(["mcopy", "-i", path, SEED_HELLO, "::docs/HELLO.TXT"], check=True, env=env)
    subprocess.run(["mcopy", "-i", path, sh_elf, "::sh"], check=True, env=env)
    subprocess.run(["mcopy", "-i", path, hello_elf, "::hello"], check=True, env=env)
    subprocess.run(["mcopy", "-i", path, cat_elf, "::cat"], check=True, env=env)


def fail(msg: str, serial: str) -> int:
    print(f"test-phase20: {msg}", file=sys.stderr)
    print(serial, file=sys.stderr)
    return 1


def main() -> int:
    if len(sys.argv) != 5:
        print(
            f"usage: {sys.argv[0]} coeleo.iso sh_elf hello_elf cat_elf",
            file=sys.stderr,
        )
        return 2
    iso, sh_elf, hello_elf, cat_elf = sys.argv[1:5]
    if not os.path.isfile(iso):
        print(f"test-phase20: missing ISO {iso}", file=sys.stderr)
        return 1

    with tempfile.TemporaryDirectory(prefix="coeleo-p20-") as tmp:
        serial_log = os.path.join(tmp, "serial.log")
        qmp_path = os.path.join(tmp, "qmp.sock")
        disk = os.path.join(tmp, "disk.img")
        make_fat_image(disk, sh_elf, hello_elf, cat_elf)
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
                prompts = 1

                type_line(sock, "hello")
                prompts += 1
                serial = wait_prompt_count(serial_log, prompts, 8)
                if "hello" not in serial:
                    return fail("hello did not print hello", serial)

                type_line(sock, "hello world")
                prompts += 1
                serial = wait_prompt_count(serial_log, prompts, 8)
                if "hello" not in serial or "world" not in serial:
                    return fail("hello world did not print both tokens", serial)

                type_line(sock, "cat README.TXT > out.txt")
                prompts += 1
                serial = wait_prompt_count(serial_log, prompts, 8)
                type_line(sock, "cat out.txt")
                prompts += 1
                serial = wait_prompt_count(serial_log, prompts, 8)
                if README_BODY not in serial:
                    return fail("cat README.TXT > out.txt then cat out.txt missing body", serial)

                before = serial.count(README_BODY)
                type_line(sock, "cat README.TXT | cat")
                prompts += 1
                serial = wait_prompt_count(serial_log, prompts, 25)
                if serial.count(PROMPT) < prompts:
                    return fail("timed out after pipe", serial)
                added = serial.count(README_BODY) - before
                if added != 1:
                    return fail(
                        f"pipe did not print README body once (delta {added})",
                        serial,
                    )

                type_line(sock, "help")
                prompts += 1
                serial = wait_prompt_count(serial_log, prompts, 5)
                if INKERNEL_HELP in serial:
                    return fail("in-kernel help on serial; expected ELF sh", serial)
                if "get" not in serial or "ping" not in serial:
                    return fail("help did not list get/ping", serial)
            finally:
                sock.close()
        finally:
            qemu.terminate()
            try:
                qemu.wait(timeout=3)
            except subprocess.TimeoutExpired:
                qemu.kill()
                qemu.wait()

        print("test-phase20: ok")
        return 0


if __name__ == "__main__":
    raise SystemExit(main())
