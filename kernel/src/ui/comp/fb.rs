//! Scene-buffer pixel writes.

use crate::fbterm::FbInfo;
use coeleo_draw::Target;
use coeleo_theme::{RADIUS, SHADOW, SHADOW_A, SHADOW_PX};

use super::rect::Rect;
use super::state::State;

pub(super) fn shade_px(st: &mut State, x: u32, y: u32, x0: u32, y0: u32, x1: u32, y1: u32) {
    let w = x1.saturating_sub(x0);
    let h = y1.saturating_sub(y0);
    let d = coeleo_draw::round_sdf(x, y, x0, y0, w, h, RADIUS);
    if d == 0 || d > SHADOW_PX {
        return;
    }
    let a = SHADOW_A[(d - 1) as usize];
    let dst = get_fb(&st.scene_fb, x as i32, y as i32);
    put_fb(&st.scene_fb, x, y, blend(dst, SHADOW, a));
}

pub(super) fn blend(dst: [u8; 4], src: u32, a: u8) -> [u8; 4] {
    let aa = u32::from(a);
    let ia = 255 - aa;
    let sb = src & 0xff;
    let sg = (src >> 8) & 0xff;
    let sr = (src >> 16) & 0xff;
    let db = dst[0] as u32;
    let dg = dst[1] as u32;
    let dr = dst[2] as u32;
    [
        ((sb * aa + db * ia) / 255) as u8,
        ((sg * aa + dg * ia) / 255) as u8,
        ((sr * aa + dr * ia) / 255) as u8,
        dst[3],
    ]
}
pub(super) fn tgt(fb: FbInfo) -> Target {
    Target {
        addr: fb.addr,
        w: fb.w,
        h: fb.h,
        pitch: fb.pitch,
    }
}
pub(super) fn get_fb(fb: &FbInfo, x: i32, y: i32) -> [u8; 4] {
    if x < 0 || y < 0 || x >= fb.w as i32 || y >= fb.h as i32 {
        return [0, 0, 0, 0];
    }
    let off = (y as u32 * fb.pitch + x as u32 * 4) as usize;
    let mut px = [0u8; 4];
    unsafe {
        let p = fb.addr as *const u8;
        core::ptr::copy_nonoverlapping(p.add(off), px.as_mut_ptr(), 4);
    }
    px
}

pub(super) fn blit_u32_at_round(
    fb: &FbInfo,
    dx: u32,
    dy: u32,
    src: &[u32],
    stride: u32,
    sx: u32,
    sy: u32,
    w: u32,
    h: u32,
    wx: u32,
    wy: u32,
    ww: u32,
    wh: u32,
) {
    if w == 0 || h == 0 || dx >= fb.w || dy >= fb.h {
        return;
    }
    let w = w.min(fb.w.saturating_sub(dx));
    let h = h.min(fb.h.saturating_sub(dy));
    let wx1 = wx.saturating_add(ww);
    let wy1 = wy.saturating_add(wh);
    for row in 0..h {
        let y = dy.saturating_add(row);
        let y_edge = y < wy.saturating_add(RADIUS) || y + RADIUS >= wy1;
        let mut col = 0u32;
        while col < w {
            let x = dx.saturating_add(col);
            if !y_edge && x >= wx.saturating_add(RADIUS) && x + RADIUS < wx1 {
                let mid = (wx1.saturating_sub(RADIUS).saturating_sub(x)).min(w.saturating_sub(col));
                if mid > 0 {
                    blit_u32_at(
                        fb,
                        x,
                        y,
                        src,
                        stride,
                        sx.saturating_add(col),
                        sy.saturating_add(row),
                        mid,
                        1,
                    );
                    col = col.saturating_add(mid);
                    continue;
                }
            }
            let cov = coeleo_draw::coverage_round(x, y, wx, wy, ww, wh, RADIUS);
            if cov > 0 {
                let src_off = (sy.saturating_add(row).saturating_mul(stride)
                    + sx.saturating_add(col)) as usize;
                if src_off < src.len() {
                    let color = src[src_off] & 0x00FF_FFFF;
                    if cov == 255 {
                        put_fb(fb, x, y, color.to_le_bytes());
                    } else {
                        put_fb(fb, x, y, blend(get_fb(fb, x as i32, y as i32), color, cov));
                    }
                }
            }
            col = col.saturating_add(1);
        }
    }
}

pub(super) fn blit_u32_at(
    fb: &FbInfo,
    dx: u32,
    dy: u32,
    src: &[u32],
    stride: u32,
    sx: u32,
    sy: u32,
    w: u32,
    h: u32,
) {
    if w == 0 || h == 0 || dx >= fb.w || dy >= fb.h {
        return;
    }
    let w = w.min(fb.w.saturating_sub(dx));
    let h = h.min(fb.h.saturating_sub(dy));
    let bytes = (w as usize).saturating_mul(4);
    for row in 0..h {
        let src_off = (sy.saturating_add(row).saturating_mul(stride) + sx) as usize;
        let dst_off = (dy.saturating_add(row) * fb.pitch + dx * 4) as usize;
        if src_off.saturating_add(w as usize) > src.len() {
            break;
        }
        unsafe {
            core::ptr::copy_nonoverlapping(
                src.as_ptr().add(src_off).cast::<u8>(),
                (fb.addr as *mut u8).add(dst_off),
                bytes,
            );
        }
    }
}

pub(super) fn fill_span(fb: &FbInfo, x: u32, y: u32, w: u32, px: [u8; 4]) {
    if w == 0 || x >= fb.w || y >= fb.h {
        return;
    }
    let w = w.min(fb.w.saturating_sub(x));
    let color = u32::from_le_bytes(px);
    let off = ((y * fb.pitch) / 4 + x) as usize;
    unsafe {
        let p = fb.addr as *mut u32;
        for i in 0..w {
            *p.add(off + i as usize) = color;
        }
    }
}

pub(super) fn put_fb(fb: &FbInfo, x: u32, y: u32, px: [u8; 4]) {
    if x >= fb.w || y >= fb.h {
        return;
    }
    let off = (y * fb.pitch + x * 4) as usize;
    unsafe {
        let p = fb.addr as *mut u8;
        core::ptr::copy_nonoverlapping(px.as_ptr(), p.add(off), 4);
    }
}

pub(super) fn put_fb_i(fb: &FbInfo, x: i32, y: i32, px: [u8; 4]) {
    if x < 0 || y < 0 {
        return;
    }
    put_fb(fb, x as u32, y as u32, px);
}
