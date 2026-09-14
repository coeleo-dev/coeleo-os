//! One-level USB hub: power/reset children, Address Device with route string.

use super::Setup;
use super::host::{self, Host, Kind};
use super::slot;

const MAX_HUB_PORTS: u8 = 8;
const FEAT_PORT_RESET: u16 = 4;
const FEAT_PORT_POWER: u16 = 8;
const FEAT_C_RESET: u16 = 20;

pub(super) fn attach(hc: &mut Host, slot: u8, classify: impl Fn(&mut Host, u8)) {
    let Some(cfg) = read_config(hc, slot) else {
        slot::disable_slot(hc, slot);
        return;
    };
    let value = u16::from(cfg[5]);
    if control(
        hc,
        slot,
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
        slot::disable_slot(hc, slot);
        return;
    }
    let mut hub = [0u8; 16];
    if control(
        hc,
        slot,
        Setup {
            ty: 0xA0,
            req: 6,
            value: 0x2900,
            index: 0,
            len: 9,
        },
        &mut hub[..9],
    )
    .is_err()
    {
        slot::disable_slot(hc, slot);
        return;
    }
    let nports = hub[2].min(MAX_HUB_PORTS);
    if nports == 0 {
        slot::disable_slot(hc, slot);
        return;
    }
    if !slot::eval_hub(hc, slot, nports) {
        slot::disable_slot(hc, slot);
        return;
    }
    if let Some(dev) = hc.slots[slot as usize].as_mut() {
        dev.kind = Kind::Hub;
    }
    for p in 1..=nports {
        let _ = set_port(hc, slot, FEAT_PORT_POWER, p);
    }
    host::spin_a_bit();
    host::spin_a_bit();
    let parent_root = hc.slots[slot as usize]
        .as_ref()
        .map(|d| d.root_port)
        .unwrap_or(0);
    for p in 1..=nports {
        let Some(st) = port_status(hc, slot, p) else {
            continue;
        };
        if st & 1 == 0 {
            continue;
        }
        let _ = set_port(hc, slot, FEAT_PORT_RESET, p);
        let mut ready = false;
        for _ in 0..32 {
            host::drain(hc);
            if let Some((_, ch)) = port_status_pair(hc, slot, p) {
                if ch & (1 << 4) != 0 {
                    ready = true;
                    break;
                }
            }
            host::spin_a_bit();
        }
        if !ready {
            continue;
        }
        let _ = clear_port(hc, slot, FEAT_C_RESET, p);
        let Some(st) = port_status(hc, slot, p) else {
            continue;
        };
        if st & 1 == 0 {
            continue;
        }
        let speed = hub_speed(st);
        let Some(child) = slot::enable_slot(hc) else {
            continue;
        };
        let route = u32::from(p) & 0xF;
        if !slot::address_device(hc, child, parent_root, speed, route, slot, p) {
            slot::disable_slot(hc, child);
            continue;
        }
        classify(hc, child);
    }
}

fn hub_speed(st: u16) -> u8 {
    if st & (1 << 10) != 0 {
        3
    } else if st & (1 << 9) != 0 {
        2
    } else {
        1
    }
}

fn set_port(hc: &mut Host, slot: u8, feat: u16, port: u8) -> Result<(), ()> {
    control(
        hc,
        slot,
        Setup {
            ty: 0x23,
            req: 3,
            value: feat,
            index: u16::from(port),
            len: 0,
        },
        &mut [],
    )
    .map(|_| ())
}

fn clear_port(hc: &mut Host, slot: u8, feat: u16, port: u8) -> Result<(), ()> {
    control(
        hc,
        slot,
        Setup {
            ty: 0x23,
            req: 1,
            value: feat,
            index: u16::from(port),
            len: 0,
        },
        &mut [],
    )
    .map(|_| ())
}

fn port_status(hc: &mut Host, slot: u8, port: u8) -> Option<u16> {
    port_status_pair(hc, slot, port).map(|(st, _)| st)
}

fn port_status_pair(hc: &mut Host, slot: u8, port: u8) -> Option<(u16, u16)> {
    let mut b = [0u8; 4];
    control(
        hc,
        slot,
        Setup {
            ty: 0xA3,
            req: 0,
            value: 0,
            index: u16::from(port),
            len: 4,
        },
        &mut b,
    )
    .ok()?;
    Some((
        u16::from_le_bytes([b[0], b[1]]),
        u16::from_le_bytes([b[2], b[3]]),
    ))
}

fn read_config(hc: &mut Host, slot: u8) -> Option<[u8; 256]> {
    super::get_config(hc, slot)
}

fn control(hc: &mut Host, slot: u8, setup: Setup, data: &mut [u8]) -> Result<usize, ()> {
    slot::control_inner(hc, slot, setup, data)
}
