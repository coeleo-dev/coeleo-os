#![no_std]
#![no_main]

mod cwd;
mod fs;
mod net;
mod path;
mod pkg;

use libcoeleo::{ERR, INFO_DISK, INFO_INSTALL, INFO_MEM, clock_ms, date, poweroff, ps, reboot, spawn, sync, sysinfo, write};

use cwd::Cwd;
use fs::{submit_write, WriteJob, WriteNext};
use net::{cmd_get, cmd_ping};
use path::{cmd_path, cmd_run, cmd_spawn_wait};
use pkg::cmd_pkg;

const PROMPT: &[u8] = b"\x1b[1;96mcoeleo>\x1b[0m";
const HELP: &[u8] = b"mem, uptime, disk, ls, cd, cat, touch, write, rm, sync, run, ping, get, ps, clock, pkg, date, reboot, poweroff, install\n\
ls [path]           list directory\n\
cd <path>           change directory\n\
cat <path>          print file\n\
touch <path>        create empty file\n\
write <path>        lines until a line with only .\n\
rm <path>           delete file\n\
sync                flush disk\n\
run <path>          spawn ELF and wait\n\
ping 10.0.2.2       ICMP to the QEMU gateway\n\
get http://host/    HTTP GET (no https)\n\
pkg --help          signed .coe packages\n\
date                civil date from the RTC (UTC)\n\
reboot              restart the machine\n\
poweroff / halt     cut power\n\
install <n>         type twice to wipe disk n and install Coeleo\n\
ps / clock / mem / disk / uptime\n";
const LINE_CAP: usize = 128;

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let mut cwd = Cwd::new();
    let mut line = [0u8; LINE_CAP];
    let mut writing: Option<WriteJob> = None;
    loop {
        if writing.is_none() {
            let _ = write(1, PROMPT);
        }
        let n = read_line(&mut line);
        if n == usize::MAX {
            libcoeleo::exit(0);
        }
        let text = core::str::from_utf8(&line[..n]).unwrap_or("");
        if let Some(job) = writing.as_mut() {
            match submit_write(job, text) {
                WriteNext::Stay => {}
                WriteNext::Done => writing = None,
            }
            continue;
        }
        dispatch(text, &mut cwd, &mut writing);
    }
}

fn read_line(line: &mut [u8]) -> usize {
    let mut n = 0;
    loop {
        let mut c = [0u8; 1];
        let r = libcoeleo::read(0, &mut c);
        if r == 0 {
            return usize::MAX;
        }
        if r == ERR {
            continue;
        }
        match c[0] {
            b'\n' | b'\r' => {
                let _ = write(1, b"\n");
                return n;
            }
            0x08 | 0x7f => {
                if n > 0 {
                    n -= 1;
                    let _ = write(1, b"\x08 \x08");
                }
            }
            b if (b.is_ascii_graphic() || b == b' ') && n < line.len() => {
                line[n] = b;
                n += 1;
                let _ = write(1, &c);
            }
            _ => {}
        }
    }
}

fn dispatch(line: &str, cwd: &mut Cwd, writing: &mut Option<WriteJob>) {
    let line = line.trim();
    if line.is_empty() {
        return;
    }
    let (cmd, args) = match line.split_once(' ') {
        Some((c, rest)) => (c, rest.trim()),
        None => (line, ""),
    };
    match cmd {
        "exit" => libcoeleo::exit(0),
        "help" => {
            let _ = write(1, HELP);
        }
        "ls" => fs::cmd_ls(cwd, args),
        "cat" => fs::cmd_cat(cwd, args),
        "cd" => fs::cmd_cd(cwd, args),
        "ps" => cmd_ps(),
        "clock" => cmd_clock(),
        "spin" => cmd_spawn_wait("spin"),
        "fault" => cmd_spawn_wait("fault"),
        "mem" => cmd_sysinfo(INFO_MEM),
        "disk" => cmd_sysinfo(INFO_DISK),
        "uptime" => cmd_uptime(),
        "touch" => fs::cmd_touch(cwd, args),
        "write" => fs::cmd_write_enter(cwd, args, writing),
        "rm" => fs::cmd_rm(cwd, args),
        "sync" => fs::cmd_sync(),
        "date" => cmd_date(),
        "reboot" => {
            let _ = sync();
            let _ = reboot();
            let _ = write(1, b"reboot: failed\n");
        }
        "poweroff" | "halt" => {
            let _ = sync();
            let _ = poweroff();
            let _ = write(1, b"poweroff: failed\n");
        }
        "run" => cmd_run(cwd, args),
        "ping" => cmd_ping(args),
        "get" => cmd_get(args),
        "pkg" => cmd_pkg(cwd, args),
        "install" => cmd_install(args),
        _ => cmd_path(cmd),
    }
}

