#![no_std]
#![no_main]

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    unsafe {
        let _ = core::ptr::read_volatile(core::ptr::null::<u8>());
    }
    loop {}
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}
