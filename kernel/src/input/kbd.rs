//! Shared PS/2 decode. TCB-adjacent: no port I/O, only the crate state.

use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use pc_keyboard::{DecodedKey, HandleControl, KeyCode, KeyState, Keyboard, ScancodeSet1, layouts};
use spin::Mutex;

static KBD: Mutex<Keyboard<layouts::Us104Key, ScancodeSet1>> = Mutex::new(Keyboard::new(
    ScancodeSet1::new(),
    layouts::Us104Key,
    HandleControl::MapLettersToUnicode,
));

const QUEUE_CAP: usize = 64;
static mut BYTES: [u8; QUEUE_CAP] = [0; QUEUE_CAP];
static HEAD: AtomicUsize = AtomicUsize::new(0);
static TAIL: AtomicUsize = AtomicUsize::new(0);

static LALT: AtomicBool = AtomicBool::new(false);
static RALT: AtomicBool = AtomicBool::new(false);
static LSHIFT: AtomicBool = AtomicBool::new(false);
static RSHIFT: AtomicBool = AtomicBool::new(false);

pub enum Key {
    Tab,
    Esc,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    Enter,
    Backspace,
    Delete,
    Char(u8),
    AltSpace,
}

/// Compositor / `coeleo>` policy shared by PS/2 and HID boot keyboard.
pub fn dispatch(key: Key) {
    match key {
        Key::AltSpace => crate::comp::irq_krunner(),
        Key::Tab => {
            if !crate::comp::irq_tab() {
                enqueue_or_kill(b'\t');
            }
        }
        Key::Esc => {
            let _ = crate::comp::irq_esc();
        }
        Key::Up => {
            if !crate::comp::irq_files_key(crate::comp::FilesKey::Up) {
                enqueue_csi(b'A');
            }
        }
        Key::Down => {
            if !crate::comp::irq_files_key(crate::comp::FilesKey::Down) {
                enqueue_csi(b'B');
            }
        }
        Key::Left => {
            if !crate::comp::irq_files_key(crate::comp::FilesKey::Left) {
                enqueue_csi(b'D');
            }
        }
        Key::Right => {
            if !crate::comp::irq_files_key(crate::comp::FilesKey::Right) {
                enqueue_csi(b'C');
            }
        }
        Key::Home => enqueue_csi(b'H'),
        Key::End => enqueue_csi(b'F'),
        Key::Enter => {
            if crate::comp::irq_files_key(crate::comp::FilesKey::Enter) {
                return;
            }
            enqueue_or_kill(b'\n');
        }
        Key::Delete => {
            if !crate::comp::irq_files_key(crate::comp::FilesKey::Delete) {
                enqueue_seq(b"\x1b[3~");
            }
        }
        Key::Backspace => {
            if crate::comp::irq_files_key(crate::comp::FilesKey::Backspace) {
                return;
            }
            enqueue_or_kill(0x08);
        }
        Key::Char(b) => {
            if b == b'\n' || b == b'\r' {
                dispatch(Key::Enter);
                return;
            }
            if b == 0x08 {
                dispatch(Key::Backspace);
                return;
            }
            if b == 0x1B {
                dispatch(Key::Esc);
                return;
            }
            if crate::comp::irq_runner_char(b) {
                return;
            }
            if crate::comp::irq_client_char(b) {
                return;
            }
            if crate::comp::irq_files_char(b) {
                return;
            }
            if crate::comp::files_focused() && b != 3 {
                return;
            }
            enqueue_or_kill(b);
        }
    }
}

/// One ASCII byte, or `None` if the scancode is not yet a character.
pub fn push(scancode: u8) -> Option<u8> {
    let event = {
        let mut kbd = KBD.lock();
        match kbd.add_byte(scancode) {
            Ok(Some(ev)) => {
                note_shift(ev.code, ev.state);
                kbd.process_keyevent(ev)
            }
            _ => None,
        }
    };
    match event {
        Some(DecodedKey::Unicode(c)) if c.is_ascii() => Some(c as u8),
        Some(DecodedKey::RawKey(KeyCode::Backspace)) => Some(0x08),
        Some(DecodedKey::RawKey(KeyCode::Return | KeyCode::NumpadEnter)) => Some(b'\n'),
        Some(DecodedKey::RawKey(KeyCode::Oem7)) => Some(oem7_byte()),
        _ => None,
    }
}

