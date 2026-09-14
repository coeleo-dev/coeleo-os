//! Window, strut, and overlay geometry.

use alloc::vec::Vec;

use super::rect::{Rect, rect_contains};
use super::state::{Frame, FrameKind, State};
use crate::panel;
use coeleo_draw::Icon;
use coeleo_theme::{DECO_H, GAP, PANEL_MARGIN, SHADOW_OFF, SHADOW_PX};

pub(super) const LAUNCH_NAMES: [&str; 5] = ["hello", "widgets", "winprobe", "install", "edit"];
pub(super) const LAUNCH_ROW: u32 = coeleo_draw::FONT_H + coeleo_theme::GAP;
pub(super) const LAUNCH_W: u32 = 196;
pub(super) const LAUNCH_PAD: u32 = coeleo_theme::PAD;
pub(super) const LAUNCH_SEP: u32 = 1;
pub(super) const POWER_FOOTER: u32 = 2;
pub(super) const CONFIRM_W: u32 = 288;
pub(super) const CONFIRM_BTN_H: u32 = coeleo_theme::BUTTON_H;

pub(super) fn h_work(st: &State) -> u32 {
    panel::work_h(st.fb.h)
}

pub(super) fn frame_new(kind: FrameKind, ox: u32, oy: u32, cw: u32, ch: u32, seq: u32) -> Frame {
    Frame {
        kind,
        ox,
        oy,
        cw,
        ch,
        seq,
        minimized: false,
        maximized: false,
        saved_ox: 0,
        saved_oy: 0,
        saved_cw: 0,
        saved_ch: 0,
    }
}

pub(super) fn frame_title(kind: FrameKind) -> &'static str {
    match kind {
        FrameKind::Vt => "Terminal",
        FrameKind::Files => "Files",
        FrameKind::Settings => "Settings",
        FrameKind::Client(_) => "Application",
    }
}

pub(super) fn frame_icon(kind: FrameKind) -> Icon {
    match kind {
        FrameKind::Vt => Icon::Terminal,
        FrameKind::Files => Icon::Folder,
        _ => Icon::App,
    }
}

pub(super) fn app_label(name: &str) -> &str {
    match name {
        "files" => "Files",
        "sh" => "Terminal",
        "hello" => "Hello",
        "widgets" => "Widgets",
        "winprobe" => "Winprobe",
        "install" => "Install Coeleo",
        "edit" => "Edit",
        other => other,
    }
}

pub(super) fn app_icon(name: &str) -> Icon {
    match name {
        "files" => Icon::Folder,
        "sh" => Icon::Terminal,
        _ => Icon::App,
    }
}

pub(super) fn visible_top(st: &State) -> Option<usize> {
    st.frames.iter().rposition(|f| !f.minimized)
}

pub(super) fn any_maximized(st: &State) -> bool {
    st.frames.iter().any(|f| f.maximized && !f.minimized)
}

pub(super) fn launcher_sel_count(st: &State) -> usize {
    st.launch_list.len() + POWER_FOOTER as usize
}

pub(super) fn launcher_popup(st: &State) -> Rect {
    let n_apps = st.launch_list.len() as u32;
    let h = LAUNCH_PAD
        .saturating_mul(2)
        .saturating_add(n_apps.saturating_mul(LAUNCH_ROW))
        .saturating_add(LAUNCH_SEP)
        .saturating_add(POWER_FOOTER.saturating_mul(LAUNCH_ROW));
    let work = h_work(st);
    let y1 = work.saturating_sub(GAP);
    let y0 = y1.saturating_sub(h);
    let x0 = panel::slot_x0();
    Rect {
        x0,
        y0,
        x1: x0.saturating_add(LAUNCH_W).min(st.fb.w),
        y1,
    }
}

pub(super) fn launcher_shadow(st: &State) -> Rect {
    let op = launcher_popup(st);
    overlay_shadow(st, op)
}

const DESK_MENU_W: u32 = 180;
const FILES_MENU_W: u32 = 160;
const FILES_MENU_N: u32 = 3;

pub(super) fn desk_menu_popup(st: &State) -> Rect {
    let work = h_work(st);
    let h = LAUNCH_PAD.saturating_mul(2).saturating_add(LAUNCH_ROW);
    let w = DESK_MENU_W.min(st.fb.w);
    let mut x0 = st.desk_menu_x;
    let mut y0 = st.desk_menu_y;
    if x0.saturating_add(w) > st.fb.w {
        x0 = st.fb.w.saturating_sub(w);
    }
    if y0.saturating_add(h) > work {
        y0 = work.saturating_sub(h);
    }
    Rect {
        x0,
        y0,
        x1: x0.saturating_add(w),
        y1: y0.saturating_add(h),
    }
}

pub(super) fn desk_menu_shadow(st: &State) -> Rect {
    overlay_shadow(st, desk_menu_popup(st))
}

