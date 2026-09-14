#!/usr/bin/env python3
"""Headless Fase 21: TUI editor persist + pkg hello after reboot."""

from __future__ import annotations

import json
import os
import socket
import subprocess
import sys
import tempfile
import time

PROMPT = "coeleo>"
BODY = "notes body xyz"
STATUS = "^S save"

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


def send_ctrl(sock: socket.socket, letter: str) -> None:
    send_key(
        sock,
        [
            {"type": "qcode", "data": "ctrl"},
            {"type": "qcode", "data": letter},
        ],
    )


def type_chars(sock: socket.socket, text: str) -> None:
    for ch in text:
        if "a" <= ch <= "z" or "0" <= ch <= "9":
            send_key(sock, [{"type": "qcode", "data": ch}])
        elif ch == " ":
            send_key(sock, [{"type": "qcode", "data": "spc"}])
        elif ch == "/":
            send_key(sock, [{"type": "qcode", "data": "slash"}])
        elif ch == ".":
            send_key(sock, [{"type": "qcode", "data": "dot"}])
        else:
            raise ValueError(f"unsupported qcode char {ch!r}")


def type_line(sock: socket.socket, text: str) -> None:
    type_chars(sock, text)
    send_key(sock, [{"type": "qcode", "data": "ret"}])


def read_serial(path: str) -> str:
    try:
        with open(path, "rb") as fh:
            return fh.read().decode("utf-8", "replace")
    except FileNotFoundError:
        return ""


def wait_prompt_count(path: str, n: int, timeout: float) -> str:
    deadline = time.time() + timeout
    data = ""
    while time.time() < deadline:
        data = read_serial(path)
        if data.count(PROMPT) >= n:
            return data
        time.sleep(0.05)
    return data


def wait_contains(path: str, needle: str, timeout: float) -> str:
    deadline = time.time() + timeout
    data = ""
    while time.time() < deadline:
        data = read_serial(path)
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


def make_fat_image(path: str, sh_elf: str, edit_elf: str, cat_elf: str, hello_coe: str) -> None:
    if not os.path.isfile(SEED_README):
        raise FileNotFoundError("disk-seed/README.TXT")
    for p in (sh_elf, edit_elf, cat_elf, hello_coe):
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
    subprocess.run(["mmd", "-i", path, "::bin", "::pacotes"], check=True, env=env)
    subprocess.run(["mcopy", "-i", path, sh_elf, "::sh"], check=True, env=env)
    subprocess.run(["mcopy", "-i", path, edit_elf, "::edit"], check=True, env=env)
    subprocess.run(["mcopy", "-i", path, cat_elf, "::cat"], check=True, env=env)
    subprocess.run(["mcopy", "-i", path, hello_coe, "::pacotes/hello.coe"], check=True, env=env)


def fail(msg: str, serial: str) -> int:
    print(f"test-phase21: {msg}", file=sys.stderr)
    print(serial, file=sys.stderr)
    return 1


def boot(iso: str, disk: str, serial_log: str, qmp_path: str) -> tuple[subprocess.Popen, socket.socket]:
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
    wait_socket(qmp_path, qemu, 5)
    sock = connect_qmp(qmp_path)
    recv_reply(sock)
    qmp_exec(sock, {"execute": "qmp_capabilities"})
    return qemu, sock


def qemu_quit(qemu: subprocess.Popen, sock: socket.socket) -> None:
    sock.close()
    qemu.terminate()
    try:
        qemu.wait(timeout=3)
    except subprocess.TimeoutExpired:
        qemu.kill()
        qemu.wait()


def main() -> int:
    if len(sys.argv) != 6:
        print(
            f"usage: {sys.argv[0]} coeleo.iso sh_elf edit_elf cat_elf hello.coe",
            file=sys.stderr,
        )
        return 2
    iso, sh_elf, edit_elf, cat_elf, hello_coe = sys.argv[1:6]
    if not os.path.isfile(iso):
        print(f"test-phase21: missing ISO {iso}", file=sys.stderr)
        return 1

    with tempfile.TemporaryDirectory(prefix="coeleo-p21-") as tmp:
        disk = os.path.join(tmp, "disk.img")
        make_fat_image(disk, sh_elf, edit_elf, cat_elf, hello_coe)

        serial_log = os.path.join(tmp, "serial1.log")
        qmp_path = os.path.join(tmp, "qmp1.sock")
        serial = ""
        qemu, sock = boot(iso, disk, serial_log, qmp_path)
        try:
            serial = wait_prompt_count(serial_log, 1, 12)
            if serial.count(PROMPT) < 1:
                return fail("timed out waiting for prompt", serial)
            prompts = 1

            type_line(sock, "edit notes.txt")
            serial = wait_contains(serial_log, STATUS, 8)
            if STATUS not in serial:
                return fail("editor did not start", serial)

            type_chars(sock, BODY)
            send_ctrl(sock, "s")
            time.sleep(0.2)
            send_ctrl(sock, "q")
            prompts += 1
            serial = wait_prompt_count(serial_log, prompts, 8)
            if serial.count(PROMPT) < prompts:
                return fail("timed out after edit quit", serial)

            type_line(sock, "cat notes.txt")
            prompts += 1
            serial = wait_prompt_count(serial_log, prompts, 8)
            if BODY not in serial:
                return fail("cat notes.txt missing body before reboot", serial)

            type_line(sock, "sync")
            prompts += 1
            serial = wait_prompt_count(serial_log, prompts, 5)

            type_line(sock, "pkg install /pacotes/hello.coe")
            prompts += 1
            serial = wait_prompt_count(serial_log, prompts, 20)
            if "pkg: installed hello" not in serial:
                return fail("missing pkg: installed hello", serial)

            type_line(sock, "sync")
            prompts += 1
            serial = wait_prompt_count(serial_log, prompts, 5)
        finally:
            qemu_quit(qemu, sock)

        serial_log = os.path.join(tmp, "serial2.log")
        qmp_path = os.path.join(tmp, "qmp2.sock")
        qemu, sock = boot(iso, disk, serial_log, qmp_path)
        try:
            serial = wait_prompt_count(serial_log, 1, 12)
            if serial.count(PROMPT) < 1:
                return fail("boot 2: timed out waiting for prompt", serial)
            type_line(sock, "cat notes.txt")
            serial = wait_prompt_count(serial_log, 2, 8)
            if BODY not in serial:
                return fail("boot 2 cat notes.txt missing body", serial)
            type_line(sock, "hello")
            serial = wait_prompt_count(serial_log, 3, 8)
            if "hello\nhello" not in serial.replace("\r\n", "\n") or "not found" in serial:
                return fail("boot 2 hello after pkg install missing", serial)
        finally:
            qemu_quit(qemu, sock)

        print("test-phase21: ok")
        return 0


if __name__ == "__main__":
    raise SystemExit(main())
