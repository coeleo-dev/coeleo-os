#![no_std]
#![no_main]

use libcoeleo::{ERR, OPEN_CREATE, OPEN_READ, OPEN_TRUNC, OPEN_WRITE, close, open, read, write};

const CAP: usize = 4096;
const STATUS: &[u8] = b"^S save  ^Q quit";

#[unsafe(no_mangle)]
pub extern "C" fn _start(argc: usize, argv: *const *const u8) -> ! {
    let path = if argc >= 2 {
        unsafe { cstr(*argv.add(1)) }
    } else {
        "notes.txt"
    };
    if path.is_empty() {
        let _ = write(1, b"edit: missing path\n");
        libcoeleo::exit(1);
    }
    let mut buf = [0u8; CAP];
    let mut n = load(path, &mut buf);
    paint(&buf, n);
    loop {
        let mut c = [0u8; 1];
        let r = libcoeleo::read(0, &mut c);
        if r == 0 {
            libcoeleo::exit(0);
        }
        if r == ERR {
            continue;
        }
        match c[0] {
            0x13 => {
                if save(path, &buf[..n]).is_err() {
                    let _ = write(1, b"\nsave: failed\n");
                }
                paint(&buf, n);
            }
            0x11 => {
                let _ = write(1, b"\n");
                libcoeleo::exit(0);
            }
            0x1B => skip_csi(),
            b'\n' | b'\r' => {
                if insert(&mut buf, &mut n, b'\n') {
                    paint(&buf, n);
                }
            }
            0x08 | 0x7f => {
                if n > 0 {
                    n -= 1;
                    paint(&buf, n);
                }
            }
            b if b.is_ascii_graphic() || b == b' ' => {
                if insert(&mut buf, &mut n, b) {
                    paint(&buf, n);
                }
            }
            _ => {}
        }
    }
}

fn load(path: &str, buf: &mut [u8]) -> usize {
    let fd = open(path, OPEN_READ);
    if fd == ERR {
        return 0;
    }
    let mut probe = [0u8; 64];
    if libcoeleo::readdir(fd, &mut probe) != ERR {
        let _ = close(fd);
        let _ = write(1, b"edit: is a directory\n");
        libcoeleo::exit(1);
    }
    let mut n = 0usize;
    loop {
        if n >= buf.len() {
            break;
        }
        let r = read(fd, &mut buf[n..]);
        if r == 0 {
            break;
        }
        if r == ERR {
            break;
        }
        n += r as usize;
    }
    let _ = close(fd);
    n
}

fn save(path: &str, data: &[u8]) -> Result<(), ()> {
    let fd = open(path, OPEN_CREATE | OPEN_WRITE | OPEN_TRUNC);
    if fd == ERR {
        return Err(());
    }
    let mut off = 0usize;
    while off < data.len() {
        let w = write(fd, &data[off..]);
        if w == ERR {
            let _ = close(fd);
            return Err(());
        }
        off += w as usize;
    }
    let _ = close(fd);
    Ok(())
}

fn insert(buf: &mut [u8], n: &mut usize, b: u8) -> bool {
    if *n >= buf.len() {
        return false;
    }
    buf[*n] = b;
    *n += 1;
    true
}

fn paint(buf: &[u8], n: usize) {
    let _ = write(1, b"\x1b[2J\x1b[H");
    if n > 0 {
        let _ = write(1, &buf[..n]);
    }
    if n == 0 || buf[n - 1] != b'\n' {
        let _ = write(1, b"\n");
    }
    let _ = write(1, STATUS);
}

fn skip_csi() {
    let mut c = [0u8; 1];
    let r = libcoeleo::read(0, &mut c);
    if r == 0 || r == ERR || c[0] != b'[' {
        return;
    }
    loop {
        let r = libcoeleo::read(0, &mut c);
        if r == 0 || r == ERR {
            return;
        }
        if c[0].is_ascii_alphabetic() {
            return;
        }
    }
}

unsafe fn cstr<'a>(p: *const u8) -> &'a str {
    if p.is_null() {
        return "";
    }
    let mut n = 0usize;
    while n < 255 && unsafe { *p.add(n) } != 0 {
        n += 1;
    }
    core::str::from_utf8(unsafe { core::slice::from_raw_parts(p, n) }).unwrap_or("")
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}
