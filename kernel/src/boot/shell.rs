//! Line editor on top of decoded PS/2 keys. No `unsafe`.

use alloc::string::String;
use core::fmt::Write;

use spin::Mutex;

use crate::console;

const LINE_CAP: usize = 128;
const WRITE_CAP: usize = 4096;
const HELP: &str = "help: type text; Enter runs it; Backspace deletes";
const HELP_CMDS: &str =
    "mem, panic, uptime, disk, ls, cd, cat, touch, write, rm, sync, run, ping, get";
const PROMPT: &str = "\x1b[1;96mcoeleo>\x1b[0m";
const WRITE_PROMPT: &str = "write>";

struct Line {
    buf: [u8; LINE_CAP],
    len: usize,
}

impl Line {
    const fn new() -> Self {
        Self {
            buf: [0; LINE_CAP],
            len: 0,
        }
    }

    fn as_str(&self) -> &str {
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("")
    }

    fn push(&mut self, c: u8) -> bool {
        if self.len >= LINE_CAP {
            return false;
        }
        self.buf[self.len] = c;
        self.len += 1;
        true
    }

    fn pop(&mut self) -> bool {
        if self.len == 0 {
            return false;
        }
        self.len -= 1;
        true
    }

    fn clear(&mut self) {
        self.len = 0;
    }
}

static LINE: Mutex<Line> = Mutex::new(Line::new());

enum Input {
    Cmd,
    Write { path: String, buf: String },
}

static INPUT: Mutex<Input> = Mutex::new(Input::Cmd);

pub fn push_scancode(byte: u8) {
    if let Some(b) = crate::kbd::push(byte) {
        handle_byte(b);
    }
}

pub fn handle_byte(b: u8) {
    match b {
        0x08 | 0x7f => backspace(),
        b'\n' | b'\r' => submit(),
        b if b.is_ascii_graphic() || b == b' ' => echo_char(b as char),
        _ => {}
    }
}

fn echo_char(c: char) {
    let b = c as u8;
    if !LINE.lock().push(b) {
        return;
    }
    let buf = [b];
    console::write(core::str::from_utf8(&buf).unwrap());
}

fn backspace() {
    if LINE.lock().pop() {
        console::write("\x08 \x08");
    }
}

fn submit() {
    console::write("\n");
    let line = {
        let locked = LINE.lock();
        String::from(locked.as_str())
    };
    LINE.lock().clear();
    if submit_write(&line) {
        return;
    }
    let (cmd, args) = match line.split_once(' ') {
        Some((c, rest)) => (String::from(c), String::from(rest.trim())),
        None => (line, String::new()),
    };
    match cmd.as_str() {
        "help" => {
            console::write(HELP);
            console::write("\n");
            console::write(HELP_CMDS);
            console::write("\n");
        }
        "mem" => cmd_mem(),
        "panic" => panic!("panic command"),
        "uptime" => cmd_uptime(),
        "disk" => cmd_disk(),
        "ls" => cmd_ls(&args),
        "cd" => cmd_cd(&args),
        "cat" => cmd_cat(&args),
        "touch" => cmd_touch(&args),
        "write" => {
            if cmd_write_enter(&args) {
                return;
            }
        }
        "rm" => cmd_rm(&args),
        "sync" => cmd_sync(),
        "date" => cmd_date(),
        "reboot" => cmd_reboot(),
        "poweroff" | "halt" => cmd_poweroff(),
        "run" => cmd_run(&args),
        "ping" => cmd_ping(&args),
        "get" => cmd_get(&args),
        "install" => cmd_install(&args),
        _ => {}
    }
    console::write(PROMPT);
}

fn submit_write(line: &str) -> bool {
    let mut input = INPUT.lock();
    let Input::Write { path, buf } = &mut *input else {
        return false;
    };
    if line == "." {
        let path = path.clone();
        let data = buf.clone();
        *input = Input::Cmd;
        drop(input);
        match crate::fs::write_file(&path, data.as_bytes()) {
            Ok(()) => {}
            Err(crate::fs::FsError::NoFs) => console::write("write: no filesystem\n"),
            Err(crate::fs::FsError::IsDir) => console::write("write: is a directory\n"),
            Err(_) => console::write("write: failed\n"),
        }
        console::write(PROMPT);
        return true;
    }
    if buf.len() + line.len() > WRITE_CAP {
        *input = Input::Cmd;
        drop(input);
        console::write("write: too large\n");
        console::write(PROMPT);
        return true;
    }
    buf.push_str(line);
    buf.push('\n');
    drop(input);
    console::write(WRITE_PROMPT);
    true
}

fn cmd_write_enter(args: &str) -> bool {
    let path = args.split_whitespace().next().unwrap_or("");
    if path.is_empty() {
        console::write("write: missing path\n");
        return false;
    }
    *INPUT.lock() = Input::Write {
        path: String::from(path),
        buf: String::new(),
    };
    console::write(WRITE_PROMPT);
    true
}

fn cmd_mem() {
    let s = crate::pmm::stats();
    let mut out = String::new();
    let _ = write!(
        out,
        "mem: {} / {} frames free ({} / {} KiB)",
        s.free,
        s.total,
        s.free * 4,
        s.total * 4
    );
    console::write(&out);
    console::write("\n");
}

fn cmd_uptime() {
    let mut buf = [0u8; 16];
    let t = crate::clock::format_mmss(&mut buf);
    console::write("uptime: ");
    console::write(t);
    console::write("\n");
}

fn cmd_date() {
    let mut buf = [0u8; 32];
    match crate::rtc::format_iso(&mut buf) {
        Some(s) => console::write(s),
        None => console::write("date: failed\n"),
    }
}

fn cmd_reboot() {
    cmd_sync();
    crate::acpi::reboot();
    console::write("reboot: failed\n");
}