pub(super) fn desk_menu_row_at(st: &State, x: u32, y: u32) -> bool {
    let p = desk_menu_popup(st);
    rect_contains(p, x, y)
}

pub(super) fn files_menu_popup(st: &State) -> Rect {
    let work = h_work(st);
    let h = LAUNCH_PAD
        .saturating_mul(2)
        .saturating_add(LAUNCH_ROW.saturating_mul(FILES_MENU_N));
    let w = FILES_MENU_W.min(st.fb.w);
    let mut x0 = st.files_menu_x;
    let mut y0 = st.files_menu_y;
    if x0.saturating_add(w) > st.fb.w {
        x0 = st.fb.w.saturating_sub(w);
    }
    if y0.saturating_add(h) > work {
        y0 = work.saturating_sub(h);
    }
    Rect {
        x0,
        y0,
        x1: x0.saturating_add(w),
        y1: y0.saturating_add(h),
    }
}

pub(super) fn files_menu_shadow(st: &State) -> Rect {
    overlay_shadow(st, files_menu_popup(st))
}

pub(super) fn files_menu_row_at(st: &State, x: u32, y: u32) -> Option<usize> {
    let p = files_menu_popup(st);
    if !rect_contains(p, x, y) {
        return None;
    }
    let rel = y.saturating_sub(p.y0.saturating_add(LAUNCH_PAD));
    let i = (rel / LAUNCH_ROW) as usize;
    if i < FILES_MENU_N as usize {
        Some(i)
    } else {
        None
    }
}

fn overlay_shadow(st: &State, op: Rect) -> Rect {
    let up = SHADOW_PX.saturating_sub(SHADOW_OFF);
    Rect {
        x0: op.x0.saturating_sub(up),
        y0: op.y0.saturating_sub(up),
        x1: op.x1.saturating_add(SHADOW_PX).min(st.fb.w),
        y1: op.y1.saturating_add(SHADOW_PX).min(h_work(st)),
    }
}

