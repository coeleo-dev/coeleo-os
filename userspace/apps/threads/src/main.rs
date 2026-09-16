#![no_std]
#![no_main]

use core::sync::atomic::{AtomicU32, Ordering};

static DONE: AtomicU32 = AtomicU32::new(0);

#[repr(align(16))]
struct Stack([u8; 8192]);

// `static mut` (not `static`) so the linker keeps it in writable .data/.bss;
// an immutable `static` lands in .rodata, which faults on the first push.
static mut THREAD_STACK: Stack = Stack([0; 8192]);

#[unsafe(no_mangle)]
extern "C" fn worker(arg: u64) {
    let _ = arg;
    let _ = libcoeleo::write(1, b"thread ok\n");
    DONE.store(1, Ordering::SeqCst);
    libcoeleo::exit(0);
}

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let stack_top = unsafe { (&raw mut THREAD_STACK) as usize as u64 + 8192 };
    let tid = libcoeleo::thread_create(worker as usize as u64, 42, stack_top, 0);
    if tid == libcoeleo::ERR {
        let _ = libcoeleo::write(1, b"thread create failed\n");
        libcoeleo::exit(0);
    }
    // Busy-wait until the worker runs; the LAPIC timer preempts us so the
    // worker thread gets a time slice.
    while DONE.load(Ordering::SeqCst) == 0 {}
    let _ = libcoeleo::write(1, b"main done\n");
    libcoeleo::exit(0);
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}
