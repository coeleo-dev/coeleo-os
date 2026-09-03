#![no_std]
#![no_main]

use libcoeleo::{ERR, OPEN_READ, close, dirent_is_dir, dirent_name, open, readdir, write};

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let fd = open("/", OPEN_READ);
    if fd == ERR {
        let _ = write(1, b"ls: not found\n");
        libcoeleo::exit(1);
    }
    loop {
        let mut ent = [0u8; 64];
        let r = readdir(fd, &mut ent);
        if r == 0 || r == ERR {
            break;
        }
        let name = dirent_name(&ent);
        let _ = write(1, name.as_bytes());
        if dirent_is_dir(&ent) {
            let _ = write(1, b"/");
        }
        let _ = write(1, b"\n");
    }
    let _ = close(fd);
    libcoeleo::exit(0);
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}
