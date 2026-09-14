#![no_std]
#![no_main]

#[unsafe(no_mangle)]
pub extern "C" fn _start(argc: usize, argv: *const *const u8) -> ! {
    let _ = libcoeleo::write(1, b"hello\n");
    if argc >= 2 {
        let p = unsafe { *argv.add(1) };
        if !p.is_null() {
            let s = unsafe { cstr(p) };
            let _ = libcoeleo::write(1, s);
            let _ = libcoeleo::write(1, b"\n");
        }
    }
    libcoeleo::exit(0);
}

unsafe fn cstr<'a>(p: *const u8) -> &'a [u8] {
    let mut n = 0usize;
    while n < 255 && unsafe { *p.add(n) } != 0 {
        n += 1;
    }
    unsafe { core::slice::from_raw_parts(p, n) }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}
