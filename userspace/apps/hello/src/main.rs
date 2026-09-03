#![no_std]
#![no_main]

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let _ = libcoeleo::write(1, b"hello\n");
    libcoeleo::exit(0);
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}
