//! Floating Plasma strut: menu, KRunner, pinned tasks, clock.

use core::sync::atomic::{AtomicU8, Ordering};

use crate::fbterm::FbInfo;
use crate::serial;
use coeleo_draw::{self, Clip, Icon, Target};
use coeleo_theme::{
    ACCENT, BG, BORDER, DIM, PAD, PANEL_ALPHA, PANEL_BG, PANEL_H, PANEL_INSET, PANEL_MARGIN,
    RADIUS, RADIUS_SM, TASK_ICON, TEXT,
};

pub const SLOT: u32 = TASK_ICON;
const TIP_GAP: u32 = PAD / 2;
const MODE_FLOAT: u8 = 0;
const MODE_FULL: u8 = 1;
static MODE: AtomicU8 = AtomicU8::new(MODE_FLOAT);

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Float,
    Full,
}

pub fn mode() -> Mode {
    if MODE.load(Ordering::Acquire) == MODE_FULL {
        Mode::Full
    } else {
        Mode::Float
    }
}

/// Returns true if the mode changed.
pub fn set_mode(m: Mode) -> bool {
    let v = match m {
        Mode::Float => MODE_FLOAT,
        Mode::Full => MODE_FULL,
    };
    if MODE.swap(v, Ordering::AcqRel) == v {
        return false;
    }
    match m {
        Mode::Float => serial::write_str("panel: float\n"),
        Mode::Full => serial::write_str("panel: full\n"),
    }
    true
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    None,
    Launcher,
    KRunner,
    Task(usize),
    TaskClose(usize),
    Clock,
    Net,
}

#[derive(Clone, Copy)]
pub struct TaskInfo {
    pub title: &'static str,
    pub focused: bool,
    pub minimized: bool,
    pub icon: Icon,
}

#[derive(Clone, Copy)]
pub struct Tip {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl Tip {
    pub fn empty() -> Self {
        Self {
            x: 0,
            y: 0,
            w: 0,
            h: 0,
        }
    }

