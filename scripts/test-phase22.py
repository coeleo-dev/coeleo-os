#!/usr/bin/env python3
"""Headless Phase 22: TLS get https://10.0.2.2:<port>/ and pkg install http://..."""

from __future__ import annotations

import json
import os
import socket
import ssl
import subprocess
import sys
import tempfile
import threading
import time
from http.server import BaseHTTPRequestHandler, HTTPServer

PROMPT = "coeleo>"
INKERNEL_HELP = "help: type text; Enter runs it; Backspace deletes"
BODY_OK = "phase22-tls-ok"
BODY_UNTRUSTED = "untrusted-body"
TLS_FAIL = "get: tls"

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SEED_README = os.path.join(ROOT, "disk-seed", "README.TXT")
SEED_HELLO = os.path.join(ROOT, "disk-seed", "docs", "HELLO.TXT")
TLS_CA_DIR = os.path.join(ROOT, "scripts", "tls_ca")
VALID_CERT = os.path.join(TLS_CA_DIR, "valid.pem")
VALID_KEY = os.path.join(TLS_CA_DIR, "valid.key")
UNTRUSTED_CERT = os.path.join(TLS_CA_DIR, "untrusted.pem")
UNTRUSTED_KEY = os.path.join(TLS_CA_DIR, "untrusted.key")


class HttpHandler(BaseHTTPRequestHandler):
    hello_bytes: bytes = b""
    bad_bytes: bytes = b""

    def do_GET(self) -> None:
        if self.path == "/hello.coe":
            payload = self.hello_bytes
        elif self.path == "/bad.coe":
            payload = self.bad_bytes
        else:
            self.send_response(404)
            self.end_headers()
            return
        self.send_response(200)
        self.send_header("Content-Type", "application/octet-stream")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

    def log_message(self, _fmt: str, *_args: object) -> None:
        return


class HttpsValidHandler(BaseHTTPRequestHandler):
    def do_GET(self) -> None:
        payload = b"phase22-tls-ok\n"
        self.send_response(200)
        self.send_header("Content-Type", "text/plain")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

    def log_message(self, _fmt: str, *_args: object) -> None:
        return


class HttpsUntrustedHandler(BaseHTTPRequestHandler):
    def do_GET(self) -> None:
        payload = b"untrusted-body\n"
        self.send_response(200)
        self.send_header("Content-Type", "text/plain")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

    def log_message(self, _fmt: str, *_args: object) -> None:
        return


def start_http(hello_path: str, bad_path: str) -> tuple[HTTPServer, int]:
    with open(hello_path, "rb") as fh:
        HttpHandler.hello_bytes = fh.read()
    with open(bad_path, "rb") as fh:
        HttpHandler.bad_bytes = fh.read()

    server = HTTPServer(("127.0.0.1", 0), HttpHandler)
    port = server.server_address[1]
    thread = threading.Thread(target=server.serve_forever, daemon=True)
    thread.start()
    return server, port


def start_https(handler_cls: type[BaseHTTPRequestHandler], cert: str, key: str) -> tuple[HTTPServer, int]:
    server = HTTPServer(("127.0.0.1", 0), handler_cls)
    ctx = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    ctx.load_cert_chain(certfile=cert, keyfile=key)
    server.socket = ctx.wrap_socket(server.socket, server_side=True)
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
    subprocess.run(["mmd", "-i", path, "::bin"], check=True, env=env)
    subprocess.run(["mcopy", "-i", path, SEED_HELLO, "::docs/HELLO.TXT"], check=True, env=env)
    subprocess.run(["mcopy", "-i", path, sh_elf, "::sh"], check=True, env=env)


def fail(msg: str, serial: str) -> int:
    print(f"test-phase22: {msg}", file=sys.stderr)
    print(serial, file=sys.stderr)
    return 1


