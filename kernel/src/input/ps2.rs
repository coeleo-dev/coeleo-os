//! i8042 keyboard + aux (mouse). IRQ handlers only enqueue; no paint.

use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use x86_64::instructions::port::Port;

const DATA: u16 = 0x60;
const STATUS: u16 = 0x64;

const QUEUE_CAP: usize = 64;
const SPIN: u32 = 100_000;

const STATUS_OUT: u8 = 1;
const STATUS_IN: u8 = 2;
const STATUS_AUX: u8 = 1 << 5;

static mut KBD_Q: [u8; QUEUE_CAP] = [0; QUEUE_CAP];
static KBD_HEAD: AtomicUsize = AtomicUsize::new(0);
static KBD_TAIL: AtomicUsize = AtomicUsize::new(0);

static mut MOUSE_Q: [u8; QUEUE_CAP] = [0; QUEUE_CAP];
static MOUSE_HEAD: AtomicUsize = AtomicUsize::new(0);
static MOUSE_TAIL: AtomicUsize = AtomicUsize::new(0);

static MOUSE_OK: AtomicBool = AtomicBool::new(false);

/// Program the controller: keyboard + mouse IRQs, scancode translation.
pub fn init() {
    unsafe {
        write_command(0xAD);
        write_command(0xA7);
        flush_output();

        write_command(0x20);
        let mut config = read_data();
        config |= 1 << 0;
        config |= 1 << 1;
        config &= !(1 << 4);
        config &= !(1 << 5);
        config |= 1 << 6;
        write_command(0x60);
        write_data(config);

        write_command(0xA8);
        write_command(0xAE);

        write_command(0xD4);
        write_data(0xF4);
        if wait_output_full_ok() {
            let ack = Port::<u8>::new(DATA).read();
            MOUSE_OK.store(ack == 0xFA, Ordering::Release);
        }
        if !MOUSE_OK.load(Ordering::Acquire) {
            // USB mouse is probed later; compositor logs `mouse: none` if both fail.
        }
    }
}

pub fn mouse_present() -> bool {
    MOUSE_OK.load(Ordering::Acquire)
}

/// Drain the data port, routing by status bit 5. Called from IRQ1 and IRQ12.
pub fn irq() {
    for _ in 0..16 {
        let status = unsafe { Port::<u8>::new(STATUS).read() };
        if status & STATUS_OUT == 0 {
            break;
        }
        let byte = unsafe { Port::<u8>::new(DATA).read() };
        if status & STATUS_AUX != 0 {
            push_mouse(byte);
        } else {
            push_kbd(byte);
        }
    }
}

pub fn pop() -> Option<u8> {
    pop_ring(&raw const KBD_Q, &KBD_HEAD, &KBD_TAIL)
}

pub fn pop_mouse() -> Option<u8> {
    pop_ring(&raw const MOUSE_Q, &MOUSE_HEAD, &MOUSE_TAIL)
}

pub fn is_empty() -> bool {
    kbd_empty() && mouse_empty()
}

pub fn kbd_empty() -> bool {
    KBD_HEAD.load(Ordering::Acquire) == KBD_TAIL.load(Ordering::Acquire)
}

pub fn mouse_empty() -> bool {
    MOUSE_HEAD.load(Ordering::Acquire) == MOUSE_TAIL.load(Ordering::Acquire)
}

fn push_kbd(byte: u8) {
    push_ring(&raw mut KBD_Q, &KBD_HEAD, &KBD_TAIL, byte);
}

fn push_mouse(byte: u8) {
    push_ring(&raw mut MOUSE_Q, &MOUSE_HEAD, &MOUSE_TAIL, byte);
}

fn push_ring(q: *mut [u8; QUEUE_CAP], head: &AtomicUsize, tail: &AtomicUsize, byte: u8) {
    let h = head.load(Ordering::Relaxed);
    let next = (h + 1) % QUEUE_CAP;
    if next == tail.load(Ordering::Acquire) {
        let t = tail.load(Ordering::Acquire);
        tail.store((t + 1) % QUEUE_CAP, Ordering::Release);
    }
    unsafe {
        (*q)[h] = byte;
    }
    head.store(next, Ordering::Release);
}

fn pop_ring(q: *const [u8; QUEUE_CAP], head: &AtomicUsize, tail: &AtomicUsize) -> Option<u8> {
    let t = tail.load(Ordering::Relaxed);
    let h = head.load(Ordering::Acquire);
    if t == h {
        return None;
    }
    let byte = unsafe { (*q)[t] };
    tail.store((t + 1) % QUEUE_CAP, Ordering::Release);
    Some(byte)
}

unsafe fn write_command(cmd: u8) {
    wait_input_empty();
    unsafe {
        Port::<u8>::new(STATUS).write(cmd);
    }
}

unsafe fn write_data(data: u8) {
    wait_input_empty();
    unsafe {
        Port::<u8>::new(DATA).write(data);
    }
}

unsafe fn read_data() -> u8 {
    wait_output_full();
    unsafe { Port::<u8>::new(DATA).read() }
}

unsafe fn flush_output() {
    for _ in 0..32 {
        let status = unsafe { Port::<u8>::new(STATUS).read() };
        if status & STATUS_OUT == 0 {
            break;
        }
        let _ = unsafe { Port::<u8>::new(DATA).read() };
    }
}

fn wait_input_empty() {
    for _ in 0..SPIN {
        let status = unsafe { Port::<u8>::new(STATUS).read() };
        if status & STATUS_IN == 0 {
            return;
        }
    }
}

fn wait_output_full() {
    let _ = wait_output_full_ok();
}

fn wait_output_full_ok() -> bool {
    for _ in 0..SPIN {
        let status = unsafe { Port::<u8>::new(STATUS).read() };
        if status & STATUS_OUT != 0 {
            return true;
        }
    }
    false
}
