//! Z-order, focus, drag, min/max.

use crate::fbterm::FbInfo;
use crate::fm;
use crate::serial;
use coeleo_theme::{DECO_H, SHADOW_PX};

use super::geom::{any_maximized, frame_new, h_work, opaque_rect, shadow_rect, visible_top};
use super::log::log_focus;
use super::log::log_panel_mode;
use super::paint::{
    fill_rect_r, fill_revealed, fill_work, paint_all_frames, paint_strut, present_damage,
};
use super::rect::rect_union;
use super::state::{
    Drag, Edge, Focus, Frame, FrameKind, Hit, State, BTN, CLIENT_TOP, FOCUS_DESK, FOCUS_FILES,
};
use core::sync::atomic::Ordering;

pub(super) fn focus_frame(st: &mut State, i: usize) {
    match st.frames[i].kind {
        FrameKind::Vt | FrameKind::Client(_) => set_focus(st, Focus::Term),
        FrameKind::Files => set_focus(st, Focus::Files),
        FrameKind::Settings => set_focus(st, Focus::Desk),
    }
}

pub(super) fn raise_visible(st: &mut State, i: usize) {
    raise(st, i);
}

pub(super) fn minimize_frame(st: &mut State, i: usize) {
    if st.frames[i].minimized {
        return;
    }
    let old = shadow_rect(&st.frames[i]);
    st.frames[i].minimized = true;
    if st
        .drag
        .as_ref()
        .is_some_and(|d| d.kind == st.frames[i].kind)
    {
        st.drag = None;
    }
    serial::write_str("wm: min\n");
    fill_rect_r(st, old);
    present_damage(st, old, false);
    retarget_focus(st);
    sync_panel_mode(st);
    paint_strut(st);
}

pub(super) fn restore_min(st: &mut State, i: usize) {
    st.frames[i].minimized = false;
    raise(st, i);
    focus_frame(st, i);
    present_damage(st, shadow_rect(&st.frames[st.frames.len() - 1]), false);
    sync_panel_mode(st);
    paint_strut(st);
}

pub(super) fn ensure_frame(st: &mut State, kind: FrameKind) -> Option<usize> {
    if let Some(i) = st.frames.iter().position(|f| f.kind == kind) {
        return Some(i);
    }
    let work = h_work(st);
    let (ox, oy, cw, ch) = match kind {
        FrameKind::Vt => {
            let (vw, vh) = crate::fbterm::vt_size()?;
            (16, 8, vw, vh)
        }
        FrameKind::Files => {
            let fm_w = 480u32.min(st.fb.w.saturating_sub(64)).max(fm::SIDE_W + 240);
            let fm_h = 400u32
                .min(work.saturating_sub(48 + DECO_H + 8 + SHADOW_PX))
                .max(fm::NAV_H + fm::COL_H + fm::ROW_H + fm::STATUS_H);
            (48, 48, fm_w, fm_h)
        }
        FrameKind::Settings => {
            let w = 360u32.min(st.fb.w.saturating_sub(64)).max(200);
            let h = 280u32
                .min(work.saturating_sub(40 + DECO_H + 8 + SHADOW_PX))
                .max(120);
            (80, 40, w, h)
        }
        FrameKind::Client(_) => return None,
    };
    let seq = st.seq_next;
    st.seq_next = st.seq_next.saturating_add(1);
    let mut f = frame_new(kind, ox, oy, cw, ch, seq);
    clamp_frame_in(&st.fb, &mut f, work);
    st.frames.push(f);
    Some(st.frames.len() - 1)
}

pub(super) fn close_frame(st: &mut State, i: usize) {
    if i >= st.frames.len() {
        return;
    }
    let kind = st.frames[i].kind;
    let old = shadow_rect(&st.frames[i]);
    if st.drag.as_ref().is_some_and(|d| d.kind == kind) {
        st.drag = None;
    }
    if let FrameKind::Client(id) = kind {
        crate::win::drop_id(id);
    }
    st.frames.remove(i);
    fill_rect_r(st, old);
    present_damage(st, old, true);
    retarget_focus(st);
    sync_panel_mode(st);
    paint_strut(st);
}

