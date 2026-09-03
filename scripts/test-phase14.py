#!/usr/bin/env python3
"""Headless Fase 14: pkg install/remove, ed25519, PATH /bin."""

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


def after_command(serial: str, cmd: str) -> str:
    i = serial.rfind(cmd)
    if i < 0:
        return ""
    return serial[i + len(cmd) :]


def norm(s: str) -> str:
    return s.replace("\r\n", "\n").replace("\r", "\n")


def make_fat_image(path: str, sh_elf: str, hello_coe: str, bad_coe: str) -> None:
    if not os.path.isfile(SEED_README) or not os.path.isfile(SEED_HELLO):
        raise FileNotFoundError("disk-seed/README.TXT or disk-seed/docs/HELLO.TXT")
    for p in (sh_elf, hello_coe, bad_coe):
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
    subprocess.run(["mmd", "-i", path, "::docs", "::bin", "::pacotes"], check=True, env=env)
    subprocess.run(["mcopy", "-i", path, SEED_HELLO, "::docs/HELLO.TXT"], check=True, env=env)
    subprocess.run(["mcopy", "-i", path, sh_elf, "::sh"], check=True, env=env)
    subprocess.run(["mcopy", "-i", path, hello_coe, "::pacotes/hello.coe"], check=True, env=env)
    subprocess.run(["mcopy", "-i", path, bad_coe, "::pacotes/bad.coe"], check=True, env=env)


def fail(msg: str, serial: str) -> int:
    print(f"test-phase14: {msg}", file=sys.stderr)
    print(serial, file=sys.stderr)
    return 1


def main() -> int:
    if len(sys.argv) != 5:
        print(
            f"usage: {sys.argv[0]} coeleo.iso sh_elf hello.coe bad.coe",
            file=sys.stderr,
        )
        return 2
    iso, sh_elf, hello_coe, bad_coe = sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4]
    if not os.path.isfile(iso):
        print(f"test-phase14: missing ISO {iso}", file=sys.stderr)
        return 1

    with tempfile.TemporaryDirectory(prefix="coeleo-p14-") as tmp:
        serial_log = os.path.join(tmp, "serial.log")
        qmp_path = os.path.join(tmp, "qmp.sock")
        disk = os.path.join(tmp, "disk.img")
        make_fat_image(disk, sh_elf, hello_coe, bad_coe)
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
                serial = wait_prompt_count(serial_log, 1, 20)
                if serial.count(PROMPT) < 1:
                    return fail("timed out waiting for prompt", serial)
                prompts = 1

                type_line(sock, "hello")
                prompts += 1
                serial = wait_prompt_count(serial_log, prompts, 8)
                serial_n = norm(serial)
                if "not found" not in after_command(serial_n, "hello"):
                    return fail("hello before install did not print not found", serial)
                if "hello\nhello" in serial_n:
                    return fail("hello ELF ran before install", serial)

                type_line(sock, "pkg install /pacotes/bad.coe")
                prompts += 1
                serial = wait_prompt_count(serial_log, prompts, 12)
                if "pkg: bad signature" not in serial:
                    return fail("missing pkg: bad signature", serial)

                type_line(sock, "ls /bin")
                prompts += 1
                serial = wait_prompt_count(serial_log, prompts, 8)
                listing = after_command(norm(serial), "ls /bin")
                if "hello" in listing.split(PROMPT)[0]:
                    return fail("bad.coe wrote /bin/hello", serial)

                type_line(sock, "pkg install /pacotes/hello.coe")
                prompts += 1
                serial = wait_prompt_count(serial_log, prompts, 20)
                if "pkg: installed hello" not in serial:
                    return fail("missing pkg: installed hello", serial)

                type_line(sock, "hello")
                prompts += 1
                serial = wait_prompt_count(serial_log, prompts, 8)
                if "hello\nhello" not in norm(serial):
                    return fail("hello after install did not run /bin/hello", serial)

                type_line(sock, "pkg remove hello")
                prompts += 1
                serial = wait_prompt_count(serial_log, prompts, 8)
                if "pkg: removed hello" not in serial:
                    return fail("missing pkg: removed hello", serial)

                type_line(sock, "hello")
                prompts += 1
                serial = wait_prompt_count(serial_log, prompts, 8)
                tail = after_command(norm(serial), "hello")
                if "not found" not in tail:
                    return fail("hello after remove did not print not found", serial)
            finally:
                sock.close()
        finally:
            qemu.terminate()
            try:
                qemu.wait(timeout=3)
            except subprocess.TimeoutExpired:
                qemu.kill()
                qemu.wait()

        print("test-phase14: ok")
        return 0


if __name__ == "__main__":
    raise SystemExit(main())
