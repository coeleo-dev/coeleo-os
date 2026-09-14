#![no_std]
#![no_main]

use libcoeleo::write;

#[unsafe(no_mangle)]
pub extern "C" fn _start(argc: usize, argv: *const *const u8) -> ! {
    for i in 1..argc {
        if i > 1 {
            let _ = write(1, b" ");
        }
        let p = unsafe { *argv.add(i) };
        if !p.is_null() {
            let _ = write(1, unsafe { cstr(p) });
        }
    }
    let _ = write(1, b"\n");
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
