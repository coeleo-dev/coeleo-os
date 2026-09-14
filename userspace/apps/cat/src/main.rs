#![no_std]
#![no_main]

use libcoeleo::{ERR, OPEN_READ, close, open, write};

#[unsafe(no_mangle)]
pub extern "C" fn _start(argc: usize, argv: *const *const u8) -> ! {
    if argc <= 1 {
        libcoeleo::exit(copy_fd(0));
    }
    let mut code = 0u64;
    for i in 1..argc {
        let p = unsafe { *argv.add(i) };
        let path = unsafe { cstr(p) };
        if path.is_empty() {
            let _ = write(1, b"cat: missing path\n");
            code = 1;
            continue;
        }
        let fd = open(path, OPEN_READ);
        if fd == ERR {
            let _ = write(1, b"cat: not found\n");
            code = 1;
            continue;
        }
        if copy_fd(fd) != 0 {
            code = 1;
        }
        let _ = close(fd);
    }
    libcoeleo::exit(code);
}

fn copy_fd(fd: u64) -> u64 {
    loop {
        let mut buf = [0u8; 512];
        let r = libcoeleo::read(fd, &mut buf);
        if r == 0 {
            return 0;
        }
        if r == ERR {
            let _ = write(1, b"cat: read failed\n");
            return 1;
        }
        let n = r as usize;
        let mut off = 0usize;
        while off < n {
            let w = write(1, &buf[off..n]);
            if w == ERR {
                return 1;
            }
            off += w as usize;
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
