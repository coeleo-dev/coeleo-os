#![no_std]
#![no_main]

use libcoeleo::write;

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let _ = write(1, b"cat: missing path\n");
    libcoeleo::exit(1);
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}
