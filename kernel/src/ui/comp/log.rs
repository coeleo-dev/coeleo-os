//! Serial traces for the compositor (test hooks).

use crate::panel;
use crate::serial;
use coeleo_theme::SHADOW_PX;

use super::state::Focus;

pub(super) fn log_panel() {
    serial::write_str("panel: h=");
    serial::write_dec_u32(panel::height());
    serial::write_str("\n");
}

pub(super) fn log_panel_mode(opaque: bool) {
    if opaque {
        serial::write_str("panel: opaque\n");
    } else {
        serial::write_str("panel: alpha\n");
    }
}

pub(super) fn log_focus(f: Focus) {
    match f {
        Focus::Term => serial::write_str("focus: term\n"),
        Focus::Files => serial::write_str("focus: files\n"),
        Focus::Desk => serial::write_str("focus: desk\n"),
    }
}

pub(super) fn log_cursor(x: i32, y: i32) {
    serial::write_str("cursor: ");
    serial::write_dec_u32(x.max(0) as u32);
    serial::write_str(",");
    serial::write_dec_u32(y.max(0) as u32);
    serial::write_str("\n");
}

pub(super) fn log_blit(w: u32, h: u32) {
    serial::write_str("blit: ");
    serial::write_dec_u32(w);
    serial::write_str("x");
    serial::write_dec_u32(h);
    serial::write_str("\n");
}

pub(super) fn log_shadow() {
    serial::write_str("shadow: ");
    serial::write_dec_u32(SHADOW_PX);
    serial::write_str("x");
    serial::write_dec_u32(SHADOW_PX);
    serial::write_str("\n");
}
