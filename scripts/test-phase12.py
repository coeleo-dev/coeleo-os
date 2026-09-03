#!/usr/bin/env python3
"""Headless Fase 12: QMP get http://10.0.2.2:<port>/ against a host HTTP server."""

from __future__ import annotations

import json
import os
import socket
import subprocess
import sys
import tempfile
import threading
import time
from http.server import BaseHTTPRequestHandler, HTTPServer

PROMPT = "coeleo>"
INKERNEL_HELP = "help: type text; Enter runs it; Backspace deletes"
BODY = "phase12-ok"
MISSING = "get: missing url"
BAD_URL = "get: bad url"
HTTPS = "get: https not supported"

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SEED_README = os.path.join(ROOT, "disk-seed", "README.TXT")
SEED_HELLO = os.path.join(ROOT, "disk-seed", "docs", "HELLO.TXT")


class Handler(BaseHTTPRequestHandler):
    def do_GET(self) -> None:
        payload = b"phase12-ok\n"
        self.send_response(200)
        self.send_header("Content-Type", "text/plain")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

    def log_message(self, _fmt: str, *_args: object) -> None:
        return


def start_http() -> tuple[HTTPServer, int]:
    server = HTTPServer(("127.0.0.1", 0), Handler)
    port = server.server_address[1]
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    return server, port


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
    qmp_exec(sock, {"execute": "send-key", "arguments": {"keys": keys}})
    time.sleep(0.05)


def type_line(sock: socket.socket, text: str) -> None:
    for ch in text:
        if "a" <= ch <= "z" or "0" <= ch <= "9":
            send_key(sock, [{"type": "qcode", "data": ch}])
        elif ch == " ":
            send_key(sock, [{"type": "qcode", "data": "spc"}])
        elif ch == ".":
            send_key(sock, [{"type": "qcode", "data": "dot"}])
        elif ch == "-":
            send_key(sock, [{"type": "qcode", "data": "minus"}])
        elif ch == "/":
            send_key(sock, [{"type": "qcode", "data": "slash"}])
        elif ch == ":":
            send_key(
                sock,
                [
                    {"type": "qcode", "data": "shift"},
                    {"type": "qcode", "data": "semicolon"},
                ],
            )
        else:
            raise ValueError(f"unsupported qcode char {ch!r}")
    send_key(sock, [{"type": "qcode", "data": "ret"}])


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
    print(f"test-phase12: {msg}", file=sys.stderr)
    print(serial, file=sys.stderr)
    return 1


def main() -> int:
    if len(sys.argv) != 3:
        print(f"usage: {sys.argv[0]} coeleo.iso sh_elf", file=sys.stderr)
        return 2
    iso, sh_elf = sys.argv[1], sys.argv[2]
    if not os.path.isfile(iso):
        print(f"test-phase12: missing ISO {iso}", file=sys.stderr)
        return 1

    httpd, port = start_http()
    try:
        with tempfile.TemporaryDirectory(prefix="coeleo-p12-") as tmp:
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
                    "-nic",
                    "user,model=virtio-net-pci",
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
                    prompts = 1

                    type_line(sock, "get")
                    prompts += 1
                    serial = wait_prompt_count(serial_log, prompts, 5)
                    if serial.count(PROMPT) < prompts:
                        return fail("timed out after get", serial)
                    if MISSING not in serial:
                        return fail("get without url did not print missing url", serial)

                    type_line(sock, "get not-a-url")
                    prompts += 1
                    serial = wait_prompt_count(serial_log, prompts, 5)
                    if serial.count(PROMPT) < prompts:
                        return fail("timed out after get not-a-url", serial)
                    if BAD_URL not in serial:
                        return fail("get not-a-url did not print bad url", serial)

                    type_line(sock, "get https://10.0.2.2/")
                    prompts += 1
                    serial = wait_prompt_count(serial_log, prompts, 5)
                    if serial.count(PROMPT) < prompts:
                        return fail("timed out after get https", serial)
                    if HTTPS not in serial:
                        return fail("get https did not print https not supported", serial)

                    type_line(sock, f"get http://10.0.2.2:{port}/")
                    serial = wait_file_contains(serial_log, BODY, 15)
                    if BODY not in serial:
                        return fail("timed out waiting for phase12-ok", serial)
                    prompts += 1
                    serial = wait_prompt_count(serial_log, prompts, 8)
                    if serial.count(PROMPT) < prompts:
                        return fail("timed out waiting for prompt after get", serial)

                    type_line(sock, "help")
                    prompts += 1
                    serial = wait_prompt_count(serial_log, prompts, 5)
                finally:
                    sock.close()
            finally:
                qemu.terminate()
                try:
                    qemu.wait(timeout=3)
                except subprocess.TimeoutExpired:
                    qemu.kill()
                    qemu.wait()

            if INKERNEL_HELP in serial:
                return fail("in-kernel help line appeared under ELF sh", serial)
            if "get" not in serial:
                return fail("ELF help did not list get", serial)
            print("test-phase12: ok")
            return 0
    finally:
        httpd.shutdown()
        httpd.server_close()


if __name__ == "__main__":
    raise SystemExit(main())