def main() -> int:
    if len(sys.argv) != 5:
        print(f"usage: {sys.argv[0]} coeleo.iso sh_elf hello.coe bad.coe", file=sys.stderr)
        return 2
    iso, sh_elf, hello_coe, bad_coe = sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4]
    for p in (iso, sh_elf, hello_coe, bad_coe):
        if not os.path.isfile(p):
            print(f"test-phase22: missing file {p}", file=sys.stderr)
            return 1

    httpd, http_port = start_http(hello_coe, bad_coe)
    valid_httpsd, valid_port = start_https(HttpsValidHandler, VALID_CERT, VALID_KEY)
    untrusted_httpsd, untrusted_port = start_https(HttpsUntrustedHandler, UNTRUSTED_CERT, UNTRUSTED_KEY)

    try:
        with tempfile.TemporaryDirectory(prefix="coeleo-p22-") as tmp:
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
                    serial = wait_prompt_count(serial_log, 1, 15)
                    if serial.count(PROMPT) < 1:
                        return fail("timed out waiting for initial prompt", serial)
                    prompts = 1

                    # 1. Test HTTPS GET with valid certificate signed by test CA
                    type_line(sock, f"get https://10.0.2.2:{valid_port}/")
                    serial = wait_file_contains(serial_log, BODY_OK, 15)
                    if BODY_OK not in serial:
                        return fail("timed out waiting for phase22-tls-ok", serial)
                    prompts += 1
                    serial = wait_prompt_count(serial_log, prompts, 8)
                    if serial.count(PROMPT) < prompts:
                        return fail("timed out waiting for prompt after valid https get", serial)

                    # 2. Test HTTPS GET with untrusted certificate
                    type_line(sock, f"get https://10.0.2.2:{untrusted_port}/")
                    serial = wait_file_contains(serial_log, TLS_FAIL, 15)
                    if TLS_FAIL not in serial:
                        return fail("untrusted https did not fail with get: tls", serial)
                    if BODY_UNTRUSTED in serial:
                        return fail("untrusted https leaked body into output", serial)
                    prompts += 1
                    serial = wait_prompt_count(serial_log, prompts, 8)
                    if serial.count(PROMPT) < prompts:
                        return fail("timed out waiting for prompt after untrusted https get", serial)

                    # 3. Test pkg install of bad signature package over HTTP
                    type_line(sock, f"pkg install http://10.0.2.2:{http_port}/bad.coe")
                    serial = wait_file_contains(serial_log, "pkg: bad signature", 15)
                    if "pkg: bad signature" not in serial:
                        return fail("pkg install of bad.coe was not refused", serial)
                    prompts += 1
                    serial = wait_prompt_count(serial_log, prompts, 8)
                    if serial.count(PROMPT) < prompts:
                        return fail("timed out waiting for prompt after bad pkg install", serial)

                    # 4. Test pkg install of valid package over HTTP
                    type_line(sock, f"pkg install http://10.0.2.2:{http_port}/hello.coe")
                    serial = wait_file_contains(serial_log, "pkg: installed hello", 15)
                    if "pkg: installed hello" not in serial:
                        return fail("pkg install of hello.coe failed", serial)
                    prompts += 1
                    serial = wait_prompt_count(serial_log, prompts, 8)
                    if serial.count(PROMPT) < prompts:
                        return fail("timed out waiting for prompt after hello pkg install", serial)

                    # 5. Run installed hello
                    type_line(sock, "hello")
                    serial = wait_file_contains(serial_log, "hello from userspace", 10)
                    if "hello from userspace" not in serial:
                        return fail("installed hello did not run", serial)
                    prompts += 1
                    serial = wait_prompt_count(serial_log, prompts, 8)
                    if serial.count(PROMPT) < prompts:
                        return fail("timed out waiting for prompt after running hello", serial)

                    # 6. Test pkg update / reinstall
                    type_line(sock, f"pkg update http://10.0.2.2:{http_port}/hello.coe")
                    serial = wait_file_contains(serial_log, "pkg: installed hello", 10)
                    if serial.count("pkg: installed hello") < 2:
                        return fail("pkg update did not replace/install package", serial)
                    prompts += 1
                    serial = wait_prompt_count(serial_log, prompts, 8)
                    if serial.count(PROMPT) < prompts:
                        return fail("timed out waiting for prompt after pkg update", serial)

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
            print("test-phase22: ok")
            return 0
    finally:
        httpd.shutdown()
        httpd.server_close()
        valid_httpsd.shutdown()
        valid_httpsd.server_close()
        untrusted_httpsd.shutdown()
        untrusted_httpsd.server_close()


if __name__ == "__main__":
    raise SystemExit(main())