pub(super) fn launcher_row_at(st: &State, x: u32, y: u32) -> Option<usize> {
    let p = launcher_popup(st);
    if !rect_contains(p, x, y) {
        return None;
    }
    let rel = y.saturating_sub(p.y0.saturating_add(LAUNCH_PAD));
    let n = st.launch_list.len();
    let apps_h = (n as u32).saturating_mul(LAUNCH_ROW);
    if rel < apps_h {
        let i = (rel / LAUNCH_ROW) as usize;
        if i < n { Some(i) } else { None }
    } else {
        let r2 = rel.saturating_sub(apps_h.saturating_add(LAUNCH_SEP));
        let i = (r2 / LAUNCH_ROW) as usize;
        if i < POWER_FOOTER as usize {
            Some(n + i)
        } else {
            None
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ConfirmHit {
    Cancel,
    Ok,
    Body,
    Outside,
}

pub(super) fn confirm_popup(st: &State) -> Rect {
    let work = h_work(st);
    let h = coeleo_theme::PAD
        .saturating_mul(2)
        .saturating_add(coeleo_draw::FONT_H)
        .saturating_add(GAP)
        .saturating_add(CONFIRM_BTN_H);
    let w = CONFIRM_W.min(st.fb.w.saturating_sub(PANEL_MARGIN.saturating_mul(2)));
    let x0 = st.fb.w.saturating_sub(w) / 2;
    let y0 = work.saturating_sub(h) / 2;
    Rect {
        x0,
        y0,
        x1: x0.saturating_add(w),
        y1: y0.saturating_add(h),
    }
}

pub(super) fn confirm_shadow(st: &State) -> Rect {
    let op = confirm_popup(st);
    let up = SHADOW_PX.saturating_sub(SHADOW_OFF);
    Rect {
        x0: op.x0.saturating_sub(up),
        y0: op.y0.saturating_sub(up),
        x1: op.x1.saturating_add(SHADOW_PX).min(st.fb.w),
        y1: op.y1.saturating_add(SHADOW_PX).min(h_work(st)),
    }
}

pub(super) fn confirm_btns(st: &State) -> (Rect, Rect) {
    let p = confirm_popup(st);
    let by =
        p.y1.saturating_sub(coeleo_theme::PAD.saturating_add(CONFIRM_BTN_H));
    let inner =
        p.x1.saturating_sub(p.x0)
            .saturating_sub(coeleo_theme::PAD * 3);
    let bw = inner / 2;
    let cancel = Rect {
        x0: p.x0.saturating_add(coeleo_theme::PAD),
        y0: by,
        x1: p.x0.saturating_add(coeleo_theme::PAD).saturating_add(bw),
        y1: by.saturating_add(CONFIRM_BTN_H),
    };
    let ok = Rect {
        x0: cancel.x1.saturating_add(coeleo_theme::PAD),
        y0: by,
        x1: p.x1.saturating_sub(coeleo_theme::PAD),
        y1: by.saturating_add(CONFIRM_BTN_H),
    };
    (cancel, ok)
}

pub(super) fn confirm_hit(st: &State, x: u32, y: u32) -> ConfirmHit {
    let p = confirm_popup(st);
    if !rect_contains(p, x, y) {
        return ConfirmHit::Outside;
    }
    let (cancel, ok) = confirm_btns(st);
    if rect_contains(cancel, x, y) {
        ConfirmHit::Cancel
    } else if rect_contains(ok, x, y) {
        ConfirmHit::Ok
    } else {
        ConfirmHit::Body
    }
}

pub(super) fn krunner_popup(st: &State) -> crate::krunner::Rect {
    crate::krunner::popup(st.fb.w, h_work(st), st.runner_list.len())
}

pub(super) fn krunner_shadow(st: &State) -> Rect {
    let s = crate::krunner::shadow(krunner_popup(st), st.fb.w, h_work(st));
    Rect {
        x0: s.x0,
        y0: s.y0,
        x1: s.x1,
        y1: s.y1,
    }
}

pub(super) fn shadow_bounds(f: &Frame) -> (u32, u32, u32, u32) {
    let r = shadow_rect(f);
    (r.x0, r.y0, r.x1, r.y1)
}

pub(super) fn opaque_rect(f: &Frame) -> Rect {
    Rect {
        x0: f.ox,
        y0: f.oy,
        x1: f.ox.saturating_add(f.cw),
        y1: f.oy.saturating_add(DECO_H).saturating_add(f.ch),
    }
}

pub(super) fn shadow_rect(f: &Frame) -> Rect {
    let fh = DECO_H.saturating_add(f.ch);
    let up = SHADOW_PX.saturating_sub(SHADOW_OFF);
    Rect {
        x0: f.ox.saturating_sub(up),
        y0: f.oy.saturating_sub(up),
        x1: f.ox.saturating_add(f.cw).saturating_add(SHADOW_PX),
        y1: f.oy.saturating_add(fh).saturating_add(SHADOW_PX),
    }
}

pub(super) fn work_rect(st: &State) -> Rect {
    Rect {
        x0: 0,
        y0: 0,
        x1: st.fb.w,
        y1: h_work(st),
    }
}

pub(super) fn strut_rect(st: &State) -> Rect {
    Rect {
        x0: 0,
        y0: h_work(st),
        x1: st.fb.w,
        y1: st.fb.h,
    }
}

pub(super) fn fb_rect(st: &State) -> Rect {
    Rect {
        x0: 0,
        y0: 0,
        x1: st.fb.w,
        y1: st.fb.h,
    }
}

pub(super) fn is_strut_rect(st: &State, r: Rect) -> bool {
    let s = strut_rect(st);
    r.x0 == s.x0 && r.y0 == s.y0 && r.x1 == s.x1 && r.y1 == s.y1
}

pub(super) fn task_infos(st: &State) -> Vec<panel::TaskInfo> {
    let top = visible_top(st);
    let mut out = Vec::new();
    if let Some(i) = st.frames.iter().position(|f| f.kind == FrameKind::Files) {
        let f = &st.frames[i];
        out.push(panel::TaskInfo {
            title: "Files",
            focused: top == Some(i),
            minimized: f.minimized,
            icon: Icon::Folder,
        });
    }
    if let Some(i) = st.frames.iter().position(|f| f.kind == FrameKind::Settings) {
        let f = &st.frames[i];
        out.push(panel::TaskInfo {
            title: "Settings",
            focused: top == Some(i),
            minimized: f.minimized,
            icon: Icon::App,
        });
    }
    if let Some(i) = st.frames.iter().position(|f| f.kind == FrameKind::Vt) {
        let f = &st.frames[i];
        out.push(panel::TaskInfo {
            title: "Terminal",
            focused: top == Some(i),
            minimized: f.minimized,
            icon: Icon::Terminal,
        });
    }
    for (i, f) in st.frames.iter().enumerate() {
        if let FrameKind::Client(_) = f.kind {
            out.push(panel::TaskInfo {
                title: "Application",
                focused: top == Some(i),
                minimized: f.minimized,
                icon: Icon::App,
            });
        }
    }
    out
}

pub(super) fn task_frame(st: &State, k: usize) -> Option<usize> {
    let mut n = 0usize;
    if let Some(i) = st.frames.iter().position(|f| f.kind == FrameKind::Files) {
        if n == k {
            return Some(i);
        }
        n += 1;
    }
    if let Some(i) = st.frames.iter().position(|f| f.kind == FrameKind::Settings) {
        if n == k {
            return Some(i);
        }
        n += 1;
    }
    if let Some(i) = st.frames.iter().position(|f| f.kind == FrameKind::Vt) {
        if n == k {
            return Some(i);
        }
        n += 1;
    }
    for (i, f) in st.frames.iter().enumerate() {
        if let FrameKind::Client(_) = f.kind {
            if n == k {
                return Some(i);
            }
            n += 1;
        }
    }
    None
}
