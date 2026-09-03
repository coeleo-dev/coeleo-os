#![no_std]
#![no_main]

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let mut pix = [0u32; 64 * 64];
    for p in pix.iter_mut() {
        *p = coeleo_theme::ACCENT;
    }
    let id = libcoeleo::win_create(64, 64, &pix);
    if id == libcoeleo::ERR {
        libcoeleo::exit(1);
    }
    let _ = libcoeleo::win_damage(id, 0, 0, 64, 64);
    libcoeleo::exit(0);
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}