    pub fn is_empty(self) -> bool {
        self.w == 0 || self.h == 0
    }
}

pub fn height() -> u32 {
    PANEL_H
}

pub fn work_h(fb_h: u32) -> u32 {
    match mode() {
        Mode::Float => fb_h.saturating_sub(PANEL_H.saturating_add(PANEL_MARGIN)),
        Mode::Full => fb_h.saturating_sub(PANEL_H),
    }
}

pub fn bar_y(fb_h: u32) -> u32 {
    work_h(fb_h)
}

fn bar_x0() -> u32 {
    match mode() {
        Mode::Float => PANEL_MARGIN,
        Mode::Full => 0,
    }
}

fn bar_x1(fb_w: u32) -> u32 {
    match mode() {
        Mode::Float => fb_w.saturating_sub(PANEL_MARGIN),
        Mode::Full => fb_w,
    }
}

pub fn slot_x0() -> u32 {
    bar_x0() + PANEL_INSET
}

fn slot_y0(bar_y0: u32) -> u32 {
    bar_y0.saturating_add(PANEL_INSET)
}

fn slot_h() -> u32 {
    PANEL_H.saturating_sub(PANEL_INSET.saturating_mul(2))
}

fn tray_geom(x1: u32, clock_w: u32) -> (u32, u32, u32) {
    let clock_x0 = x1.saturating_sub(PAD + clock_w);
    let net_x = clock_x0.saturating_sub(coeleo_draw::ICON + PAD);
    let sep_x = net_x.saturating_sub(PAD);
    (sep_x, net_x, clock_x0)
}

pub fn paint(
    fb: FbInfo,
    clock_mmss: &str,
    tasks: &[TaskInfo],
    hover: Hit,
    pressed: Hit,
    opaque: bool,
    wall: Option<(&[u32], u32, u32)>,
    menu_icon: Option<(&[u32], u32, u32)>,
) -> Tip {
    let t = tgt(fb);
    let clip = Clip::all(fb.w, fb.h);
    let y0 = bar_y(fb.h);
    let x0 = bar_x0();
    let x1 = bar_x1(fb.w);
    let bw = x1.saturating_sub(x0);
    blit_band(t, y0, fb.h, wall);
    fill_bar(t, x0, y0, bw, opaque, clip);
    let iy0 = slot_y0(y0);
    let sh = slot_h();
    let ty = iy0 + (sh.saturating_sub(coeleo_draw::FONT_H)) / 2;
    let ioff = (SLOT.saturating_sub(coeleo_draw::ICON)) / 2;
    let iy = iy0 + (sh.saturating_sub(coeleo_draw::ICON)) / 2;
    let sx0 = slot_x0();
    slot_bg(t, sx0, iy0, Hit::Launcher, hover, pressed, clip);
    if let Some((pix, iw, ih)) = menu_icon {
        let ix = sx0 + (SLOT.saturating_sub(iw)) / 2;
        let iy = iy0 + (sh.saturating_sub(ih)) / 2;
        coeleo_draw::icon_blit(t, ix, iy, pix, iw, ih, clip);
    } else {
        coeleo_draw::icon(t, Icon::App, sx0 + ioff, iy, ACCENT, clip);
    }
    let kx = sx0 + SLOT;
    slot_bg(t, kx, iy0, Hit::KRunner, hover, pressed, clip);
    coeleo_draw::icon(t, Icon::Search, kx + ioff, iy, TEXT, clip);
    coeleo_draw::vsep(t, sx0 + SLOT * 2, iy0 + 2, sh.saturating_sub(4), clip);
    let clock_w = clock_w(clock_mmss);
    let (sep_x, net_x, clock_x0) = tray_geom(x1, clock_w);
    let mut sx = sx0 + SLOT * 2;
    let overflow = coeleo_draw::FONT_W;
    let mut hidden = false;
    for (i, task) in tasks.iter().enumerate() {
        if sx.saturating_add(SLOT) + overflow > sep_x {
            hidden = i < tasks.len();
            break;
        }
        let hit = Hit::Task(i);
        if task.focused {
            coeleo_draw::fill_round(t, sx, iy0, SLOT, sh, RADIUS_SM, 0x0022_2A38, clip);
        }
        slot_bg(t, sx, iy0, hit, hover, pressed, clip);
        let icon_color = if task.minimized { DIM } else { TEXT };
        coeleo_draw::icon(t, task.icon, sx + ioff, iy, icon_color, clip);
        paint_pill(t, sx, y0, task, clip);
        sx = sx.saturating_add(SLOT);
    }
    if hidden {
        coeleo_draw::text(t, sx, ty, ">", DIM, sep_x, clip);
    }
    coeleo_draw::vsep(t, sep_x, iy0 + 2, sh.saturating_sub(4), clip);
    if hover == Hit::Net {
        coeleo_draw::hover_fill(t, net_x.saturating_sub(2), iy0, coeleo_draw::ICON + 4, sh, clip);
    }
    let net_color = if crate::net::is_up() {
        ACCENT
    } else {
        DIM
    };
    coeleo_draw::icon(t, Icon::Network, net_x, iy, net_color, clip);
    coeleo_draw::text(t, clock_x0, ty, clock_mmss, TEXT, x1, clip);
    paint_tip(t, fb.w, fb.h, tasks, hover, clip)
}

/// Redraw only the clock strip (wallpaper + panel slice + text).
pub fn paint_clock(fb: FbInfo, clock_mmss: &str, opaque: bool, wall: Option<(&[u32], u32, u32)>) {
    let t = tgt(fb);
    let y0 = bar_y(fb.h);
    let x1 = bar_x1(fb.w);
    let tw = clock_w(clock_mmss);
    let (_, _, clock_x0) = tray_geom(x1, tw);
    let clip = Clip {
        x0: clock_x0.saturating_sub(2),
        y0,
        x1,
        y1: y0.saturating_add(PANEL_H).min(fb.h),
    };
    blit_band_x(
        t,
        y0,
        y0.saturating_add(PANEL_H).min(fb.h),
        clip.x0,
        x1,
        wall,
    );
    let x0 = bar_x0();
    let bw = x1.saturating_sub(x0);
    fill_bar(t, x0, y0, bw, opaque, clip);
    let ty = slot_y0(y0) + (slot_h().saturating_sub(coeleo_draw::FONT_H)) / 2;
    coeleo_draw::text(t, clock_x0, ty, clock_mmss, TEXT, x1, clip);
}

pub fn clock_rect(fb_w: u32, fb_h: u32, clock_mmss: &str) -> (u32, u32, u32, u32) {
    let y0 = bar_y(fb_h);
    let x1 = bar_x1(fb_w);
    let tw = clock_w(clock_mmss);
    let (_, _, clock_x0) = tray_geom(x1, tw);
    (
        clock_x0.saturating_sub(2),
        y0,
        x1.saturating_sub(clock_x0.saturating_sub(2)),
        PANEL_H,
    )
}

pub fn hit(x: u32, y: u32, fb_w: u32, fb_h: u32, tasks: &[TaskInfo], clock_mmss: &str) -> Hit {
    if let Some(i) = tip_task(tasks, fb_w, fb_h, clock_mmss, x, y) {
        let with_close = !tasks[i].minimized;
        if with_close {
            let (tx, ty, _w, _h, close) = tip_geom(fb_w, fb_h, i, tasks[i].title, true);
            let _ = (tx, ty);
            if x >= close.x0 && x < close.x1 && y >= close.y0 && y < close.y1 {
                return Hit::TaskClose(i);
            }
        }
        return Hit::Task(i);
    }
    let y0 = bar_y(fb_h);
    let x0 = slot_x0();
    let x1 = bar_x1(fb_w);
    if y < y0 || y >= y0.saturating_add(PANEL_H) || x < bar_x0() || x >= x1 {
        return Hit::None;
    }
    if x >= x0 && x < x0 + SLOT {
        return Hit::Launcher;
    }
    if x >= x0 + SLOT && x < x0 + SLOT * 2 {
        return Hit::KRunner;
    }
    let tw = clock_w(clock_mmss);
    let (sep_x, net_x, clock_x0) = tray_geom(x1, tw);
    if x >= clock_x0 {
        return Hit::Clock;
    }
    if x >= net_x && x < clock_x0 {
        return Hit::Net;
    }
    let tasks_x0 = x0 + SLOT * 2;
    let rel = x.saturating_sub(tasks_x0);
    let i = (rel / SLOT) as usize;
    let max_fit = (sep_x.saturating_sub(tasks_x0) / SLOT) as usize;
    if x >= tasks_x0 && i < tasks.len() && i < max_fit {
        Hit::Task(i)
    } else {
        Hit::None
    }
}

fn tip_task(
    tasks: &[TaskInfo],
    fb_w: u32,
    fb_h: u32,
    clock_mmss: &str,
    mx: u32,
    my: u32,
) -> Option<usize> {
    let y0 = bar_y(fb_h);
    let x0 = slot_x0();
    let x1 = bar_x1(fb_w);
    let tw = clock_w(clock_mmss);
    let (sep_x, _, _) = tray_geom(x1, tw);
    let tasks_x0 = x0 + SLOT * 2;
    let max_fit = (sep_x.saturating_sub(tasks_x0) / SLOT) as usize;
    let n = tasks.len().min(max_fit);
    for i in 0..n {
        let sx = tasks_x0 + i as u32 * SLOT;
        let in_slot = mx >= sx && mx < sx + SLOT && my >= y0 && my < y0 + PANEL_H;
        let with_close = !tasks[i].minimized;
        let (tx, ty, tw, th, _) = tip_geom(fb_w, fb_h, i, tasks[i].title, with_close);
        let in_tip =
            mx >= tx && mx < tx.saturating_add(tw) && my >= ty && my < ty.saturating_add(th);
        if in_slot || in_tip {
            return Some(i);
        }
    }
    None
}

fn paint_tip(t: Target, fb_w: u32, fb_h: u32, tasks: &[TaskInfo], hover: Hit, clip: Clip) -> Tip {
    match hover {
        Hit::Task(i) | Hit::TaskClose(i) => {
            let Some(task) = tasks.get(i) else {
                return Tip::empty();
            };
            let with_close = !task.minimized;
            let (x, y, w, h, _) = tip_geom(fb_w, fb_h, i, task.title, with_close);
            coeleo_draw::tooltip(t, x, y, task.title, with_close, clip);
            Tip { x, y, w, h }
        }
        Hit::Launcher => tip_at(t, fb_w, fb_h, slot_x0(), "Applications", clip),
        Hit::KRunner => tip_at(t, fb_w, fb_h, slot_x0() + SLOT, "Search", clip),
        Hit::Net => {
            let msg = if crate::net::is_up() {
                "Network: Connected"
            } else if crate::net::has_nic() {
                "Network: Disconnected"
            } else {
                "Network: No Interface"
            };
            let (_, net_x, _) = tray_geom(bar_x1(fb_w), clock_w("??:??"));
            tip_at(t, fb_w, fb_h, net_x, msg, clip)
        }
        Hit::Clock => {
            let mut date = [0u8; 16];
            let Some(s) = crate::rtc::format_ymd(&mut date) else {
                return Tip::empty();
            };
            let (w, h) = coeleo_draw::tooltip_size(s, false);
            let y0 = bar_y(fb_h);
            let x1 = bar_x1(fb_w);
            let mut x = x1.saturating_sub(w);
            if x < bar_x0() {
                x = bar_x0();
            }
            let y = y0.saturating_sub(h.saturating_add(TIP_GAP));
            coeleo_draw::tooltip(t, x, y, s, false, clip);
            Tip { x, y, w, h }
        }
        _ => Tip::empty(),
    }
}

fn tip_geom(
    fb_w: u32,
    fb_h: u32,
    i: usize,
    title: &str,
    with_close: bool,
) -> (u32, u32, u32, u32, Clip) {
    let (w, h) = coeleo_draw::tooltip_size(title, with_close);
    let y0 = bar_y(fb_h);
    let x0 = slot_x0() + SLOT * 2 + i as u32 * SLOT;
    let mut x = x0;
    if x.saturating_add(w) > bar_x1(fb_w) {
        x = bar_x1(fb_w).saturating_sub(w);
    }
    let y = y0.saturating_sub(h.saturating_add(TIP_GAP));
    let close = if with_close {
        let cx = x.saturating_add(w).saturating_sub(coeleo_draw::ICON + 2);
        let cy = y + (h.saturating_sub(coeleo_draw::ICON)) / 2;
        Clip {
            x0: cx,
            y0: cy,
            x1: cx.saturating_add(coeleo_draw::ICON),
            y1: cy.saturating_add(coeleo_draw::ICON),
        }
    } else {
        Clip {
            x0: 0,
            y0: 0,
            x1: 0,
            y1: 0,
        }
    };
    (x, y, w, h, close)
}

fn tip_at(t: Target, fb_w: u32, fb_h: u32, x0: u32, s: &str, clip: Clip) -> Tip {
    let (w, h) = coeleo_draw::tooltip_size(s, false);
    let y0 = bar_y(fb_h);
    let mut x = x0;
    if x.saturating_add(w) > bar_x1(fb_w) {
        x = bar_x1(fb_w).saturating_sub(w);
    }
    let y = y0.saturating_sub(h.saturating_add(TIP_GAP));
    coeleo_draw::tooltip(t, x, y, s, false, clip);
    Tip { x, y, w, h }
}

fn fill_bar(t: Target, x0: u32, y0: u32, bw: u32, opaque: bool, clip: Clip) {
    match mode() {
        Mode::Float => {
            if opaque {
                coeleo_draw::fill_round(t, x0, y0, bw, PANEL_H, RADIUS, PANEL_BG, clip);
            } else {
                coeleo_draw::fill_round_blend(
                    t,
                    x0,
                    y0,
                    bw,
                    PANEL_H,
                    RADIUS,
                    PANEL_BG,
                    PANEL_ALPHA,
                    clip,
                );
            }
            coeleo_draw::stroke_round(t, x0, y0, bw, PANEL_H, RADIUS, BORDER, clip);
        }
        Mode::Full => {
            let y1 = y0.saturating_add(PANEL_H);
            let x1 = x0.saturating_add(bw);
            let ys = y0.max(clip.y0);
            let ye = y1.min(clip.y1).min(t.h);
            let xs = x0.max(clip.x0);
            let xe = x1.min(clip.x1);
            if xs >= xe {
                return;
            }
            let w = xe.saturating_sub(xs);
            for y in ys..ye {
                if opaque {
                    coeleo_draw::fill_span(t, xs, y, w, PANEL_BG);
                } else {
                    coeleo_draw::fill_span_blend(t, xs, y, w, PANEL_BG, PANEL_ALPHA);
                }
            }
            if y0 >= clip.y0 && y0 < clip.y1 && y0 < t.h {
                coeleo_draw::fill_span(t, xs, y0, w, BORDER);
            }
        }
    }
}

fn slot_bg(t: Target, x: u32, y: u32, which: Hit, hover: Hit, pressed: Hit, clip: Clip) {
    if pressed == which
        || hover == which
        || matches!((hover, which), (Hit::TaskClose(a), Hit::Task(b)) if a == b)
    {
        coeleo_draw::hover_fill(t, x, y, SLOT, slot_h(), clip);
    }
}

fn paint_pill(t: Target, sx: u32, y0: u32, task: &TaskInfo, clip: Clip) {
    if task.focused {
        let w = 16u32;
        let x = sx.saturating_add((SLOT.saturating_sub(w)) / 2);
        let y = y0.saturating_add(PANEL_H.saturating_sub(PANEL_INSET + 3));
        coeleo_draw::fill_round(t, x, y, w, 3, 1, ACCENT, clip);
    } else if !task.minimized {
        let w = 4u32;
        let x = sx.saturating_add((SLOT.saturating_sub(w)) / 2);
        let y = y0.saturating_add(PANEL_H.saturating_sub(PANEL_INSET + 4));
        coeleo_draw::fill_round(t, x, y, w, 4, 2, DIM, clip);
    }
}

fn clock_w(clock_mmss: &str) -> u32 {
    coeleo_draw::text_width(clock_mmss)
}

fn wall_px(pix: &[u32], ww: u32, wh: u32, x: u32, y: u32, fb_w: u32) -> u32 {
    if ww == 0 || wh == 0 || pix.is_empty() {
        return BG;
    }
    let sx = if ww == fb_w {
        x.min(ww.saturating_sub(1))
    } else {
        x.saturating_mul(ww) / fb_w.max(1)
    };
    let sy = y.min(wh.saturating_sub(1));
    let src = (sy * ww + sx) as usize;
    if src < pix.len() {
        pix[src] & 0x00FF_FFFF
    } else {
        BG
    }
}

fn blit_band(t: Target, y0: u32, y1: u32, wall: Option<(&[u32], u32, u32)>) {
    blit_band_x(t, y0, y1, 0, t.w, wall);
}

fn blit_band_x(t: Target, y0: u32, y1: u32, x0: u32, x1: u32, wall: Option<(&[u32], u32, u32)>) {
    let x0 = x0.min(t.w);
    let x1 = x1.min(t.w);
    if x0 >= x1 {
        return;
    }
    for y in y0..y1.min(t.h) {
        if let Some((pix, ww, wh)) = wall {
            if ww != 0 && wh != 0 && ww == t.w {
                let sy = y.min(wh.saturating_sub(1));
                let off = ((y * t.pitch) / 4 + x0) as usize;
                let src = (sy * ww + x0) as usize;
                let n = (x1 - x0) as usize;
                if src + n <= pix.len() {
                    unsafe {
                        core::ptr::copy_nonoverlapping(
                            pix.as_ptr().add(src),
                            (t.addr as *mut u32).add(off),
                            n,
                        );
                    }
                    continue;
                }
            }
            if ww != 0 && wh != 0 {
                let off = ((y * t.pitch) / 4 + x0) as usize;
                unsafe {
                    let p = t.addr as *mut u32;
                    for i in 0..(x1 - x0) as usize {
                        *p.add(off + i) = wall_px(pix, ww, wh, x0 + i as u32, y, t.w);
                    }
                }
                continue;
            }
        }
        coeleo_draw::fill_span(t, x0, y, x1.saturating_sub(x0), BG);
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