/// Drain the scancode queue into decoded bytes. Ctrl+C (0x03) kills the
/// foreground child and is not enqueued. Tab/arrows go to the compositor
/// when Files or an overlay has focus; otherwise Tab and arrows reach stdin.
pub fn drain_ps2() {
    while let Some(sc) = crate::ps2::pop() {
        let (event, krunner) = {
            let mut kbd = KBD.lock();
            match kbd.add_byte(sc) {
                Ok(Some(ev)) => {
                    note_alt(ev.code, ev.state);
                    note_shift(ev.code, ev.state);
                    if ev.code == KeyCode::Spacebar && ev.state == KeyState::Down && alt_down() {
                        (None, true)
                    } else {
                        (kbd.process_keyevent(ev), false)
                    }
                }
                _ => (None, false),
            }
        };
        if krunner {
            dispatch(Key::AltSpace);
            continue;
        }
        match event {
            Some(DecodedKey::Unicode('\t')) => dispatch(Key::Tab),
            Some(DecodedKey::RawKey(KeyCode::Escape)) => dispatch(Key::Esc),
            Some(DecodedKey::RawKey(KeyCode::ArrowUp)) => dispatch(Key::Up),
            Some(DecodedKey::RawKey(KeyCode::ArrowDown)) => dispatch(Key::Down),
            Some(DecodedKey::RawKey(KeyCode::ArrowLeft)) => dispatch(Key::Left),
            Some(DecodedKey::RawKey(KeyCode::ArrowRight)) => dispatch(Key::Right),
            Some(DecodedKey::RawKey(KeyCode::Home)) => dispatch(Key::Home),
            Some(DecodedKey::RawKey(KeyCode::End)) => dispatch(Key::End),
            Some(DecodedKey::RawKey(KeyCode::Return | KeyCode::NumpadEnter)) => {
                dispatch(Key::Enter);
            }
            Some(DecodedKey::RawKey(KeyCode::Backspace)) => {
                dispatch(Key::Backspace);
            }
            Some(DecodedKey::RawKey(KeyCode::Delete)) => {
                dispatch(Key::Delete);
            }
            Some(DecodedKey::Unicode(c)) if c.is_ascii() => dispatch(Key::Char(c as u8)),
            Some(DecodedKey::RawKey(KeyCode::Oem7)) => dispatch(Key::Char(oem7_byte())),
            _ => {}
        }
    }
}

fn note_alt(code: KeyCode, state: KeyState) {
    let down = state == KeyState::Down;
    match code {
        KeyCode::LAlt => LALT.store(down, Ordering::Release),
        KeyCode::RAltGr => RALT.store(down, Ordering::Release),
        _ => {}
    }
}

fn note_shift(code: KeyCode, state: KeyState) {
    let down = state == KeyState::Down;
    match code {
        KeyCode::LShift => LSHIFT.store(down, Ordering::Release),
        KeyCode::RShift => RSHIFT.store(down, Ordering::Release),
        _ => {}
    }
}

fn alt_down() -> bool {
    LALT.load(Ordering::Acquire) || RALT.load(Ordering::Acquire)
}

fn shift_down() -> bool {
    LSHIFT.load(Ordering::Acquire) || RSHIFT.load(Ordering::Acquire)
}

/// ANSI `\\`/`|` is scancode 0x2B. pc-keyboard Set1 labels that Oem7; Us104Key
/// only maps Oem5 (ISO 0x56).
fn oem7_byte() -> u8 {
    if shift_down() { b'|' } else { b'\\' }
}

fn enqueue_csi(final_byte: u8) {
    enqueue_or_kill(0x1B);
    enqueue_or_kill(b'[');
    enqueue_or_kill(final_byte);
}

fn enqueue_seq(s: &[u8]) {
    for &b in s {
        enqueue_or_kill(b);
    }
}

fn enqueue_or_kill(b: u8) {
    if b == 3 && crate::sched::kill_foreground() {
        return;
    }
    enqueue(b);
    crate::sched::wake_stdin();
}

pub fn enqueue_byte(b: u8) {
    enqueue_or_kill(b);
}

pub fn enqueue_str(s: &str) {
    for &b in s.as_bytes() {
        enqueue_or_kill(b);
    }
}

pub fn pop_byte() -> Option<u8> {
    let tail = TAIL.load(Ordering::Relaxed);
    let head = HEAD.load(Ordering::Acquire);
    if tail == head {
        return None;
    }
    let byte = unsafe { BYTES[tail] };
    TAIL.store((tail + 1) % QUEUE_CAP, Ordering::Release);
    Some(byte)
}

pub fn bytes_empty() -> bool {
    HEAD.load(Ordering::Acquire) == TAIL.load(Ordering::Acquire)
}

fn enqueue(byte: u8) {
    let head = HEAD.load(Ordering::Relaxed);
    let next = (head + 1) % QUEUE_CAP;
    if next == TAIL.load(Ordering::Acquire) {
        return;
    }
    unsafe {
        BYTES[head] = byte;
    }
    HEAD.store(next, Ordering::Release);
}