pub(super) fn retarget_focus(st: &mut State) {
    let Some(i) = visible_top(st) else {
        set_focus(st, Focus::Term);
        sync_client_top(st);
        return;
    };
    focus_frame(st, i);
    sync_client_top(st);
}

pub(super) fn sync_client_top(st: &State) {
    let top = matches!(
        visible_top(st).map(|i| st.frames[i].kind),
        Some(FrameKind::Client(_))
    );
    CLIENT_TOP.store(top, Ordering::Release);
}

pub(super) fn toggle_max(st: &mut State, i: usize) {
    if st.frames[i].minimized {
        st.frames[i].minimized = false;
    }
    let work = h_work(st);
    let old = shadow_rect(&st.frames[i]);
    if st.frames[i].maximized {
        st.frames[i].ox = st.frames[i].saved_ox;
        st.frames[i].oy = st.frames[i].saved_oy;
        st.frames[i].cw = st.frames[i].saved_cw;
        st.frames[i].ch = st.frames[i].saved_ch;
        st.frames[i].maximized = false;
        clamp_frame_in(&st.fb, &mut st.frames[i], work);
    } else {
        st.frames[i].saved_ox = st.frames[i].ox;
        st.frames[i].saved_oy = st.frames[i].oy;
        st.frames[i].saved_cw = st.frames[i].cw;
        st.frames[i].saved_ch = st.frames[i].ch;
        st.frames[i].ox = 0;
        st.frames[i].oy = 0;
        st.frames[i].cw = st.fb.w;
        st.frames[i].ch = work.saturating_sub(DECO_H).max(16);
        st.frames[i].maximized = true;
    }
    if let FrameKind::Client(id) = st.frames[i].kind {
        crate::win::set_frame_pos(id, st.frames[i].ox, st.frames[i].oy);
    }
    raise(st, i);
    let new = shadow_rect(&st.frames[st.frames.len() - 1]);
    fill_revealed(st, old, opaque_rect(&st.frames[st.frames.len() - 1]));
    present_damage(st, rect_union(old, new), false);
    sync_panel_mode(st);
    paint_strut(st);
}

pub(super) fn sync_panel_mode(st: &mut State) {
    let op = any_maximized(st);
    if st.panel_opaque == op {
        return;
    }
    st.panel_opaque = op;
    log_panel_mode(op);
}

pub(super) fn apply_panel_mode(st: &mut State, m: crate::panel::Mode) {
    if !crate::panel::set_mode(m) {
        return;
    }
    reflow_maximized(st);
    rescale_wallpaper(st);
    fill_work(st);
    paint_all_frames(st, false);
    paint_strut(st);
}

fn reflow_maximized(st: &mut State) {
    let work = h_work(st);
    for i in 0..st.frames.len() {
        if !st.frames[i].maximized || st.frames[i].minimized {
            continue;
        }
        st.frames[i].ox = 0;
        st.frames[i].oy = 0;
        st.frames[i].cw = st.fb.w;
        st.frames[i].ch = work.saturating_sub(DECO_H).max(16);
        if let FrameKind::Client(id) = st.frames[i].kind {
            crate::win::set_frame_pos(id, 0, 0);
        }
    }
}

fn rescale_wallpaper(st: &mut State) {
    let hw = h_work(st);
    if hw == 0 || st.fb.w == 0 {
        return;
    }
    match crate::desk::load_current(st.fb.w, hw) {
        Some(w) => {
            st.wallpaper = w.pix;
            st.wall_w = w.w;
            st.wall_h = w.h;
        }
        None => {}
    }
}

pub(super) fn set_focus(st: &mut State, f: Focus) {
    FOCUS_FILES.store(f == Focus::Files, Ordering::Release);
    FOCUS_DESK.store(f == Focus::Desk, Ordering::Release);
    if st.focus != f {
        st.focus = f;
        log_focus(f);
    }
}

