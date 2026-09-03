#!/usr/bin/env python3
"""Headless Fase 17.2: install GPT+ESP on the spare AHCI disk; OVMF boots the target.

Live seed is the 17.1 GPT-FAT (type 0700) plus the all-hdd tree the engine copies:
kernel, limine.conf, limine-bios.sys, BOOTX64.EFI, BOOTIA32.EFI, README, docs, sh.
"""

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

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
SEED_README = os.path.join(ROOT, "disk-seed", "README.TXT")
SEED_HELLO = os.path.join(ROOT, "disk-seed", "docs", "HELLO.TXT")
SH_ELF = os.path.join(ROOT, "userspace", "apps", "sh", "sh")
KERNEL = os.path.join(ROOT, "kernel", "kernel")
LIMINE_CONF = os.path.join(ROOT, "limine.conf")
LIMINE_BIOS = os.path.join(ROOT, "limine", "limine-bios.sys")
BOOTX64 = os.path.join(ROOT, "limine", "BOOTX64.EFI")
BOOTIA32 = os.path.join(ROOT, "limine", "BOOTIA32.EFI")
OVMF = os.path.join(ROOT, "edk2-ovmf", "ovmf-code-x86_64.fd")


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


def mtools_env() -> dict[str, str]:
    env = os.environ.copy()
    env["MTOOLS_SKIP_CHECK"] = "1"
    return env


def require_file(path: str) -> None:
    if not os.path.isfile(path):
        raise FileNotFoundError(path)


def make_live_image(path: str) -> None:
    """17.1 GPT-FAT 0700 plus the all-hdd payload install copies."""
    require_file(SEED_README)
    require_file(SEED_HELLO)
    require_file(SH_ELF)
    require_file(KERNEL)
    require_file(LIMINE_CONF)
    require_file(LIMINE_BIOS)
    require_file(BOOTX64)
    env = mtools_env()
    subprocess.run(
        ["dd", "if=/dev/zero", f"of={path}", "bs=1M", "count=64", "status=none"],
        check=True,
    )
    subprocess.run(["sgdisk", path, "-n", "1:2048", "-t", "1:0700"], check=True, capture_output=True)
    subprocess.run(
        ["mformat", "-i", f"{path}@@1M", "-F", "-c", "1", "-v", "COELEO", "::"],
        check=True,
        env=env,
    )
    subprocess.run(
        ["mmd", "-i", f"{path}@@1M", "::/boot", "::/boot/limine", "::/EFI", "::/EFI/BOOT", "::docs"],
        check=True,
        env=env,
    )
    subprocess.run(["mcopy", "-i", f"{path}@@1M", KERNEL, "::/boot/kernel"], check=True, env=env)
    subprocess.run(["mcopy", "-i", f"{path}@@1M", LIMINE_CONF, "::/boot/limine/limine.conf"], check=True, env=env)
    subprocess.run(["mcopy", "-i", f"{path}@@1M", LIMINE_BIOS, "::/boot/limine/limine-bios.sys"], check=True, env=env)
    subprocess.run(["mcopy", "-i", f"{path}@@1M", BOOTX64, "::/EFI/BOOT/BOOTX64.EFI"], check=True, env=env)
    if os.path.isfile(BOOTIA32):
        subprocess.run(
            ["mcopy", "-i", f"{path}@@1M", BOOTIA32, "::/EFI/BOOT/BOOTIA32.EFI"],
            check=True,
            env=env,
        )
    subprocess.run(["mcopy", "-i", f"{path}@@1M", SEED_README, "::README.TXT"], check=True, env=env)
    subprocess.run(["mcopy", "-i", f"{path}@@1M", SEED_HELLO, "::docs/HELLO.TXT"], check=True, env=env)
    subprocess.run(["mcopy", "-i", f"{path}@@1M", SH_ELF, "::sh"], check=True, env=env)


def make_blank_image(path: str) -> None:
    subprocess.run(
        ["dd", "if=/dev/zero", f"of={path}", "bs=1M", "count=64", "status=none"],
        check=True,
    )


