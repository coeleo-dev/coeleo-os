#!/usr/bin/env python3
"""Headless Fase 19: e1000e DHCP/DNS — GET by IP and hostname.

QEMU 8.2 guestfwd is TCP-only; libslirp 4.7 NATs guest DNS (10.0.2.3) to the
host resolver instead of getaddrinfo(/etc/hosts). A stub nameserver plus
LD_PRELOAD sendto/recvfrom answers A coeleo.test → 10.0.2.2 without sudo
and without an in-kernel hosts file.
"""

from __future__ import annotations

import json
import os
import shutil
import socket
import subprocess
import sys
import tempfile
import threading
import time
from http.server import BaseHTTPRequestHandler, HTTPServer

PROMPT = "coeleo>"
INKERNEL_HELP = "help: type text; Enter runs it; Backspace deletes"
BODY = "phase19-ok"
REPLY = "reply from 10.0.2.2"

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SEED_README = os.path.join(ROOT, "disk-seed", "README.TXT")
SEED_HELLO = os.path.join(ROOT, "disk-seed", "docs", "HELLO.TXT")
DNS_REDIR_C = os.path.join(os.path.dirname(os.path.abspath(__file__)), "phase19-dns-redir.c")


class Handler(BaseHTTPRequestHandler):
    def do_GET(self) -> None:
        payload = b"phase19-ok\n"
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


def parse_qname(data: bytes, off: int) -> tuple[str | None, int]:
    labels: list[str] = []
    while off < len(data):
        n = data[off]
        if n == 0:
            return ".".join(labels), off + 1
        if n & 0xC0:
            return None, off
        off += 1
        if off + n > len(data):
            return None, off
        labels.append(data[off : off + n].decode("ascii", "replace"))
        off += n
    return None, off


def dns_a_reply(query: bytes) -> bytes | None:
    if len(query) < 12:
        return None
    name, off = parse_qname(query, 12)
    if name is None or off + 4 > len(query):
        return None
    qtype = int.from_bytes(query[off : off + 2], "big")
    qclass = int.from_bytes(query[off + 2 : off + 4], "big")
    question = query[12 : off + 4]
    ident = query[:2]
    rd = query[2] & 0x01
    if name.rstrip(".").lower() != "coeleo.test" or qtype != 1 or qclass != 1:
        flags = bytes((0x80 | (rd << 0), 0x83))  # QR + NXDOMAIN
        return ident + flags + b"\x00\x01\x00\x00\x00\x00\x00\x00" + question
    flags = bytes((0x84 | rd, 0x80))  # QR AA RA
    answer = b"\xc0\x0c\x00\x01\x00\x01\x00\x00\x00\x3c\x00\x04\x0a\x00\x02\x02"
    return ident + flags + b"\x00\x01\x00\x01\x00\x00\x00\x00" + question + answer


def start_dns() -> tuple[socket.socket, int]:
    sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    sock.bind(("127.0.0.1", 0))
    port = sock.getsockname()[1]

    def loop() -> None:
        while True:
            try:
                data, addr = sock.recvfrom(512)
            except OSError:
                return
            reply = dns_a_reply(data)
            if reply:
                try:
                    sock.sendto(reply, addr)
                except OSError:
                    return

    thread = threading.Thread(target=loop, daemon=True)
    thread.start()
    return sock, port


def compile_dns_redir(tmp: str) -> str:
    if shutil.which("gcc") is None:
        raise RuntimeError("gcc not found (needed to compile phase19 DNS redirect)")
    so = os.path.join(tmp, "phase19-dns-redir.so")
    subprocess.run(
        ["gcc", "-shared", "-fPIC", "-O2", "-o", so, DNS_REDIR_C, "-ldl"],
        check=True,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.PIPE,
        text=True,
    )
    return so


def fail(msg: str, serial: str) -> int:
    print(f"test-phase19: {msg}", file=sys.stderr)
    print(serial, file=sys.stderr)
    return 1


