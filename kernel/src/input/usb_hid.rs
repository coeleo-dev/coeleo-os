//! xHCI HID boot protocol: keyboard 8-byte reports and mouse like UHCI.

use core::sync::atomic::{AtomicBool, Ordering};

use spin::Mutex;

use crate::kbd::{self, Key};
use crate::mouse::Event;
use crate::xhci::{self, Dev, Setup};

const MAX_HID: usize = 8;

struct Iface {
    dev: Dev,
    ep: u8,
    proto: u8,
    prev_keys: [u8; 8],
    prev_left: bool,
    prev_right: bool,
}

struct State {
    ifaces: [Option<Iface>; MAX_HID],
    mouse: bool,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);
static MOUSE: AtomicBool = AtomicBool::new(false);

pub fn init() {
    if STATE.lock().is_some() {
        return;
    }
    let mut found = [Dev { hc: 0, slot: 0 }; MAX_HID];
    let n = xhci::hid_devs(&mut found);
    let mut st = State {
        ifaces: [None, None, None, None, None, None, None, None],
        mouse: false,
    };
    let mut k = 0usize;
    for i in 0..n {
        k += attach(&mut st, found[i], k);
    }
    let mouse = st.mouse;
    *STATE.lock() = Some(st);
    MOUSE.store(mouse, Ordering::Release);
}

pub fn mouse_present() -> bool {
    MOUSE.load(Ordering::Acquire)
}

pub fn idle() -> bool {
    true
}

pub fn poll() -> Option<Event> {
    let mut g = STATE.try_lock()?;
    let st = g.as_mut()?;
    let mut mouse: Option<Event> = None;
    for slot in st.ifaces.iter_mut().flatten() {
        let mut buf = [0u8; 8];
        let Some(n) = xhci::interrupt_poll(slot.dev, slot.ep, &mut buf) else {
            continue;
        };
        if slot.proto == 2 {
            if let Some(ev) = decode_mouse(slot, &buf, n) {
                mouse = Some(match mouse {
                    None => ev,
                    Some(p) => Event {
                        dx: p.dx.saturating_add(ev.dx),
                        dy: p.dy.saturating_add(ev.dy),
                        left_down: p.left_down | ev.left_down,
                        left_up: p.left_up | ev.left_up,
                        right_down: p.right_down | ev.right_down,
                        right_up: p.right_up | ev.right_up,
                    },
                });
            }
        } else if slot.proto == 1 && n >= 8 {
            dispatch_kbd(slot, &buf);
        }
    }
    mouse
}

fn attach(st: &mut State, dev: Dev, start: usize) -> usize {
    let mut hdr = [0u8; 9];
    if xhci::control(
        dev,
        Setup {
            ty: 0x80,
            req: 6,
            value: 0x0200,
            index: 0,
            len: 9,
        },
        &mut hdr,
    )
    .is_err()
    {
        return 0;
    }
    let total = u16::from_le_bytes([hdr[2], hdr[3]]).min(256) as usize;
    if total < 9 {
        return 0;
    }
    let mut cfg = [0u8; 256];
    if xhci::control(
        dev,
        Setup {
            ty: 0x80,
            req: 6,
            value: 0x0200,
            index: 0,
            len: total as u16,
        },
        &mut cfg[..total],
    )
    .is_err()
    {
        return 0;
    }
    let value = u16::from(cfg[5]);
    let hid = parse_hid(&cfg, total);
    if hid.iter().all(|h| h.is_none()) {
        return 0;
    }
    if xhci::control(
        dev,
        Setup {
            ty: 0x00,
            req: 9,
            value,
            index: 0,
            len: 0,
        },
        &mut [],
    )
    .is_err()
    {
        return 0;
    }
    let mut eps = [(0u8, 0u16); 2];
    let mut n_ep = 0usize;
    let mut added = 0usize;
    for h in hid.iter().flatten() {
        if set_protocol(dev, h.iface).is_err() {
            continue;
        }
        let _ = set_idle(dev, h.iface);
        if n_ep < 2 {
            eps[n_ep] = (h.ep, h.maxpkt);
            n_ep += 1;
        }
        if start + added < MAX_HID {
            if h.proto == 2 {
                st.mouse = true;
            }
            st.ifaces[start + added] = Some(Iface {
                dev,
                ep: h.ep,
                proto: h.proto,
                prev_keys: [0; 8],
                prev_left: false,
                prev_right: false,
            });
            added += 1;
        }
    }
    if n_ep == 0 {
        return 0;
    }
    if xhci::configure_interrupt(dev, &eps[..n_ep]).is_err() {
        for i in 0..added {
            st.ifaces[start + i] = None;
        }
        return 0;
    }
    added
}

struct HidIf {
    iface: u16,
    proto: u8,
    ep: u8,
    maxpkt: u16,
}

