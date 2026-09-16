use libcoeleo::{
    ERR, INFO_DISK, INFO_MEM, clipboard_get, clipboard_set, clock_ms, kill, sysinfo, write,
};

use crate::err;

pub fn cmd_kill(args: &str) {
    let pid_str = args.split_whitespace().next().unwrap_or("");
    if pid_str.is_empty() {
        err::err("kill", "missing process PID");
        err::usage("kill", "<pid>");
        return;
    }
    let Ok(pid) = parse_u64(pid_str) else {
        err::err_target("kill", pid_str, "invalid PID (expected a number)");
        return;
    };
    let r = kill(pid);
    if r == ERR {
        err::err_target("kill", pid_str, "process not found or failed to terminate");
    } else {
        let _ = write(1, b"process ");
        let _ = write(1, pid_str.as_bytes());
        let _ = write(1, b" terminated successfully\n");
    }
}

pub fn cmd_disks() {
    let mut buf = [0u8; 1024];
    let r = sysinfo(INFO_DISK, &mut buf);
    if r == ERR || r == 0 {
        err::warn("no storage device detected");
        return;
    }
    let _ = write(1, &buf[..r as usize]);
}

pub fn cmd_uname(args: &str) {
    let all = args.split_whitespace().any(|a| a == "-a" || a == "--all");
    if all {
        let _ = write(1, b"Coeleo OS 0.2.0 x86_64 SMP APIC (Clang/Rust no_std)\n");
    } else {
        let _ = write(1, b"Coeleo\n");
    }
}

pub fn cmd_free(args: &str) {
    let mut buf = [0u8; 512];
    let r = sysinfo(INFO_MEM, &mut buf);
    if r == ERR || r == 0 {
        err::err("free", "could not read memory statistics");
        return;
    }
    let text = core::str::from_utf8(&buf[..r as usize]).unwrap_or("");
    let human = args.split_whitespace().any(|a| a == "-h" || a == "--human");

    let _ = write(1, b"\x1b[1mSystem Memory (PMM / Heap):\x1b[0m\n");
    if human {
        let _ = write(1, b"  ");
        let _ = write(1, text.as_bytes());
    } else {
        let _ = write(1, b"  ");
        let _ = write(1, text.as_bytes());
    }
}

pub fn cmd_df(_args: &str) {
    let mut buf = [0u8; 1024];
    let r = sysinfo(INFO_DISK, &mut buf);
    if r == ERR || r == 0 {
        err::err("df", "could not inspect disks");
        return;
    }
    let _ = write(1, b"\x1b[1mMounted Filesystems:\x1b[0m\n");
    let _ = write(1, &buf[..r as usize]);
}

pub fn cmd_sleep(args: &str) {
    let sec_str = args.split_whitespace().next().unwrap_or("");
    if sec_str.is_empty() {
        err::err("sleep", "missing time in seconds");
        err::usage("sleep", "<seconds>");
        return;
    }
    let Ok(secs) = parse_u64(sec_str) else {
        err::err_target("sleep", sec_str, "invalid time (expected a number)");
        return;
    };
    if secs == 0 {
        return;
    }
    let ms = secs.saturating_mul(1000);
    let start = clock_ms();
    while clock_ms().saturating_sub(start) < ms {
        // Cooperative yield via tiny read or nop
        let mut dummy = [0u8; 1];
        let _ = libcoeleo::sysinfo(INFO_MEM, &mut dummy);
    }
}

pub fn cmd_clip(args: &str) {
    let mut parts = args.split_whitespace();
    let sub = parts.next().unwrap_or("");
    match sub {
        "get" => {
            let mut buf = [0u8; 4096];
            if let Some(len) = clipboard_get(&mut buf) {
                if len == 0 {
                    let _ = write(1, b"(clipboard empty)\n");
                } else {
                    let _ = write(1, &buf[..len.min(buf.len())]);
                    let _ = write(1, b"\n");
                }
            } else {
                let _ = write(1, b"(clipboard empty)\n");
            }
        }
        "set" => {
            let rest = args.strip_prefix("set").unwrap_or("").trim_start();
            if rest.is_empty() {
                err::err("clip set", "missing text to copy");
                err::usage("clip", "set <text>");
                return;
            }
            clipboard_set(rest.as_bytes());
            let _ = write(1, b"copied to clipboard\n");
        }
        _ => {
            err::err("clip", "invalid subcommand");
            err::usage("clip", "get | set <text>");
        }
    }
}

fn parse_u64(s: &str) -> Result<u64, ()> {
    let mut val = 0u64;
    for b in s.bytes() {
        if !b.is_ascii_digit() {
            return Err(());
        }
        val = val.saturating_mul(10).saturating_add((b - b'0') as u64);
    }
    Ok(val)
}
