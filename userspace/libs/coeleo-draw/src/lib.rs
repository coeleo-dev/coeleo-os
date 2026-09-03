//! Immediate-mode paint primitives for kernel chrome and `libcoeleoui`.
//! `no_std`, no alloc.

#![no_std]

pub mod font;

use coeleo_theme::{
    BG, DANGER, DIM, GAP, HIGHLIGHT, HOVER, OVERLAY, OVERLAY_ALPHA, PAD, RADIUS_SM, SHADOW, SURFACE,
    TEXT,
};

pub const FONT_W: u32 = font::WIDTH;
pub const FONT_H: u32 = font::HEIGHT;
pub const ICON: u32 = 16;
pub const ROW: u32 = FONT_H + GAP;

const SDF_S: i32 = 8;

#[derive(Clone, Copy)]
pub struct Target {
    pub addr: usize,
    pub w: u32,
    pub h: u32,
    pub pitch: u32,
}

#[derive(Clone, Copy)]
pub struct Clip {
    pub x0: u32,
    pub y0: u32,
    pub x1: u32,
    pub y1: u32,
}

impl Clip {
    pub fn all(w: u32, h: u32) -> Self {
        Self {
            x0: 0,
            y0: 0,
            x1: w,
            y1: h,
        }
    }

    pub fn contains(self, x: u32, y: u32) -> bool {
        x >= self.x0 && x < self.x1 && y >= self.y0 && y < self.y1
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Icon {
    Folder,
    Terminal,
    Search,
    App,
    Close,
    Back,
    Forward,
}

pub fn put(t: Target, x: u32, y: u32, color: u32) {
    if x >= t.w || y >= t.h {
        return;
    }
    let off = ((y * t.pitch) / 4 + x) as usize;
    unsafe {
        *(t.addr as *mut u32).add(off) = color;
    }
}

pub fn get(t: Target, x: u32, y: u32) -> u32 {
    if x >= t.w || y >= t.h {
        return 0;
    }
    let off = ((y * t.pitch) / 4 + x) as usize;
    unsafe { *(t.addr as *const u32).add(off) }
}

pub fn fill_span(t: Target, x: u32, y: u32, w: u32, color: u32) {
    if w == 0 || x >= t.w || y >= t.h {
        return;
    }
    let w = w.min(t.w.saturating_sub(x));
    let off = ((y * t.pitch) / 4 + x) as usize;
    unsafe {
        let p = t.addr as *mut u32;
        for i in 0..w {
            *p.add(off + i as usize) = color;
        }
    }
}

pub fn fill_span_blend(t: Target, x: u32, y: u32, w: u32, color: u32, a: u8) {
    if a == 255 {
        fill_span(t, x, y, w, color);
        return;
    }
    if w == 0 || x >= t.w || y >= t.h {
        return;
    }
    let w = w.min(t.w.saturating_sub(x));
    for i in 0..w {
        let px = x + i;
        put(t, px, y, blend(get(t, px, y), color, a));
    }
}

fn sdf8(px: u32, py: u32, rx: u32, ry: u32, rw: u32, rh: u32, radius: u32) -> i32 {
    let r = (radius.min(rw / 2).min(rh / 2) as i32) * SDF_S;
    let x = (px as i32) * SDF_S + SDF_S / 2;
    let y = (py as i32) * SDF_S + SDF_S / 2;
    let x0 = (rx as i32) * SDF_S;
    let y0 = (ry as i32) * SDF_S;
    let w = (rw as i32) * SDF_S;
    let h = (rh as i32) * SDF_S;
    let cx = x0 + w / 2;
    let cy = y0 + h / 2;
    let hx = w / 2;
    let hy = h / 2;
    let dx = (x - cx).abs();
    let dy = (y - cy).abs();
    let qx = dx - hx + r;
    let qy = dy - hy + r;
    let ox = qx.max(0);
    let oy = qy.max(0);
    let outer = isqrt(ox.saturating_mul(ox).saturating_add(oy.saturating_mul(oy)));
    let inner = qx.max(qy).min(0);
    outer + inner - r
}

fn isqrt(n: i32) -> i32 {
    (n.max(0) as u32).isqrt() as i32
}

/// Signed distance in 1/8 px; negative inside the rounded rect.
pub fn coverage_round(px: u32, py: u32, rx: u32, ry: u32, rw: u32, rh: u32, radius: u32) -> u8 {
    aa_from_d8(sdf8(px, py, rx, ry, rw, rh, radius))
}

fn aa_from_d8(d8: i32) -> u8 {
    let t = 8 - d8;
    if t >= 16 {
        255
    } else if t <= 0 {
        0
    } else {
        ((t * 255) / 16) as u8
    }
}

pub fn in_round(x: u32, y: u32, rx: u32, ry: u32, rw: u32, rh: u32, radius: u32) -> bool {
    coverage_round(x, y, rx, ry, rw, rh, radius) >= 128
}

/// Outside distance in whole pixels (0 inside). Same shape as `in_round`.
pub fn round_sdf(px: u32, py: u32, rx: u32, ry: u32, rw: u32, rh: u32, radius: u32) -> u32 {
    let d8 = sdf8(px, py, rx, ry, rw, rh, radius);
    if d8 <= 0 {
        0
    } else {
        ((d8 as u32) + (SDF_S as u32) - 1) / (SDF_S as u32)
    }
}

pub fn fill_round(t: Target, x: u32, y: u32, w: u32, h: u32, radius: u32, color: u32, clip: Clip) {
    let x1 = x.saturating_add(w).min(clip.x1).min(t.w);
    let y1 = y.saturating_add(h).min(clip.y1).min(t.h);
    let x0 = x.max(clip.x0);
    let y0 = y.max(clip.y0);
    if x0 >= x1 || y0 >= y1 {
        return;
    }
    let r = radius.min(w / 2).min(h / 2);
    for py in y0..y1 {
        fill_round_row(t, py, x, y, w, h, r, x0, x1, color, 255);
    }
}

fn fill_round_row(
    t: Target,
    py: u32,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    r: u32,
    x0: u32,
    x1: u32,
    color: u32,
    a: u8,
) {
    let ly = py.saturating_sub(y);
    if ly >= r && ly + r < h {
        fill_span_blend(t, x0, py, x1.saturating_sub(x0), color, a);
        return;
    }
    let mid_l = x.saturating_add(r).max(x0);
    let mid_r = x.saturating_add(w.saturating_sub(r)).min(x1);
    if mid_l < mid_r {
        fill_span_blend(t, mid_l, py, mid_r.saturating_sub(mid_l), color, a);
        plot_round_band(t, x0, mid_l, py, x, y, w, h, r, color, a);
        plot_round_band(t, mid_r, x1, py, x, y, w, h, r, color, a);
    } else {
        plot_round_band(t, x0, x1, py, x, y, w, h, r, color, a);
    }
}

fn plot_round_band(
    t: Target,
    x0: u32,
    x1: u32,
    py: u32,
    rx: u32,
    ry: u32,
    rw: u32,
    rh: u32,
    radius: u32,
    color: u32,
    a: u8,
) {
    for px in x0..x1 {
        let cov = coverage_round(px, py, rx, ry, rw, rh, radius);
        if cov == 0 {
            continue;
        }
        let aa = if a == 255 {
            cov
        } else {
            ((u32::from(a) * u32::from(cov)) / 255) as u8
        };
        if aa == 0 {
            continue;
        }
        if aa == 255 {
            put(t, px, py, color);
        } else {
            put(t, px, py, blend(get(t, px, py), color, aa));
        }
    }
}

pub fn fill_round_blend(
    t: Target,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    radius: u32,
    color: u32,
    a: u8,
    clip: Clip,
) {
    if a == 255 {
        fill_round(t, x, y, w, h, radius, color, clip);
        return;
    }
    let x1 = x.saturating_add(w).min(clip.x1).min(t.w);
    let y1 = y.saturating_add(h).min(clip.y1).min(t.h);
    let x0 = x.max(clip.x0);
    let y0 = y.max(clip.y0);
    if x0 >= x1 || y0 >= y1 {
        return;
    }
    let r = radius.min(w / 2).min(h / 2);
    for py in y0..y1 {
        fill_round_row(t, py, x, y, w, h, r, x0, x1, color, a);
    }
}

pub fn hover_fill(t: Target, x: u32, y: u32, w: u32, h: u32, clip: Clip) {
    fill_round(t, x, y, w, h, RADIUS_SM, HOVER, clip);
}

pub fn highlight_fill(t: Target, x: u32, y: u32, w: u32, h: u32, clip: Clip) {
    fill_round(t, x, y, w, h, RADIUS_SM, HIGHLIGHT, clip);
}

pub fn danger_fill(t: Target, x: u32, y: u32, w: u32, h: u32, clip: Clip) {
    fill_round(t, x, y, w, h, RADIUS_SM, DANGER, clip);
}

pub fn vsep(t: Target, x: u32, y: u32, h: u32, clip: Clip) {
    if x < clip.x0 || x >= clip.x1 {
        return;
    }
    let y0 = y.max(clip.y0);
    let y1 = y.saturating_add(h).min(clip.y1).min(t.h);
    for py in y0..y1 {
        put(t, x, py, blend(get(t, x, py), DIM, 160));
    }
}

pub fn caret(t: Target, x: u32, y: u32, h: u32, clip: Clip) {
    if x < clip.x0 || x >= clip.x1 {
        return;
    }
    let y0 = y.max(clip.y0);
    let y1 = y.saturating_add(h).min(clip.y1).min(t.h);
    for py in y0..y1 {
        put(t, x, py, TEXT);
    }
}

pub fn scrim(t: Target, x: u32, y: u32, w: u32, h: u32, clip: Clip) {
    let x0 = x.max(clip.x0);
    let y0 = y.max(clip.y0);
    let x1 = x.saturating_add(w).min(clip.x1).min(t.w);
    let y1 = y.saturating_add(h).min(clip.y1).min(t.h);
    if x0 >= x1 || y0 >= y1 {
        return;
    }
    for py in y0..y1 {
        fill_span_blend(t, x0, py, x1.saturating_sub(x0), OVERLAY, OVERLAY_ALPHA);
    }
}

pub fn text_elide(t: Target, x: u32, y: u32, s: &str, color: u32, xmax: u32, clip: Clip) {
    let avail = xmax.saturating_sub(x);
    let max_chars = avail / FONT_W;
    let n = s.len() as u32;
    if n <= max_chars {
        text(t, x, y, s, color, xmax, clip);
        return;
    }
    if max_chars < 4 {
        text(t, x, y, s, color, xmax, clip);
        return;
    }
    let keep = (max_chars as usize).saturating_sub(3).min(s.len());
    let mut buf = [0u8; 96];
    let nkeep = keep.min(buf.len().saturating_sub(3));
    buf[..nkeep].copy_from_slice(&s.as_bytes()[..nkeep]);
    buf[nkeep] = b'.';
    buf[nkeep + 1] = b'.';
    buf[nkeep + 2] = b'.';
    let out = core::str::from_utf8(&buf[..nkeep + 3]).unwrap_or("...");
    text(t, x, y, out, color, xmax, clip);
}

pub fn query_field(
    t: Target,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    text_s: &str,
    placeholder: &str,
    show_caret: bool,
    search_icon: bool,
    clip: Clip,
) {
    fill_round(t, x, y, w, h, RADIUS_SM, BG, clip);
    let mut tx = x.saturating_add(PAD / 2);
    if search_icon {
        icon(
            t,
            Icon::Search,
            tx,
            y + (h.saturating_sub(ICON)) / 2,
            DIM,
            clip,
        );
        tx = tx.saturating_add(ICON);
    }
    let ty = y.saturating_add(h.saturating_sub(FONT_H) / 2);
    let xmax = x.saturating_add(w).saturating_sub(PAD / 2);
    if text_s.is_empty() {
        text(t, tx, ty, placeholder, DIM, xmax, clip);
        if show_caret {
            caret(t, tx, ty, FONT_H, clip);
        }
        return;
    }
    text_elide(t, tx, ty, text_s, TEXT, xmax, clip);
    if show_caret {
        let cx = tx
            .saturating_add((text_s.len() as u32).saturating_mul(FONT_W))
            .min(xmax.saturating_sub(1));
        caret(t, cx, ty, FONT_H, clip);
    }
}

pub fn list_row(
    t: Target,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    label: &str,
    icon_which: Option<Icon>,
    selected: bool,
    hovered: bool,
    clip: Clip,
) {
    if selected {
        highlight_fill(t, x, y, w, h, clip);
    } else if hovered {
        hover_fill(t, x, y, w, h, clip);
    }
    let mut tx = x.saturating_add(PAD / 2);
    let iy = y + (h.saturating_sub(ICON)) / 2;
    if let Some(ic) = icon_which {
        icon(t, ic, tx, iy, TEXT, clip);
        tx = tx.saturating_add(ICON + PAD / 2);
    }
    let ty = y.saturating_add(h.saturating_sub(FONT_H) / 2);
    text_elide(t, tx, ty, label, TEXT, x.saturating_add(w), clip);
}

pub fn blend(dst: u32, src: u32, a: u8) -> u32 {
    let aa = u32::from(a);
    let ia = 255 - aa;
    let db = dst & 0xff;
    let dg = (dst >> 8) & 0xff;
    let dr = (dst >> 16) & 0xff;
    let sb = src & 0xff;
    let sg = (src >> 8) & 0xff;
    let sr = (src >> 16) & 0xff;
    ((sb * aa + db * ia) / 255)
        | (((sg * aa + dg * ia) / 255) << 8)
        | (((sr * aa + dr * ia) / 255) << 16)
}

pub fn shade_px(t: Target, x: u32, y: u32, a: u8) {
    put(t, x, y, blend(get(t, x, y), SHADOW, a));
}

pub fn text(t: Target, x: u32, y: u32, s: &str, color: u32, xmax: u32, clip: Clip) {
    text_cov(t, x, y, s, color, xmax, clip, false);
}

pub fn text_bold(t: Target, x: u32, y: u32, s: &str, color: u32, xmax: u32, clip: Clip) {
    text_cov(t, x, y, s, color, xmax, clip, true);
}

fn text_cov(t: Target, x: u32, y: u32, s: &str, color: u32, xmax: u32, clip: Clip, bold: bool) {
    let mut cx = x;
    for c in s.bytes() {
        if cx.saturating_add(FONT_W) > xmax {
            break;
        }
        for gy in 0..FONT_H {
            for gx in 0..FONT_W {
                let mut cov = font::coverage(c, gx, gy);
                if bold {
                    cov = cov.saturating_add(cov / 2);
                }
                if cov == 0 {
                    continue;
                }
                let px = cx.saturating_add(gx);
                let py = y.saturating_add(gy);
                if !clip.contains(px, py) {
                    continue;
                }
                if cov == 255 {
                    put(t, px, py, color);
                } else {
                    put(t, px, py, blend(get(t, px, py), color, cov));
                }
            }
        }
        cx = cx.saturating_add(FONT_W);
    }
}

pub fn tooltip_size(s: &str, with_close: bool) -> (u32, u32) {
    let extra = if with_close { 18 } else { 0 };
    let w = (s.len() as u32)
        .saturating_mul(FONT_W)
        .saturating_add(PAD)
        .saturating_add(PAD / 2)
        .saturating_add(extra);
    (w.max(24), FONT_H.saturating_add(PAD))
}

pub fn tooltip(t: Target, x: u32, y: u32, s: &str, with_close: bool, clip: Clip) -> Clip {
    let (w, h) = tooltip_size(s, with_close);
    fill_round(t, x, y, w, h, RADIUS_SM, SURFACE, clip);
    text(
        t,
        x.saturating_add(PAD / 2 + 2),
        y.saturating_add(PAD / 2),
        s,
        TEXT,
        x.saturating_add(w),
        clip,
    );
    if with_close {
        let cx = x.saturating_add(w).saturating_sub(14);
        let cy = y.saturating_add(PAD / 2);
        icon(t, Icon::Close, cx, cy, coeleo_theme::DANGER, clip);
        return Clip {
            x0: cx,
            y0: cy,
            x1: cx.saturating_add(ICON),
            y1: cy.saturating_add(ICON),
        };
    }
    Clip {
        x0: 0,
        y0: 0,
        x1: 0,
        y1: 0,
    }
}

pub fn icon_blit(t: Target, x: u32, y: u32, pix: &[u32], iw: u32, ih: u32, clip: Clip) {
    for row in 0..ih {
        for col in 0..iw {
            let px = x.saturating_add(col);
            let py = y.saturating_add(row);
            if !clip.contains(px, py) {
                continue;
            }
            let c = pix[(row * iw + col) as usize];
            let a = ((c >> 24) & 0xff) as u8;
            if a == 0 {
                continue;
            }
            let rgb = c & 0x00FF_FFFF;
            if a == 255 {
                put(t, px, py, rgb);
            } else {
                put(t, px, py, blend(get(t, px, py), rgb, a));
            }
        }
    }
}

pub fn icon(t: Target, which: Icon, x: u32, y: u32, color: u32, clip: Clip) {
    match which {
        Icon::Folder => icon_folder(t, x, y, color, clip),
        Icon::Terminal => icon_term(t, x, y, color, clip),
        Icon::Search => icon_search(t, x, y, color, clip),
        Icon::App => icon_app(t, x, y, color, clip),
        Icon::Close => icon_close(t, x, y, color, clip),
        Icon::Back => icon_chev(t, x, y, color, clip, true),
        Icon::Forward => icon_chev(t, x, y, color, clip, false),
    }
}

fn plot(t: Target, x: u32, y: u32, color: u32, cov: u8, clip: Clip) {
    if !clip.contains(x, y) || cov == 0 {
        return;
    }
    if cov == 255 {
        put(t, x, y, color);
    } else {
        put(t, x, y, blend(get(t, x, y), color, cov));
    }
}

fn icon_folder(t: Target, x: u32, y: u32, c: u32, clip: Clip) {
    fill_round(t, x + 2, y + 4, 12, 9, 2, c, clip);
    fill_round(t, x + 2, y + 3, 5, 3, 1, c, clip);
}

fn icon_term(t: Target, x: u32, y: u32, c: u32, clip: Clip) {
    fill_round(t, x + 2, y + 3, 12, 10, 2, c, clip);
    for i in 0..10u32 {
        for j in 0..8u32 {
            plot(t, x + 3 + i, y + 4 + j, SURFACE, 255, clip);
        }
    }
    plot(t, x + 5, y + 7, c, 200, clip);
    plot(t, x + 6, y + 8, c, 200, clip);
    plot(t, x + 5, y + 9, c, 200, clip);
    for col in 8..11u32 {
        plot(t, x + col, y + 10, c, 200, clip);
    }
}

fn icon_search(t: Target, x: u32, y: u32, c: u32, clip: Clip) {
    for row in 2..12u32 {
        for col in 2..12u32 {
            let dx = col as i32 - 6;
            let dy = row as i32 - 6;
            let d2 = dx * dx + dy * dy;
            let cov = ring_cov(d2, 9, 16);
            plot(t, x + col, y + row, c, cov, clip);
        }
    }
    for i in 0..4u32 {
        plot(t, x + 10 + i, y + 10 + i, c, 200, clip);
        plot(t, x + 11 + i, y + 10 + i, c, 120, clip);
    }
}

fn ring_cov(d2: i32, inner: i32, outer: i32) -> u8 {
    if d2 < inner - 4 || d2 > outer + 4 {
        return 0;
    }
    if d2 >= inner && d2 <= outer {
        return 255;
    }
    let t = if d2 < inner { inner - d2 } else { d2 - outer };
    (255 - t * 50).clamp(0, 255) as u8
}

fn icon_app(t: Target, x: u32, y: u32, c: u32, clip: Clip) {
    fill_round(t, x + 2, y + 2, 12, 12, 3, c, clip);
}

fn icon_close(t: Target, x: u32, y: u32, c: u32, clip: Clip) {
    for i in 0..10u32 {
        plot(t, x + 3 + i, y + 3 + i, c, 220, clip);
        plot(t, x + 4 + i, y + 3 + i, c, 140, clip);
        plot(t, x + 12 - i, y + 3 + i, c, 220, clip);
        plot(t, x + 11 - i, y + 3 + i, c, 140, clip);
    }
}

fn icon_chev(t: Target, x: u32, y: u32, c: u32, clip: Clip, left: bool) {
    for i in 0..6u32 {
        let ox = if left { 9 - i / 2 } else { 6 + i / 2 };
        plot(t, x + ox, y + 5 + i, c, 220, clip);
        plot(t, x + ox + 1, y + 5 + i, c, 180, clip);
    }
}

pub fn btn_shadow(t: Target, x: u32, y: u32, w: u32, h: u32, clip: Clip) {
    let a = 40u8;
    for i in 0..w {
        let px = x.saturating_add(i);
        let py = y.saturating_add(h);
        if clip.contains(px, py) {
            shade_px(t, px, py, a);
        }
    }
    for i in 0..h {
        let px = x.saturating_add(w);
        let py = y.saturating_add(i);
        if clip.contains(px, py) {
            shade_px(t, px, py, a);
        }
    }
}

pub fn dim() -> u32 {
    DIM
}
