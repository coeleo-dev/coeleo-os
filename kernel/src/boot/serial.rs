//! COM1 (0x3F8). Initialized first so later panics can still speak.

use core::arch::asm;
use core::fmt::Write;
use spin::Mutex;
use uart_16550::SerialPort;

const COM1: u16 = 0x3F8;

/// Port I/O wrapper; the port is only touched after [`init`].
static SERIAL: Mutex<SerialPort> = Mutex::new(unsafe { SerialPort::new(COM1) });

pub fn init() {
    SERIAL.lock().init();
    // uart_16550 0.3 programs 38400 and leaves the receiver IRQ enabled.
    // Spec wants 115200; there is no IDT yet, so IER must stay clear.
    unsafe {
        outb(COM1 + 1, 0x00);
        outb(COM1 + 3, 0x80);
        outb(COM1, 0x01);
        outb(COM1 + 1, 0x00);
        outb(COM1 + 3, 0x03);
    }
}

pub fn write_str(s: &str) {
    let _ = SERIAL.lock().write_str(s);
}

pub fn write_dec_u32(n: u32) {
    if n == 0 {
        write_str("0");
        return;
    }
    let mut buf = [0u8; 10];
    let mut i = 10;
    let mut x = n;
    while x > 0 {
        i -= 1;
        buf[i] = b'0' + (x % 10) as u8;
        x /= 10;
    }
    if let Ok(s) = core::str::from_utf8(&buf[i..]) {
        write_str(s);
    }
}

unsafe fn outb(port: u16, value: u8) {
    unsafe {
        asm!(
            "out dx, al",
            in("dx") port,
            in("al") value,
            options(nomem, nostack, preserves_flags)
        );
    }
}
