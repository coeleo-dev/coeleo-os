#![no_std]
#![no_main]

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    loop {}
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}