pub(super) fn raise(st: &mut State, i: usize) {
    if i + 1 == st.frames.len() {
        sync_client_top(st);
        return;
    }
    let f = st.frames.remove(i);
    st.frames.push(f);
    sync_client_top(st);
}

const RESIZE: u32 = 5;
const MIN_CW: i32 = 120;
const MIN_CH: i32 = 64;

pub(super) fn hit_test(st: &State, x: u32, y: u32) -> Option<Hit> {
    for i in (0..st.frames.len()).rev() {
        let f = &st.frames[i];
        if f.minimized {
            continue;
        }
        let fh = DECO_H.saturating_add(f.ch);
        if x < f.ox || y < f.oy || x >= f.ox.saturating_add(f.cw) || y >= f.oy.saturating_add(fh) {
            continue;
        }
        if y < f.oy.saturating_add(DECO_H) {
            let bx = f.ox.saturating_add(f.cw);
            if x >= bx.saturating_sub(BTN) {
                return Some(Hit::Close(i));
            }
            if x >= bx.saturating_sub(BTN.saturating_mul(2)) {
                return Some(Hit::Max(i));
            }
            if x >= bx.saturating_sub(BTN.saturating_mul(3)) {
                return Some(Hit::Min(i));
            }
            if let Some(edge) = resize_edge(f, x, y, fh) {
                return Some(Hit::Resize(i, edge));
            }
            return Some(Hit::Title(i));
        }
        if let Some(edge) = resize_edge(f, x, y, fh) {
            return Some(Hit::Resize(i, edge));
        }
        return Some(Hit::Client(i));
    }
    None
}

fn resize_edge(f: &Frame, x: u32, y: u32, fh: u32) -> Option<Edge> {
    if f.maximized {
        return None;
    }
    let on_n = y < f.oy.saturating_add(RESIZE);
    let on_s = y >= f.oy.saturating_add(fh.saturating_sub(RESIZE));
    let on_w = x < f.ox.saturating_add(RESIZE);
    let on_e = x >= f.ox.saturating_add(f.cw.saturating_sub(RESIZE));
    match (on_n, on_s, on_w, on_e) {
        (true, _, true, _) => Some(Edge::Nw),
        (true, _, _, true) => Some(Edge::Ne),
        (_, true, true, _) => Some(Edge::Sw),
        (_, true, _, true) => Some(Edge::Se),
        (true, _, _, _) => Some(Edge::N),
        (_, true, _, _) => Some(Edge::S),
        (_, _, true, _) => Some(Edge::W),
        (_, _, _, true) => Some(Edge::E),
        _ => None,
    }
}

pub(super) fn apply_drag(st: &mut State) {
    let Some(d) = st.drag else {
        return;
    };
    if let Some(edge) = d.edge {
        apply_resize(st, d, edge);
        return;
    }
    let Some(i) = st.frames.iter().position(|f| f.kind == d.kind) else {
        st.drag = None;
        return;
    };
    let old_sh = shadow_rect(&st.frames[i]);
    let work = h_work(st);
    let mut ox = (st.cx - d.gx).max(0) as u32;
    let mut oy = (st.cy - d.gy).max(0) as u32;
    clamp_xy(
        &st.fb,
        work,
        st.frames[i].cw,
        st.frames[i].ch,
        &mut ox,
        &mut oy,
    );
    if ox == st.frames[i].ox && oy == st.frames[i].oy {
        return;
    }
    st.frames[i].ox = ox;
    st.frames[i].oy = oy;
    if let FrameKind::Client(id) = d.kind {
        crate::win::set_frame_pos(id, ox, oy);
    }
    let new_sh = shadow_rect(&st.frames[i]);
    let clip = rect_union(old_sh, new_sh);
    let log_sh = !st.drag_shadow_logged;
    st.drag_shadow_logged = true;
    present_damage(st, clip, log_sh);
}