def qemu_quit(qemu: subprocess.Popen, sock: socket.socket | None) -> None:
    if sock is not None:
        try:
            qmp_exec(sock, {"execute": "quit"})
        except (OSError, EOFError, RuntimeError):
            try:
                sock.sendall(b'{"execute":"quit"}\n')
            except OSError:
                pass
        try:
            sock.close()
        except OSError:
            pass
        try:
            qemu.wait(timeout=15)
            return
        except subprocess.TimeoutExpired:
            pass
    if qemu.poll() is None:
        qemu.kill()
        qemu.wait()


def ahci_disk_lines(serial: str) -> list[str]:
    return [
        ln.strip()
        for ln in serial.splitlines()
        if ln.strip().startswith("disk:") and "ahci" in ln and "MiB" in ln
    ]


def sgdisk_info(path: str, part: int = 1) -> str:
    r = subprocess.run(["sgdisk", "-i", str(part), path], capture_output=True, text=True)
    return r.stdout + r.stderr


def boot_live_and_target(iso: str, live: str, blank: str) -> tuple[subprocess.Popen, socket.socket, str]:
    work = os.path.dirname(os.path.abspath(live))
    nonce = str(time.time_ns())
    serial_log = os.path.join(work, f"serial-{nonce}.log")
    qmp_path = os.path.join(work, f"qmp-{nonce}.sock")
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
            f"file={live},if=none,format=raw,id=live",
            "-device",
            "ide-hd,bus=ide.0,drive=live",
            "-drive",
            f"file={blank},if=none,format=raw,id=tgt",
            "-device",
            "ide-hd,bus=ide.1,drive=tgt",
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
    sock: socket.socket | None = None
    try:
        wait_socket(qmp_path, qemu, 5)
        sock = connect_qmp(qmp_path)
        recv_reply(sock)
        qmp_exec(sock, {"execute": "qmp_capabilities"})
        serial = wait_count(serial_log, PROMPT, 1, 12)
        if serial.count(PROMPT) < 1:
            raise RuntimeError("timed out waiting for prompt")
        return qemu, sock, serial_log
    except Exception:
        qemu_quit(qemu, sock)
        raise