fn cmd_sysinfo(kind: u64) {
    let mut buf = [0u8; 512];
    let r = sysinfo(kind, &mut buf);
    if r == ERR {
        return;
    }
    let _ = write(1, &buf[..r as usize]);
}

fn cmd_install(args: &str) {
    let mut buf = [0u8; 512];
    let n = args.len().min(buf.len());
    buf[..n].copy_from_slice(&args.as_bytes()[..n]);
    let r = sysinfo(INFO_INSTALL, &mut buf);
    if r == ERR {
        let _ = write(1, b"install: refuse\n");
        return;
    }
    let _ = write(1, &buf[..r as usize]);
}

fn cmd_date() {
    let mut buf = [0u8; 32];
    let r = date(&mut buf);
    if r == ERR {
        let _ = write(1, b"date: failed\n");
        return;
    }
    let _ = write(1, &buf[..r as usize]);
}

fn cmd_uptime() {
    let total = clock_ms() / 1000;
    let _ = write(1, b"uptime: ");
    write_mmss(total);
    let _ = write(1, b"\n");
}

fn write_mmss(secs: u64) {
    let s = (secs % 60) as u32;
    let m = (secs / 60) as u32;
    if m < 100 {
        write_pad2(m);
    } else {
        write_u32(m);
    }
    let _ = write(1, b":");
    write_pad2(s);
}

fn write_pad2(n: u32) {
    let tens = ((n / 10) % 10) as u8;
    let ones = (n % 10) as u8;
    let buf = [b'0' + tens, b'0' + ones];
    let _ = write(1, &buf);
}

fn cmd_ps() {
    let mut buf = [0u8; 64];
    let r = ps(&mut buf);
    if r == ERR {
        return;
    }
    let n = r as usize;
    let mut i = 0usize;
    while i + 16 <= n {
        let pid = u32::from_le_bytes(buf[i..i + 4].try_into().unwrap_or([0; 4]));
        write_u32(pid);
        let _ = write(1, b" ");
        let name = &buf[i + 4..i + 16];
        let nlen = name.iter().position(|&b| b == 0).unwrap_or(name.len());
        let _ = write(1, &name[..nlen]);
        let _ = write(1, b"\n");
        i += 16;
    }
}

fn cmd_clock() {
    let mut buf = [0u8; 64];
    let r = ps(&mut buf);
    if r != ERR {
        let n = r as usize;
        let mut i = 0usize;
        while i + 16 <= n {
            let name = &buf[i + 4..i + 16];
            let nlen = name.iter().position(|&b| b == 0).unwrap_or(name.len());
            if &name[..nlen] == b"clock" {
                return;
            }
            i += 16;
        }
    }
    let _ = spawn("clock");
}

pub(crate) fn write_u32(n: u32) {
    if n == 0 {
        let _ = write(1, b"0");
        return;
    }
    let mut buf = [0u8; 10];
    let mut x = n;
    let mut i = 10usize;
    while x > 0 {
        i -= 1;
        buf[i] = b'0' + (x % 10) as u8;
        x /= 10;
    }
    let _ = write(1, &buf[i..]);
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}