def main() -> int:
    if len(sys.argv) != 3:
        print(f"usage: {sys.argv[0]} coeleo.iso sh_elf", file=sys.stderr)
        return 2
    iso, sh_elf = sys.argv[1], sys.argv[2]
    if not os.path.isfile(iso):
        print(f"test-phase19: missing ISO {iso}", file=sys.stderr)
        return 1
    if not os.path.isfile(DNS_REDIR_C):
        print(f"test-phase19: missing {DNS_REDIR_C}", file=sys.stderr)
        return 1

    qemu_net = "user,model=e1000e"
    if "virtio-net" in qemu_net:
        print("test-phase19: virtio-net must not be used", file=sys.stderr)
        return 1

    httpd, port = start_http()
    dns_sock, dns_port = start_dns()
    try:
        with tempfile.TemporaryDirectory(prefix="coeleo-p19-") as tmp:
            serial_log = os.path.join(tmp, "serial.log")
            qmp_path = os.path.join(tmp, "qmp.sock")
            disk = os.path.join(tmp, "disk.img")
            qemu_err = os.path.join(tmp, "qemu.err")
            make_fat_image(disk, sh_elf)
            try:
                redir_so = compile_dns_redir(tmp)
            except (RuntimeError, subprocess.CalledProcessError) as exc:
                print(f"test-phase19: DNS redirect build failed: {exc}", file=sys.stderr)
                if isinstance(exc, subprocess.CalledProcessError) and exc.stderr:
                    print(exc.stderr, file=sys.stderr)
                return 1
            serial = ""
            env = os.environ.copy()
            env["LD_PRELOAD"] = redir_so
            env["COELEO_DNS_PORT"] = str(dns_port)
            with open(qemu_err, "w", encoding="utf-8") as err_fh:
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
                        qemu_net,
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
                    stderr=err_fh,
                    env=env,
                )
            try:
                try:
                    wait_socket(qmp_path, qemu, 8)
                except RuntimeError as exc:
                    err_txt = ""
                    try:
                        with open(qemu_err, encoding="utf-8") as fh:
                            err_txt = fh.read()
                    except OSError:
                        pass
                    print(f"test-phase19: {exc}", file=sys.stderr)
                    if err_txt:
                        print(err_txt, file=sys.stderr)
                    return 1
                sock = connect_qmp(qmp_path)
                try:
                    recv_reply(sock)
                    qmp_exec(sock, {"execute": "qmp_capabilities"})
                    serial = wait_prompt_count(serial_log, 1, 12)
                    if serial.count(PROMPT) < 1:
                        return fail("timed out waiting for prompt", serial)
                    boot = serial.split(PROMPT, 1)[0]
                    if "net:" in boot:
                        return fail("boot serial contained net:", serial)
                    prompts = 1

                    type_line(sock, f"get http://10.0.2.2:{port}/")
                    serial = wait_file_contains(serial_log, BODY, 20)
                    if BODY not in serial:
                        return fail("timed out waiting for phase19-ok on IP GET", serial)
                    prompts += 1
                    serial = wait_prompt_count(serial_log, prompts, 8)
                    if serial.count(PROMPT) < prompts:
                        return fail("timed out waiting for prompt after IP GET", serial)

                    type_line(sock, f"get http://coeleo.test:{port}/")
                    deadline = time.time() + 20
                    serial = ""
                    while time.time() < deadline:
                        try:
                            with open(serial_log, "rb") as fh:
                                serial = fh.read().decode("utf-8", "replace")
                        except FileNotFoundError:
                            serial = ""
                        if serial.count(BODY) >= 2:
                            break
                        time.sleep(0.05)
                    if serial.count(BODY) < 2:
                        return fail("timed out waiting for phase19-ok on hostname GET", serial)
                    prompts += 1
                    serial = wait_prompt_count(serial_log, prompts, 8)
                    if serial.count(PROMPT) < prompts:
                        return fail("timed out waiting for prompt after hostname GET", serial)

                    type_line(sock, "ping 10.0.2.2")
                    serial = wait_file_contains(serial_log, REPLY, 25)
                    if REPLY not in serial:
                        return fail("timed out waiting for reply from 10.0.2.2", serial)
                    prompts += 1
                    serial = wait_prompt_count(serial_log, prompts, 8)
                    if serial.count(PROMPT) < prompts:
                        return fail("timed out waiting for prompt after ping 10.0.2.2", serial)

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
            if "get" not in serial or "ping" not in serial:
                return fail("ELF help did not list get/ping", serial)
            print("test-phase19: ok")
            return 0
    finally:
        httpd.shutdown()
        httpd.server_close()
        dns_sock.close()


if __name__ == "__main__":
    raise SystemExit(main())
