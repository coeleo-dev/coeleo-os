use libcoeleo::{
    ERR, OPEN_CREATE, OPEN_READ, OPEN_TRUNC, OPEN_WRITE, close, open, read, sync, unlink, write,
};

use crate::cwd::{resolve, Cwd};

const PKG_CAP: usize = ::pkg::PAYLOAD_MAX;

static mut PKG_BUF: [u8; PKG_CAP] = [0; PKG_CAP];

const PKG_HELP: &[u8] = b"pkg - signed .coe packages\n\
  pkg install <file>   e.g. pkg install /pacotes/hello.coe\n\
  pkg remove <name>     e.g. pkg remove hello\n\
  pkg --help\n\
Name: 1-8 characters [a-z0-9]. Installs to /bin/<name>.\n";

pub fn cmd_pkg(cwd: &Cwd, args: &str) {
    let (sub, rest) = match args.split_once(' ') {
        Some((s, r)) => (s, r.trim()),
        None => (args, ""),
    };
    match sub {
        "" | "help" | "--help" | "-h" => {
            let _ = write(1, PKG_HELP);
        }
        "install" => cmd_pkg_install(cwd, rest),
        "remove" => cmd_pkg_remove(rest),
        _ => {
            let _ = write(1, b"pkg: failed\n");
        }
    }
}

fn cmd_pkg_install(cwd: &Cwd, args: &str) {
    let path_arg = args.split_whitespace().next().unwrap_or("");
    if path_arg.is_empty() {
        let _ = write(1, b"pkg: failed\n");
        return;
    }
    let mut abs = [0u8; 256];
    let path = resolve(cwd.as_str(), path_arg, &mut abs);
    let fd = open(path, OPEN_READ);
    if fd == ERR {
        let _ = write(1, b"pkg: failed\n");
        return;
    }
    let buf = unsafe { &mut *core::ptr::addr_of_mut!(PKG_BUF) };
    let mut n = 0usize;
    loop {
        if n >= buf.len() {
            let _ = close(fd);
            let _ = write(1, b"pkg: bad package\n");
            return;
        }
        let take = (buf.len() - n).min(4096);
        let r = read(fd, &mut buf[n..n + take]);
        if r == 0 {
            break;
        }
        if r == ERR {
            let _ = close(fd);
            let _ = write(1, b"pkg: failed\n");
            return;
        }
        n += r as usize;
    }
    let _ = close(fd);
    let Ok(p) = ::pkg::parse(&buf[..n]) else {
        let _ = write(1, b"pkg: bad package\n");
        return;
    };
    if !::pkg::verify(&p, ::pkg::pubkey()) {
        let _ = write(1, b"pkg: bad signature\n");
        return;
    }
    let mut dest = [0u8; 16];
    dest[..5].copy_from_slice(b"/bin/");
    let nl = p.name.len();
    dest[5..5 + nl].copy_from_slice(p.name.as_bytes());
    let dest = core::str::from_utf8(&dest[..5 + nl]).unwrap_or("");
    let out = open(dest, OPEN_CREATE | OPEN_WRITE | OPEN_TRUNC);
    if out == ERR {
        let _ = write(1, b"pkg: failed\n");
        return;
    }
    let mut off = 0usize;
    while off < p.payload.len() {
        let take = (p.payload.len() - off).min(4096);
        let w = write(out, &p.payload[off..off + take]);
        if w == ERR {
            let _ = close(out);
            let _ = write(1, b"pkg: failed\n");
            return;
        }
        off += w as usize;
    }
    let _ = close(out);
    let _ = sync();
    let _ = write(1, b"pkg: installed ");
    let _ = write(1, p.name.as_bytes());
    let _ = write(1, b"\n");
}

fn cmd_pkg_remove(args: &str) {
    let name = args.split_whitespace().next().unwrap_or("");
    if name.is_empty()
        || name.len() > ::pkg::NAME_MAX
        || !name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
    {
        let _ = write(1, b"pkg: failed\n");
        return;
    }
    let mut dest = [0u8; 16];
    dest[..5].copy_from_slice(b"/bin/");
    dest[5..5 + name.len()].copy_from_slice(name.as_bytes());
    let dest = core::str::from_utf8(&dest[..5 + name.len()]).unwrap_or("");
    if unlink(dest) == ERR {
        let _ = write(1, b"pkg: failed\n");
        return;
    }
    let _ = sync();
    let _ = write(1, b"pkg: removed ");
    let _ = write(1, name.as_bytes());
    let _ = write(1, b"\n");
}
