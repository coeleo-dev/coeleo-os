//! PS/2 mouse packets. IRQ only fills the byte queue; this drains it.

#[derive(Clone, Copy)]
pub struct Event {
    pub dx: i16,
    pub dy: i16,
    pub left_down: bool,
    pub left_up: bool,
    pub right_down: bool,
    pub right_up: bool,
}

static mut PKT: [u8; 3] = [0; 3];
static mut PKT_N: u8 = 0;
static mut PREV_LEFT: bool = false;
static mut PREV_RIGHT: bool = false;

pub fn drain() -> Option<Event> {
    let mut last = None;
    while let Some(b) = crate::ps2::pop_mouse() {
        if let Some(ev) = push_byte(b) {
            last = Some(merge(last, ev));
        }
    }
    if let Some(ev) = crate::uhci::poll() {
        last = Some(merge(last, ev));
    }
    if let Some(ev) = crate::usb_hid::poll() {
        last = Some(merge(last, ev));
    }
    last
}

fn merge(prev: Option<Event>, ev: Event) -> Event {
    match prev {
        None => ev,
        Some(p) => Event {
            dx: p.dx.saturating_add(ev.dx),
            dy: p.dy.saturating_add(ev.dy),
            left_down: p.left_down | ev.left_down,
            left_up: p.left_up | ev.left_up,
            right_down: p.right_down | ev.right_down,
            right_up: p.right_up | ev.right_up,
        },
    }
}

fn push_byte(b: u8) -> Option<Event> {
    unsafe {
        if PKT_N == 0 && b & 0x08 == 0 {
            return None;
        }
        PKT[PKT_N as usize] = b;
        PKT_N += 1;
        if PKT_N < 3 {
            return None;
        }
        PKT_N = 0;
        let b0 = PKT[0];
        let b1 = PKT[1];
        let b2 = PKT[2];
        if b0 & 0xc0 != 0 {
            return None;
        }
        let mut dx = b1 as i16;
        if b0 & 0x10 != 0 {
            dx -= 256;
        }
        let mut dy = b2 as i16;
        if b0 & 0x20 != 0 {
            dy -= 256;
        }
        // PS/2 Y is positive-up; the compositor Y axis is positive-down.
        dy = -dy;
        let left = b0 & 1 != 0;
        let right = b0 & 2 != 0;
        let was = PREV_LEFT;
        let was_r = PREV_RIGHT;
        PREV_LEFT = left;
        PREV_RIGHT = right;
        Some(Event {
            dx,
            dy,
            left_down: left && !was,
            left_up: !left && was,
            right_down: right && !was_r,
            right_up: !right && was_r,
        })
    }
}

pub fn empty() -> bool {
    crate::ps2::mouse_empty() && crate::uhci::idle() && crate::usb_hid::idle()
}
