#![no_std]
#![no_main]

mod alias;
mod complete;
mod cwd;
mod err;
mod exec;
mod fs;
mod highlight;
mod line;
mod net;
mod path;
mod pkg;
mod sys;
mod text;
mod tree;

use libcoeleo::{
    ERR, INFO_DISK, INFO_INSTALL, INFO_MEM, clock_ms, date, poweroff, ps, reboot, spawn, sync,
    sysinfo, write,
};

use alias::AliasTable;
use cwd::Cwd;
use fs::{WriteJob, WriteNext, submit_write};
use line::{History, LINE_CAP, read_line};
use net::{cmd_get, cmd_ping};
use path::{cmd_path, cmd_run, cmd_spawn_wait};
use pkg::cmd_pkg;

const HELP: &[u8] = b"mem, uptime, disk, ls, cd, cat, touch, write, rm, mkdir, cp, mv, echo, pwd, clear, sync, run, ping, get, ps, clock, pkg, date, reboot, poweroff, install, edit\n\
ls [-l] [path]      list directory\n\
cd <path>           change directory\n\
pwd                 print cwd\n\
cat <path>          print file\n\
touch <path>        create empty file\n\
mkdir <path>        create directory\n\
cp <src> <dst>      copy file\n\
mv <src> <dst>      move file\n\
echo [text]         print arguments ($? for exit code)\n\
clear               clear the VT\n\
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
edit <path>         TUI editor (^S save, ^Q quit)\n\
head [-n N] <path>  print first N lines\n\
tail [-n N] <path>  print last N lines\n\
wc [-l|-w|-c] <p>   count lines, words, bytes\n\
grep [-i|-n] <term> search text in file or stdin\n\
tree [path]         recursive directory tree\n\
stat <path>         file metadata and size\n\
kill <pid>          terminate a process\n\
disks / lsblk       storage devices\n\
uname [-a]          system and kernel info\n\
free [-h] / df [-h] memory and disk usage\n\
sleep <seconds>     pause execution\n\
clip [get|set]      system clipboard\n\
alias [name='cmd']  shortcuts (unalias <name>)\n\
which <cmd>         locate command origin\n\
history [-c]        command history\n\
source <file.sh>    run commands from script\n\
| > <               pipe stdout; redirect file (ELF)\n\
;                   chain multiple commands\n\
Up / Tab / arrows   history; complete; move cursor\n\
Home/End/Del/^A/^E  fast navigation and line edit\n\
^R                  reverse incremental history search\n\
ps / clock / mem / disk / uptime\n";

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let mut cwd = Cwd::new();
    let mut hist = History::new();
    let mut aliases = AliasTable::new();
    let mut line = [0u8; LINE_CAP];
    let mut writing: Option<WriteJob> = None;
    let mut last_status = 0u8;
    let mut pbuf = [0u8; 128];

    loop {
        let plen = format_prompt(&cwd, last_status, &mut pbuf);
        let prompt = &pbuf[..plen];
        if writing.is_none() {
            let _ = write(1, prompt);
        }
        let n = read_line(&mut line, &mut hist, &cwd, prompt, writing.is_none());
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
        dispatch(
            text,
            &mut cwd,
            &mut writing,
            &mut aliases,
            &mut hist,
            &mut last_status,
        );
    }
}

fn format_prompt(cwd: &Cwd, last_status: u8, buf: &mut [u8; 128]) -> usize {
    let mut i = 0usize;
    let b1 = b"\x1b[1;34m";
    buf[i..i + b1.len()].copy_from_slice(b1);
    i += b1.len();

    let cs = cwd.as_str();
    let cn = cs.len().min(buf.len() - i - 32);
    buf[i..i + cn].copy_from_slice(&cs.as_bytes()[..cn]);
    i += cn;

    let b2 = b"\x1b[0m ";
    buf[i..i + b2.len()].copy_from_slice(b2);
    i += b2.len();

    if last_status == 0 {
        let b3 = b"\x1b[1;96mcoeleo>\x1b[0m ";
        buf[i..i + b3.len()].copy_from_slice(b3);
        i += b3.len();
    } else {
        let b3 = b"\x1b[1;31mcoeleo>\x1b[0m ";
        buf[i..i + b3.len()].copy_from_slice(b3);
        i += b3.len();
    }
    i
}

fn dispatch(
    line: &str,
    cwd: &mut Cwd,
    writing: &mut Option<WriteJob>,
    aliases: &mut AliasTable,
    hist: &mut History,
    last_status: &mut u8,
) {
    let line = line.trim();
    if line.is_empty() {
        return;
    }
    if line.contains(';') {
        for sub in line.split(';') {
            let sub = sub.trim();
            if !sub.is_empty() {
                dispatch_single(sub, cwd, writing, aliases, hist, last_status);
            }
        }
        return;
    }
    dispatch_single(line, cwd, writing, aliases, hist, last_status);
}