def boot_target_ovmf(ovmf: str, target: str) -> tuple[subprocess.Popen, socket.socket, str]:
    work = os.path.dirname(os.path.abspath(target))
    nonce = str(time.time_ns())
    serial_log = os.path.join(work, f"ovmf-serial-{nonce}.log")
    qmp_path = os.path.join(work, f"ovmf-qmp-{nonce}.sock")
    qemu = subprocess.Popen(
        [
            "qemu-system-x86_64",
            "-M",
            "q35",
            "-m",
            "512M",
            "-drive",
            f"if=pflash,unit=0,format=raw,file={ovmf},readonly=on",
            "-drive",
            f"file={target},if=none,format=raw,id=tgt",
            "-device",
            "ide-hd,bus=ide.0,drive=tgt,bootindex=1",
            "-net",
            "none",
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
    sock: socket.socket | None = None
    try:
        wait_socket(qmp_path, qemu, 8)
        sock = connect_qmp(qmp_path)
        recv_reply(sock)
        qmp_exec(sock, {"execute": "qmp_capabilities"})
        return qemu, sock, serial_log
    except Exception:
        qemu_quit(qemu, sock)
        raise


def fail(msg: str, serial: str) -> int:
    print(f"test-phase17.2: {msg}", file=sys.stderr)
    print(serial, file=sys.stderr)
    return 1


def main() -> int:
    if len(sys.argv) != 2:
        print(f"usage: {sys.argv[0]} coeleo.iso", file=sys.stderr)
        return 2
    iso = sys.argv[1]
    if not os.path.isfile(iso):
        print(f"test-phase17.2: missing ISO {iso}", file=sys.stderr)
        return 1
    if not os.path.isfile(OVMF):
        print(f"test-phase17.2: missing OVMF {OVMF}", file=sys.stderr)
        return 1

    with tempfile.TemporaryDirectory(prefix="coeleo-p17.2-") as tmp:
        live = os.path.join(tmp, "live.img")
        blank = os.path.join(tmp, "blank.img")
        make_live_image(live)
        make_blank_image(blank)

        qemu, sock, serial_log = boot_live_and_target(iso, live, blank)
        serial = ""
        try:
            prompts = 1

            type_line(sock, "disk")
            prompts += 1
            serial = wait_count(serial_log, PROMPT, prompts, 8)
            rows = ahci_disk_lines(serial)
            if len(rows) < 2:
                return fail("disk did not list two ahci rows", serial)

            type_line(sock, "install")
            prompts += 1
            serial = wait_count(serial_log, PROMPT, prompts, 8)
            if "install: refuse" not in serial:
                return fail("install without args did not refuse", serial)
            if "install: refuse live-root" in serial:
                return fail("install without args used live-root token", serial)

            type_line(sock, "install 0")
            prompts += 1
            serial = wait_count(serial_log, PROMPT, prompts, 8)
            if "install: refuse live-root" not in serial:
                return fail("install 0 did not refuse live-root", serial)

            type_line(sock, "install 1")
            prompts += 1
            serial = wait_count(serial_log, "install: confirm", 1, 8)
            serial = wait_count(serial_log, PROMPT, prompts, 8)
            if "install: confirm" not in serial:
                return fail("first install 1 did not confirm", serial)
            if "install: ok" in serial:
                return fail("first install 1 wrote without confirm", serial)

            type_line(sock, "install 1")
            serial = wait_count(serial_log, "install: ok", 1, 180)
            if "install: failed" in serial:
                return fail("install 1 failed", serial)
            if "install: ok" not in serial:
                return fail("second install 1 did not finish", serial)
            prompts += 1
            serial = wait_count(serial_log, PROMPT, prompts, 8)
        finally:
            qemu_quit(qemu, sock)

        live_gpt = sgdisk_info(live)
        if "Microsoft basic data" not in live_gpt:
            return fail("live image GPT was rewritten", live_gpt)

        tgt_gpt = sgdisk_info(blank)
        if "EFI system" not in tgt_gpt and "EF00" not in tgt_gpt:
            return fail("target is not ESP (EF00)", tgt_gpt)

        env = mtools_env()
        listing = subprocess.run(
            ["mdir", "-i", f"{blank}@@1M", "::/EFI/BOOT"],
            check=True,
            capture_output=True,
            text=True,
            env=env,
        )
        boot_dir = listing.stdout + listing.stderr
        if "BOOTX64" not in boot_dir.upper():
            return fail("mdir ::/EFI/BOOT missing BOOTX64.EFI", boot_dir)

        readme = subprocess.run(
            ["mtype", "-i", f"{blank}@@1M", "::README.TXT"],
            check=True,
            capture_output=True,
            text=True,
            env=env,
        )
        if README_BODY not in readme.stdout:
            return fail("mtype README.TXT mismatch", readme.stdout)

        qemu, sock, serial_log = boot_target_ovmf(OVMF, blank)
        serial = ""
        try:
            serial = wait_count(serial_log, "Coeleo OS", 1, 40)
            if "Coeleo OS" not in serial:
                return fail("OVMF boot did not print Coeleo OS", serial)
            serial = wait_count(serial_log, PROMPT, 1, 20)
            if serial.count(PROMPT) < 1:
                return fail("OVMF boot timed out waiting for prompt", serial)

            type_line(sock, "ls")
            serial = wait_count(serial_log, PROMPT, 2, 8)
            if "README.TXT" not in serial:
                return fail("ls did not list README.TXT on target", serial)

            type_line(sock, "disk")
            serial = wait_count(serial_log, PROMPT, 3, 8)
            if not ahci_disk_lines(serial):
                return fail("disk did not list ahci on target", serial)
        finally:
            qemu_quit(qemu, sock)

        print("test-phase17.2: ok")
        return 0


if __name__ == "__main__":
    raise SystemExit(main())
