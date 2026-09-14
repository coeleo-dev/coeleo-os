//! xHCI host: up to two controllers, eight slots, poll rings. No MSI.

use spin::Mutex;

use self::host::{Host, Kind, MAX_HC};
use self::slot::IntSpec;

mod host;
mod hub;
mod port;
mod slot;

pub struct Setup {
    pub ty: u8,
    pub req: u8,
    pub value: u16,
    pub index: u16,
    pub len: u16,
}

#[derive(Clone, Copy)]
pub struct Dev {
    pub hc: u8,
    pub slot: u8,
}

static HOSTS: Mutex<[Option<Host>; MAX_HC]> = Mutex::new([None, None]);

pub fn init() {
    let mut g = HOSTS.lock();
    if g.iter().any(|h| h.is_some()) {
        return;
    }
    *g = host::probe_all();
}

pub fn present() -> bool {
    HOSTS.lock().iter().any(|h| h.is_some())
}

pub fn enumerate() {
    let mut g = HOSTS.lock();
    for hc in g.iter_mut().flatten() {
        enumerate_host(hc);
    }
}

pub fn msc_devs(out: &mut [Dev]) -> usize {
    let g = HOSTS.lock();
    let mut n = 0usize;
    for (i, hc) in g.iter().enumerate() {
        let Some(hc) = hc else {
            continue;
        };
        for (s, slot) in hc.slots.iter().enumerate() {
            if slot.as_ref().is_some_and(|d| d.kind == Kind::Msc) {
                if n < out.len() {
                    out[n] = Dev {
                        hc: i as u8,
                        slot: s as u8,
                    };
                    n += 1;
                }
            }
        }
    }
    n
}

pub fn hid_devs(out: &mut [Dev]) -> usize {
    let g = HOSTS.lock();
    let mut n = 0usize;
    for (i, hc) in g.iter().enumerate() {
        let Some(hc) = hc else {
            continue;
        };
        for (s, slot) in hc.slots.iter().enumerate() {
            if slot.as_ref().is_some_and(|d| d.kind == Kind::Hid) {
                if n < out.len() {
                    out[n] = Dev {
                        hc: i as u8,
                        slot: s as u8,
                    };
                    n += 1;
                }
            }
        }
    }
    n
}

pub fn release(dev: Dev) {
    let mut g = HOSTS.lock();
    let Some(hc) = g.get_mut(dev.hc as usize).and_then(|h| h.as_mut()) else {
        return;
    };
    if hc.slots.get(dev.slot as usize).is_some_and(|s| s.is_some()) {
        slot::disable_slot(hc, dev.slot);
    }
}

pub fn control(dev: Dev, setup: Setup, data: &mut [u8]) -> Result<usize, ()> {
    let mut g = HOSTS.lock();
    let hc = g
        .get_mut(dev.hc as usize)
        .and_then(|h| h.as_mut())
        .ok_or(())?;
    slot::control_inner(hc, dev.slot, setup, data)
}

pub fn configure_bulk(
    dev: Dev,
    ep_out: u8,
    ep_in: u8,
    max_out: u16,
    max_in: u16,
) -> Result<(), ()> {
    let mut g = HOSTS.lock();
    let hc = g
        .get_mut(dev.hc as usize)
        .and_then(|h| h.as_mut())
        .ok_or(())?;
    slot::config_bulk(hc, dev.slot, ep_out, ep_in, max_out, max_in)
}

pub fn bulk_out(dev: Dev, buf: &[u8]) -> Result<(), ()> {
    let mut g = HOSTS.lock();
    let hc = g
        .get_mut(dev.hc as usize)
        .and_then(|h| h.as_mut())
        .ok_or(())?;
    slot::bounce_out(hc, dev.slot, buf)
}

pub fn bulk_in(dev: Dev, buf: &mut [u8]) -> Result<(), ()> {
    let mut g = HOSTS.lock();
    let hc = g
        .get_mut(dev.hc as usize)
        .and_then(|h| h.as_mut())
        .ok_or(())?;
    slot::bounce_in(hc, dev.slot, buf)
}

pub fn configure_interrupt(dev: Dev, eps: &[(u8, u16)]) -> Result<(), ()> {
    let mut specs = [IntSpec { ep: 0, maxpkt: 0 }, IntSpec { ep: 0, maxpkt: 0 }];
    let n = eps.len().min(2);
    if n == 0 {
        return Err(());
    }
    for i in 0..n {
        specs[i] = IntSpec {
            ep: eps[i].0,
            maxpkt: eps[i].1,
        };
    }
    let mut g = HOSTS.lock();
    let hc = g
        .get_mut(dev.hc as usize)
        .and_then(|h| h.as_mut())
        .ok_or(())?;
    slot::config_interrupt(hc, dev.slot, &specs[..n])
}