fn apply_resize(st: &mut State, d: Drag, edge: Edge) {
    let Some(i) = st.frames.iter().position(|f| f.kind == d.kind) else {
        st.drag = None;
        return;
    };
    if st.frames[i].maximized || st.frames[i].minimized {
        return;
    }
    let dx = st.cx - d.gx;
    let dy = st.cy - d.gy;
    let work = h_work(st);
    let west = matches!(edge, Edge::W | Edge::Nw | Edge::Sw);
    let east = matches!(edge, Edge::E | Edge::Ne | Edge::Se);
    let north = matches!(edge, Edge::N | Edge::Ne | Edge::Nw);
    let south = matches!(edge, Edge::S | Edge::Se | Edge::Sw);
    let mut ox = d.ox0 as i32;
    let mut oy = d.oy0 as i32;
    let mut cw = d.cw0 as i32;
    let mut ch = d.ch0 as i32;
    if east {
        cw += dx;
    }
    if south {
        ch += dy;
    }
    if west {
        ox += dx;
        cw -= dx;
    }
    if north {
        oy += dy;
        ch -= dy;
    }
    if cw < MIN_CW {
        if west {
            ox -= MIN_CW - cw;
        }
        cw = MIN_CW;
    }
    if ch < MIN_CH {
        if north {
            oy -= MIN_CH - ch;
        }
        ch = MIN_CH;
    }
    ox = ox.max(0);
    oy = oy.max(0);
    let sh = SHADOW_PX as i32;
    let fw = st.fb.w as i32;
    let wh = work as i32;
    if ox + cw + sh > fw {
        if east && !west {
            cw = (fw - ox - sh).max(MIN_CW);
        } else {
            ox = (fw - cw - sh).max(0);
        }
    }
    if oy + DECO_H as i32 + ch + sh > wh {
        if south && !north {
            ch = (wh - oy - DECO_H as i32 - sh).max(MIN_CH);
        } else {
            oy = (wh - DECO_H as i32 - ch - sh).max(0);
        }
    }
    let ox = ox as u32;
    let oy = oy as u32;
    let cw = cw as u32;
    let ch = ch as u32;
    if ox == st.frames[i].ox
        && oy == st.frames[i].oy
        && cw == st.frames[i].cw
        && ch == st.frames[i].ch
    {
        return;
    }
    let old_sh = shadow_rect(&st.frames[i]);
    st.frames[i].ox = ox;
    st.frames[i].oy = oy;
    st.frames[i].cw = cw;
    st.frames[i].ch = ch;
    if let FrameKind::Client(id) = d.kind {
        crate::win::set_frame_pos(id, ox, oy);
    }
    let new_sh = shadow_rect(&st.frames[i]);
    fill_revealed(st, old_sh, opaque_rect(&st.frames[i]));
    let log_sh = !st.drag_shadow_logged;
    st.drag_shadow_logged = true;
    present_damage(st, rect_union(old_sh, new_sh), log_sh);
}

pub(super) fn clamp_frame_in(fb: &FbInfo, f: &mut Frame, work: u32) {
    let mut ox = f.ox;
    let mut oy = f.oy;
    clamp_xy(fb, work, f.cw, f.ch, &mut ox, &mut oy);
    f.ox = ox;
    f.oy = oy;
}

pub(super) fn clamp_xy(fb: &FbInfo, work: u32, cw: u32, ch: u32, ox: &mut u32, oy: &mut u32) {
    let fh = DECO_H.saturating_add(ch);
    if ox.saturating_add(cw).saturating_add(SHADOW_PX) > fb.w {
        *ox = fb.w.saturating_sub(cw.saturating_add(SHADOW_PX));
    }
    if oy.saturating_add(fh).saturating_add(SHADOW_PX) > work {
        *oy = work.saturating_sub(fh.saturating_add(SHADOW_PX));
    }
}