fn parse_hid(cfg: &[u8; 256], total: usize) -> [Option<HidIf>; 2] {
    let mut out = [None, None];
    let mut n = 0usize;
    let mut i = 9usize;
    let mut want = false;
    let mut iface = 0u16;
    let mut proto = 0u8;
    let mut ep = 0u8;
    let mut maxpkt = 8u16;
    while i + 2 <= total {
        let len = cfg[i] as usize;
        if len < 2 || i + len > total {
            break;
        }
        match cfg[i + 1] {
            4 if len >= 9 => {
                if want && ep != 0 && n < 2 {
                    out[n] = Some(HidIf {
                        iface,
                        proto,
                        ep,
                        maxpkt,
                    });
                    n += 1;
                }
                want = cfg[i + 5] == 3 && cfg[i + 6] == 1 && (cfg[i + 7] == 1 || cfg[i + 7] == 2);
                iface = u16::from(cfg[i + 2]);
                proto = cfg[i + 7];
                ep = 0;
                maxpkt = 8;
            }
            5 if want && len >= 7 => {
                let addr = cfg[i + 2];
                let attr = cfg[i + 3] & 3;
                if attr == 3 && addr & 0x80 != 0 && ep == 0 {
                    ep = addr;
                    maxpkt = u16::from_le_bytes([cfg[i + 4], cfg[i + 5]]).max(8);
                }
            }
            _ => {}
        }
        i += len;
    }
    if want && ep != 0 && n < 2 {
        out[n] = Some(HidIf {
            iface,
            proto,
            ep,
            maxpkt,
        });
    }
    out
}

fn set_protocol(dev: Dev, iface: u16) -> Result<(), ()> {
    xhci::control(
        dev,
        Setup {
            ty: 0x21,
            req: 0x0B,
            value: 0,
            index: iface,
            len: 0,
        },
        &mut [],
    )
    .map(|_| ())
}

fn set_idle(dev: Dev, iface: u16) -> Result<(), ()> {
    xhci::control(
        dev,
        Setup {
            ty: 0x21,
            req: 0x0A,
            value: 0,
            index: iface,
            len: 0,
        },
        &mut [],
    )
    .map(|_| ())
}

fn decode_mouse(slot: &mut Iface, report: &[u8], n: usize) -> Option<Event> {
    if n < 3 {
        return None;
    }
    let dx = report[1] as i8 as i16;
    let dy = report[2] as i8 as i16;
    let left = report[0] & 1 != 0;
    let right = report[0] & 2 != 0;
    let was = slot.prev_left;
    let was_r = slot.prev_right;
    slot.prev_left = left;
    slot.prev_right = right;
    if dx == 0 && dy == 0 && left == was && right == was_r {
        return None;
    }
    Some(Event {
        dx,
        dy,
        left_down: left && !was,
        left_up: !left && was,
        right_down: right && !was_r,
        right_up: !right && was_r,
    })
}

fn dispatch_kbd(slot: &mut Iface, report: &[u8; 8]) {
    if report[2] == 0x01 {
        return;
    }
    let mods = report[0];
    let prev = slot.prev_keys;
    slot.prev_keys = *report;
    let alt = mods & 0x44 != 0;
    for &code in &report[2..8] {
        if code == 0 || prev[2..8].contains(&code) {
            continue;
        }
        if code == 0x2C && alt {
            kbd::dispatch(Key::AltSpace);
            continue;
        }
        if let Some(key) = map_usage(code, mods) {
            kbd::dispatch(key);
        }
    }
}

fn map_usage(usage: u8, mods: u8) -> Option<Key> {
    let shift = mods & 0x22 != 0;
    let ctrl = mods & 0x11 != 0;
    match usage {
        0x28 => Some(Key::Enter),
        0x29 => Some(Key::Esc),
        0x2A => Some(Key::Backspace),
        0x4C => Some(Key::Delete),
        0x2B => Some(Key::Tab),
        0x2C => Some(Key::Char(b' ')),
        0x4F => Some(Key::Right),
        0x50 => Some(Key::Left),
        0x51 => Some(Key::Down),
        0x52 => Some(Key::Up),
        0x04..=0x1D => {
            let mut c = b'a' + (usage - 0x04);
            if ctrl {
                return Some(Key::Char(c - b'a' + 1));
            }
            if shift {
                c = c.to_ascii_uppercase();
            }
            Some(Key::Char(c))
        }
        0x1E..=0x26 => {
            let digits = b"123456789";
            let shifted = b"!@#$%^&*(";
            let i = (usage - 0x1E) as usize;
            Some(Key::Char(if shift { shifted[i] } else { digits[i] }))
        }
        0x27 => Some(Key::Char(if shift { b')' } else { b'0' })),
        0x2D => Some(Key::Char(if shift { b'_' } else { b'-' })),
        0x2E => Some(Key::Char(if shift { b'+' } else { b'=' })),
        0x2F => Some(Key::Char(if shift { b'{' } else { b'[' })),
        0x30 => Some(Key::Char(if shift { b'}' } else { b']' })),
        0x31 => Some(Key::Char(if shift { b'|' } else { b'\\' })),
        0x33 => Some(Key::Char(if shift { b':' } else { b';' })),
        0x34 => Some(Key::Char(if shift { b'"' } else { b'\'' })),
        0x36 => Some(Key::Char(if shift { b'<' } else { b',' })),
        0x37 => Some(Key::Char(if shift { b'>' } else { b'.' })),
        0x38 => Some(Key::Char(if shift { b'?' } else { b'/' })),
        _ => None,
    }
}