pub fn interrupt_poll(dev: Dev, ep: u8, buf: &mut [u8]) -> Option<usize> {
    let mut g = HOSTS.try_lock()?;
    let hc = g.get_mut(dev.hc as usize).and_then(|h| h.as_mut())?;
    let dci = host::dci(ep);
    slot::interrupt_poll(hc, dev.slot, dci, buf)
}

fn enumerate_host(hc: &mut Host) {
    for _ in 0..8 {
        let mut progress = false;
        for p in 1..=hc.max_ports {
            if port::port_tried(hc, p) {
                continue;
            }
            let sc = port::read_port(hc, p);
            if !port::port_has_device(sc) {
                continue;
            }
            if !port::reset_port(hc, p) {
                port::mark_tried(hc, p);
                continue;
            }
            host::drain(hc);
            let speed = ((port::read_port(hc, p) >> 10) & 0xF) as u8;
            let Some(slot) = slot::enable_slot(hc) else {
                port::mark_tried(hc, p);
                continue;
            };
            if slot::address_device(hc, slot, p, speed, 0, 0, 0) {
                port::mark_tried(hc, p);
                progress = true;
                classify(hc, slot);
            } else {
                slot::disable_slot(hc, slot);
                port::mark_tried(hc, p);
            }
        }
        if !progress {
            break;
        }
        host::spin_a_bit();
    }
}

fn classify(hc: &mut Host, slot: u8) {
    let mut dev = [0u8; 18];
    if control_on(
        hc,
        slot,
        Setup {
            ty: 0x80,
            req: 6,
            value: 0x0100,
            index: 0,
            len: 18,
        },
        &mut dev,
    )
    .is_err()
    {
        slot::disable_slot(hc, slot);
        return;
    }
    if dev[4] == 9 {
        let nested = hc.slots[slot as usize]
            .as_ref()
            .is_some_and(|s| s.parent != 0);
        if nested {
            slot::disable_slot(hc, slot);
            return;
        }
        hub::attach(hc, slot, classify);
        return;
    }
    let Some(cfg) = get_config(hc, slot) else {
        slot::disable_slot(hc, slot);
        return;
    };
    if has_msc(&cfg) {
        if let Some(s) = hc.slots[slot as usize].as_mut() {
            s.kind = Kind::Msc;
        }
        return;
    }
    if has_hid_boot(&cfg) {
        if let Some(s) = hc.slots[slot as usize].as_mut() {
            s.kind = Kind::Hid;
        }
        return;
    }
    slot::disable_slot(hc, slot);
}

fn control_on(hc: &mut Host, slot: u8, setup: Setup, data: &mut [u8]) -> Result<usize, ()> {
    slot::control_inner(hc, slot, setup, data)
}

fn get_config(hc: &mut Host, slot: u8) -> Option<[u8; 256]> {
    let mut hdr = [0u8; 9];
    control_on(
        hc,
        slot,
        Setup {
            ty: 0x80,
            req: 6,
            value: 0x0200,
            index: 0,
            len: 9,
        },
        &mut hdr,
    )
    .ok()?;
    let total = u16::from_le_bytes([hdr[2], hdr[3]]).min(256) as usize;
    if total < 9 {
        return None;
    }
    let mut cfg = [0u8; 256];
    control_on(
        hc,
        slot,
        Setup {
            ty: 0x80,
            req: 6,
            value: 0x0200,
            index: 0,
            len: total as u16,
        },
        &mut cfg[..total],
    )
    .ok()?;
    Some(cfg)
}

fn walk_ifaces(cfg: &[u8; 256], mut f: impl FnMut(u8, u8, u8)) {
    let total = u16::from_le_bytes([cfg[2], cfg[3]]).min(256) as usize;
    let mut i = 9usize;
    while i + 2 <= total {
        let len = cfg[i] as usize;
        if len < 2 || i + len > total {
            break;
        }
        if cfg[i + 1] == 4 && len >= 9 {
            f(cfg[i + 5], cfg[i + 6], cfg[i + 7]);
        }
        i += len;
    }
}

fn has_msc(cfg: &[u8; 256]) -> bool {
    let mut yes = false;
    walk_ifaces(cfg, |class, sub, proto| {
        if class == 8 && sub == 6 && proto == 0x50 {
            yes = true;
        }
    });
    yes
}

fn has_hid_boot(cfg: &[u8; 256]) -> bool {
    let mut yes = false;
    walk_ifaces(cfg, |class, sub, proto| {
        if class == 3 && sub == 1 && (proto == 1 || proto == 2) {
            yes = true;
        }
    });
    yes
}