fn dispatch_single(
    line: &str,
    cwd: &mut Cwd,
    writing: &mut Option<WriteJob>,
    aliases: &mut AliasTable,
    hist: &mut History,
    last_status: &mut u8,
) {
    if exec::has_meta(line) {
        exec::run_line(cwd, line);
        *last_status = 0;
        return;
    }
    let (mut cmd, mut args) = match line.split_once(' ') {
        Some((c, rest)) => (c, rest.trim()),
        None => (line, ""),
    };

    // Alias expansion
    let mut expanded_buf = [0u8; 128];
    let mut expanded_len = 0usize;
    if let Some(val) = aliases.get(cmd) {
        let vb = val.as_bytes();
        let vn = vb.len().min(expanded_buf.len());
        expanded_buf[..vn].copy_from_slice(&vb[..vn]);
        expanded_len = vn;
        if !args.is_empty() && expanded_len < expanded_buf.len() {
            expanded_buf[expanded_len] = b' ';
            expanded_len += 1;
            let ab = args.as_bytes();
            let an = ab.len().min(expanded_buf.len() - expanded_len);
            expanded_buf[expanded_len..expanded_len + an].copy_from_slice(&ab[..an]);
            expanded_len += an;
        }
    }
    let expanded_str = if expanded_len > 0 {
        core::str::from_utf8(&expanded_buf[..expanded_len]).unwrap_or("")
    } else {
        ""
    };
    if !expanded_str.is_empty() {
        if let Some((c, rest)) = expanded_str.split_once(' ') {
            cmd = c;
            args = rest.trim();
        } else {
            cmd = expanded_str;
            args = "";
        }
    }

    *last_status = 0;
    match cmd {
        "exit" => libcoeleo::exit(0),
        "help" => {
            let _ = write(1, HELP);
        }
        "ls" => fs::cmd_ls(cwd, args),
        "cat" => fs::cmd_cat(cwd, args),
        "cd" => fs::cmd_cd(cwd, args),
        "pwd" => fs::cmd_pwd(cwd),
        "echo" => cmd_echo(args, *last_status),
        "clear" => {
            let _ = write(1, b"\x1b[2J\x1b[H");
        }
        "mkdir" => fs::cmd_mkdir(cwd, args),
        "cp" => fs::cmd_cp(cwd, args),
        "mv" => fs::cmd_mv(cwd, args),
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
            *last_status = 1;
        }
        "poweroff" | "halt" => {
            let _ = sync();
            let _ = poweroff();
            let _ = write(1, b"poweroff: failed\n");
            *last_status = 1;
        }
        "run" => cmd_run(cwd, args),
        "ping" => cmd_ping(args),
        "get" => cmd_get(args),
        "pkg" => cmd_pkg(cwd, args),
        "install" => cmd_install(args),
        // New built-ins
        "head" => text::cmd_head(cwd, args),
        "tail" => text::cmd_tail(cwd, args),
        "wc" => text::cmd_wc(cwd, args),
        "grep" => text::cmd_grep(cwd, args),
        "stat" => text::cmd_stat(cwd, args),
        "tree" => tree::cmd_tree(cwd, args),
        "kill" => sys::cmd_kill(args),
        "disks" | "lsblk" => sys::cmd_disks(),
        "uname" | "version" => sys::cmd_uname(args),
        "free" => sys::cmd_free(args),
        "df" => sys::cmd_df(args),
        "sleep" => sys::cmd_sleep(args),
        "clip" => sys::cmd_clip(args),
        "alias" => alias::cmd_alias(aliases, args),
        "unalias" => alias::cmd_unalias(aliases, args),
        "which" => alias::cmd_which(aliases, args),
        "history" => alias::cmd_history(hist, args),
        "source" | "." => cmd_source(cwd, args, writing, aliases, hist, last_status),
        _ => {
            cmd_path(cmd, args);
        }
    }
}

fn cmd_echo(args: &str, last_status: u8) {
    if args == "$?" {
        write_u32(last_status as u32);
        let _ = write(1, b"\n");
        return;
    }
    if !args.is_empty() {
        let _ = write(1, args.as_bytes());
    }
    let _ = write(1, b"\n");
}

fn cmd_source(
    cwd: &mut Cwd,
    args: &str,
    writing: &mut Option<WriteJob>,
    aliases: &mut AliasTable,
    hist: &mut History,
    last_status: &mut u8,
) {
    let path_arg = args.split_whitespace().next().unwrap_or("");
    if path_arg.is_empty() {
        err::err("source", "caminho de arquivo ausente");
        err::usage("source", "<arquivo.sh>");
        *last_status = 1;
        return;
    }
    let mut abs = [0u8; 256];
    let path = cwd::resolve(cwd.as_str(), path_arg, &mut abs);
    let fd = libcoeleo::open(path, libcoeleo::OPEN_READ);
    if fd == ERR {
        err::err_target("source", path_arg, "arquivo não encontrado");
        *last_status = 1;
        return;
    }
    let mut line_buf = [0u8; 128];
    let mut line_len = 0usize;
    let mut buf = [0u8; 512];
    loop {
        let r = libcoeleo::read(fd, &mut buf);
        if r == 0 || r == ERR {
            break;
        }
        for &b in &buf[..r as usize] {
            if b == b'\n' {
                if let Ok(line_str) = core::str::from_utf8(&line_buf[..line_len]) {
                    dispatch(line_str, cwd, writing, aliases, hist, last_status);
                }
                line_len = 0;
            } else if line_len < line_buf.len() {
                line_buf[line_len] = b;
                line_len += 1;
            }
        }
    }
    if line_len > 0 {
        if let Ok(line_str) = core::str::from_utf8(&line_buf[..line_len]) {
            dispatch(line_str, cwd, writing, aliases, hist, last_status);
        }
    }
    let _ = libcoeleo::close(fd);
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