fn cmd_poweroff() {
    cmd_sync();
    if !crate::acpi::poweroff() {
        console::write("poweroff: failed\n");
    }
}

fn cmd_disk() {
    let mut out = String::new();
    crate::blk::format_table(&mut out);
    console::write(&out);
}

fn cmd_install(args: &str) {
    console::write(crate::install::handle_cmd(args));
}

fn cmd_ls(args: &str) {
    match crate::fs::list(args) {
        Ok(ents) => {
            for e in ents {
                console::write(&e.name);
                if e.is_dir {
                    console::write("/");
                }
                console::write("\n");
            }
        }
        Err(crate::fs::FsError::NoFs) => console::write("ls: no filesystem\n"),
        Err(_) => console::write("ls: not found\n"),
    }
}

fn cmd_cd(args: &str) {
    if args.is_empty() {
        console::write("cd: missing path\n");
        return;
    }
    match crate::fs::chdir(args) {
        Ok(()) => {}
        Err(crate::fs::FsError::NoFs) => console::write("cd: no filesystem\n"),
        Err(crate::fs::FsError::NotDir) => console::write("cd: not a directory\n"),
        Err(_) => console::write("cd: not found\n"),
    }
}

fn cmd_cat(args: &str) {
    if args.is_empty() {
        console::write("cat: missing path\n");
        return;
    }
    match crate::fs::read_chunks(args, |bytes| match core::str::from_utf8(bytes) {
        Ok(s) => {
            console::write(s);
            Ok(())
        }
        Err(_) => Err(crate::fs::FsError::NotText),
    }) {
        Ok(()) => {}
        Err(crate::fs::FsError::NoFs) => console::write("cat: no filesystem\n"),
        Err(crate::fs::FsError::NotFound) => console::write("cat: not found\n"),
        Err(crate::fs::FsError::IsDir) => console::write("cat: is a directory\n"),
        Err(crate::fs::FsError::NotText) => console::write("cat: not text\n"),
        Err(_) => console::write("cat: read failed\n"),
    }
}

fn cmd_touch(args: &str) {
    if args.is_empty() {
        console::write("touch: missing path\n");
        return;
    }
    match crate::fs::touch(args) {
        Ok(()) => {}
        Err(crate::fs::FsError::NoFs) => console::write("touch: no filesystem\n"),
        Err(_) => console::write("touch: failed\n"),
    }
}

fn cmd_rm(args: &str) {
    if args.is_empty() {
        console::write("rm: missing path\n");
        return;
    }
    match crate::fs::remove(args) {
        Ok(()) => {}
        Err(crate::fs::FsError::NoFs) => console::write("rm: no filesystem\n"),
        Err(crate::fs::FsError::NotFound) => console::write("rm: not found\n"),
        Err(crate::fs::FsError::IsDir) => console::write("rm: is a directory\n"),
        Err(_) => console::write("rm: failed\n"),
    }
}

fn cmd_sync() {
    match crate::fs::sync() {
        Ok(()) => {}
        Err(crate::fs::FsError::NoFs) => console::write("sync: no filesystem\n"),
        Err(_) => console::write("sync: failed\n"),
    }
}

fn cmd_run(args: &str) {
    let path = args.split_whitespace().next().unwrap_or("");
    if path.is_empty() {
        console::write("run: missing path\n");
        return;
    }
    match crate::fs::read_file(path) {
        Err(crate::fs::FsError::NoFs) => console::write("run: no filesystem\n"),
        Err(crate::fs::FsError::NotFound) => console::write("run: not found\n"),
        Err(crate::fs::FsError::IsDir) => console::write("run: is a directory\n"),
        Err(_) => console::write("run: bad elf\n"),
        Ok(bytes) => match crate::elfload::load(&bytes) {
            Err(()) => console::write("run: bad elf\n"),
            Ok(image) => match crate::process::run_named(image, path) {
                crate::process::Outcome::Exited => {}
                crate::process::Outcome::Fault => console::write("run: fault\n"),
            },
        },
    }
}

fn cmd_ping(args: &str) {
    let tok = args.split_whitespace().next().unwrap_or("");
    if tok.is_empty() {
        console::write("ping: missing address\n");
        return;
    }
    let Some(addr) = crate::net::parse_ipv4(tok) else {
        console::write("ping: bad address\n");
        return;
    };
    match crate::net::ping(addr) {
        Err(crate::net::PingError::NoNet) => console::write("ping: no network\n"),
        Err(crate::net::PingError::Failed) => console::write("ping: failed\n"),
        Ok(replies) if replies.is_empty() => console::write("ping: timeout\n"),
        Ok(replies) => {
            for r in replies {
                let mut out = String::new();
                let _ = write!(out, "reply from {} time={}ms\n", r.from, r.time_ms);
                console::write(&out);
            }
        }
    }
}

fn cmd_get(args: &str) {
    let tok = args.split_whitespace().next().unwrap_or("");
    if tok.is_empty() {
        console::write("get: missing url\n");
        return;
    }
    let url = match crate::http::parse_url(tok) {
        Err(crate::http::UrlError::Https) => {
            console::write("get: https not supported\n");
            return;
        }
        Err(crate::http::UrlError::Bad) => {
            console::write("get: bad url\n");
            return;
        }
        Ok(url) => url,
    };
    match crate::net::http_get(&url) {
        Err(crate::net::HttpError::NoNet) => console::write("get: no network\n"),
        Err(crate::net::HttpError::Timeout) => console::write("get: timeout\n"),
        Err(crate::net::HttpError::Failed) => console::write("get: failed\n"),
        Ok(body) => match core::str::from_utf8(&body) {
            Ok(s) => console::write(s),
            Err(_) => console::write("get: not text\n"),
        },
    }
}
