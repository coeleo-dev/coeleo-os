//! Boot splash: logo + spinner on the framebuffer until the compositor takes over.

use alloc::vec::Vec;
use spin::Mutex;

use coeleo_draw::{Clip, Target};
use coeleo_theme::{ACCENT, BG, DIM};

use crate::fbterm::{self, FbInfo};

const ICON_PNG: &[u8] = include_bytes!("../../../docs/image/coeleo-icon.png");
const ICON: u32 = 96;
const SPINNER_R: i32 = 16;
const DOT: u32 = 6;
const BELOW: u32 = 24;
const ORBIT: [(i32, i32); 8] = [
    (0, -16),
    (11, -11),
    (16, 0),
    (11, 11),
    (0, 16),
    (-11, 11),
    (-16, 0),
    (-11, -11),
];

struct Splash {
    live: bool,
    frame: u32,
    icon: Vec<u32>,
    iw: u32,
    ih: u32,
    ix: u32,
    iy: u32,
    sx: u32,
    sy: u32,
}

static STATE: Mutex<Option<Splash>> = Mutex::new(None);

pub fn start() {
    let Some(fb) = fbterm::info() else {
        return;
    };
    if fb.bpp != 32 || fb.w < ICON || fb.h < ICON + BELOW + 40 {
        return;
    }
    let (ow, oh, raw) = match coeleo_image::decode_rgba(ICON_PNG) {
        Ok(v) => v,
        Err(()) => return,
    };
    let pix = match coeleo_image::scale_box(&raw, ow, oh, ICON, ICON) {
        Ok(v) => v,
        Err(()) => return,
    };
    let ix = fb.w.saturating_sub(ICON) / 2;
    let iy = fb.h.saturating_sub(ICON + BELOW + (SPINNER_R as u32) * 2) / 2;
    let sx = fb.w / 2;
    let sy = iy.saturating_add(ICON).saturating_add(BELOW);
    let sp = Splash {
        live: true,
        frame: 0,
        icon: pix,
        iw: ICON,
        ih: ICON,
        ix,
        iy,
        sx,
        sy,
    };
    paint_full(&fb, &sp);
    *STATE.lock() = Some(sp);
}

pub fn tick() {
    let mut g = STATE.lock();
    let Some(sp) = g.as_mut() else {
        return;
    };
    if !sp.live {
        return;
    }
    sp.frame = sp.frame.wrapping_add(1);
    let Some(fb) = fbterm::info() else {
        return;
    };
    paint_spinner(&fb, sp);
}

pub fn stop() {
    if let Some(sp) = STATE.lock().as_mut() {
        sp.live = false;
    }
}

fn tgt(fb: FbInfo) -> Target {
    Target {
        addr: fb.addr,
        w: fb.w,
        h: fb.h,
        pitch: fb.pitch,
    }
}

fn paint_full(fb: &FbInfo, sp: &Splash) {
    let t = tgt(*fb);
    let clip = Clip::all(fb.w, fb.h);
    for y in 0..fb.h {
        coeleo_draw::fill_span(t, 0, y, fb.w, BG);
    }
    coeleo_draw::icon_blit(t, sp.ix, sp.iy, &sp.icon, sp.iw, sp.ih, clip);
    paint_spinner(fb, sp);
}

fn paint_spinner(fb: &FbInfo, sp: &Splash) {
    let t = tgt(*fb);
    let clip = Clip::all(fb.w, fb.h);
    let r = SPINNER_R as u32 + DOT;
    let x0 = sp.sx.saturating_sub(r);
    let y0 = sp.sy.saturating_sub(r);
    let w = r.saturating_mul(2).saturating_add(DOT);
    for y in y0..y0.saturating_add(w).min(fb.h) {
        coeleo_draw::fill_span(t, x0, y, w, BG);
    }
    let hi = (sp.frame % 8) as usize;
    for (i, (dx, dy)) in ORBIT.iter().enumerate() {
        let x = (sp.sx as i32 + dx - DOT as i32 / 2).max(0) as u32;
        let y = (sp.sy as i32 + dy - DOT as i32 / 2).max(0) as u32;
        let color = if i == hi { ACCENT } else { DIM };
        coeleo_draw::fill_round(t, x, y, DOT, DOT, DOT / 2, color, clip);
    }
}
