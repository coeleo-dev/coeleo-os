//! Root-port PORTSC: USB2 PR vs SuperSpeed WPR, skip 0xFFFFFFFF, bounded drain.

use super::host::{
    self, Host, PORT_CCS, PORT_CSC, PORT_PED, PORT_PP, PORT_PR, PORT_PRC, PORT_RW1C, PORT_SPIN,
    PORT_WPR, PORTSC0,
};

pub(super) fn reset_port(hc: &mut Host, p: u8) -> bool {
    host::drain(hc);
    let sc0 = read_port(hc, p);
    let speed = (sc0 >> 10) & 0xF;
    let mut sc = port_neutral(sc0);
    if hc.ppc {
        sc |= PORT_PP;
    }
    let first = if speed >= 4 { PORT_WPR } else { PORT_PR };
    let second = if speed >= 4 { PORT_PR } else { PORT_WPR };
    write_port(hc, p, sc | first);
    if !wait_port(hc, p, PORT_PRC, PORT_PRC) {
        write_port(hc, p, port_neutral(read_port(hc, p)) | second);
        if !wait_port(hc, p, PORT_PRC, PORT_PRC) {
            return false;
        }
    }
    write_port(
        hc,
        p,
        port_neutral(read_port(hc, p)) | PORT_PRC | PORT_CSC,
    );
    wait_port(hc, p, PORT_PED, PORT_PED)
}

fn wait_port(hc: &mut Host, p: u8, mask: u32, want: u32) -> bool {
    for i in 0..PORT_SPIN {
        if i & 0xFFF == 0 {
            host::drain(hc);
        }
        if read_port(hc, p) & mask == want {
            return true;
        }
        core::hint::spin_loop();
    }
    false
}

pub(super) fn port_exists(sc: u32) -> bool {
    sc != 0xFFFF_FFFF
}

pub(super) fn port_has_device(sc: u32) -> bool {
    port_exists(sc) && sc & PORT_CCS != 0
}

pub(super) fn port_tried(hc: &Host, p: u8) -> bool {
    matches!(p, 1..=32) && hc.tried & (1 << (p - 1)) != 0
}

pub(super) fn mark_tried(hc: &mut Host, p: u8) {
    if matches!(p, 1..=32) {
        hc.tried |= 1 << (p - 1);
    }
}

pub(super) fn read_port(hc: &Host, p: u8) -> u32 {
    host::read32(hc.cap, hc.op + PORTSC0 + u32::from(p - 1) * 0x10)
}

pub(super) fn write_port(hc: &Host, p: u8, v: u32) {
    host::write32(hc.cap, hc.op + PORTSC0 + u32::from(p - 1) * 0x10, v);
}

pub(super) fn port_neutral(v: u32) -> u32 {
    v & !PORT_RW1C
}
