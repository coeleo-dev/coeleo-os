//! Scene paint: wallpaper, shadow, deco, clients, overlays.

use alloc::vec::Vec;

use crate::fbterm::{self, FbInfo};
use crate::fm;
use crate::panel;
use crate::serial;
use coeleo_draw::{self, Clip, Target};
use coeleo_theme::{
    ACCENT, BG, BORDER, BORDER_LIGHT, DANGER, DECO_BTN_PAD, DECO_H, DIM, GAP, HOVER, PAD, RADIUS,
    SHADOW, SHADOW_A, SHADOW_PX, SURFACE, SURFACE_RAISED, TEXT,
};

use super::damage::note;
use super::fb::{blend, blit_u32_at_round, fill_span, get_fb, put_fb, shade_px, tgt};
use super::files::FilesSink;
use super::geom::{
    LAUNCH_PAD, LAUNCH_ROW, LAUNCH_SEP, POWER_FOOTER, any_maximized, app_icon, app_label,
    confirm_btns, confirm_popup, confirm_shadow, desk_menu_popup, desk_menu_shadow,
    files_menu_popup, files_menu_shadow, frame_icon, frame_title, h_work, krunner_popup, launcher_popup,
    launcher_shadow, opaque_rect, shadow_bounds, shadow_rect, strut_rect, task_infos, visible_top,
    work_rect,
};
use super::log::{log_panel, log_panel_mode, log_shadow};
use super::rect::{
    RECT_EMPTY, Rect, rect_contains, rect_intersect, rect_intersects, rect_is_empty, rect_sub,
    rect_union,
};
use super::state::{BTN, Frame, FrameKind, PowerKind, State};

pub(super) fn paint_clock(st: &mut State) {
    let mut time = [0u8; 16];
    let text = crate::clock::format_panel(&mut time);
    let wall = if !st.wallpaper.is_empty() && st.wall_w != 0 {
        Some((st.wallpaper.as_slice(), st.wall_w, st.wall_h))
    } else {
        None
    };
    panel::paint_clock(st.scene_fb, text, any_maximized(st), wall);
    let (x, y, w, h) = panel::clock_rect(st.fb.w, st.fb.h, text);
    note(
        st,
        Rect {
            x0: x,
            y0: y,
            x1: x.saturating_add(w),
            y1: y.saturating_add(h),
        },
    );
}

pub(super) fn paint_strut(st: &mut State) {
    let mut time = [0u8; 16];
    let text = crate::clock::format_panel(&mut time);
    let tasks = task_infos(st);
    let wall = if !st.wallpaper.is_empty() && st.wall_w != 0 {
        Some((st.wallpaper.as_slice(), st.wall_w, st.wall_h))
    } else {
        None
    };
    let icon = if st.menu_iw != 0 && !st.menu_icon.is_empty() {
        Some((st.menu_icon.as_slice(), st.menu_iw, st.menu_ih))
    } else {
        None
    };
    st.tip = panel::paint(
        st.scene_fb,
        text,
        &tasks,
        st.hover,
        st.pressed,
        any_maximized(st),
        wall,
        icon,
    );
    let mut r = strut_rect(st);
    if !st.tip.is_empty() {
        r = rect_union(
            r,
            Rect {
                x0: st.tip.x,
                y0: st.tip.y,
                x1: st.tip.x.saturating_add(st.tip.w),
                y1: st.tip.y.saturating_add(st.tip.h),
            },
        );
    }
    note(st, r);
}
pub(super) fn paint_launcher_clip(st: &mut State, clip: Rect) {
    let op = launcher_popup(st);
    let sh = launcher_shadow(st);
    if !rect_intersects(sh, clip) {
        return;
    }
    let t = tgt(st.scene_fb);
    let vis_sh = rect_intersect(sh, clip);
    shade_round_ring(
        st, vis_sh, RECT_EMPTY, op.x0, op.y0, op.x1, op.y1, sh.x0, sh.y0, sh.x1, sh.y1,
    );
    let vis = rect_intersect(op, clip);
    if rect_is_empty(vis) {
        return;
    }
    let dclip = Clip {
        x0: vis.x0,
        y0: vis.y0,
        x1: vis.x1,
        y1: vis.y1,
    };
    let pw = op.x1.saturating_sub(op.x0);
    let ph = op.y1.saturating_sub(op.y0);
    coeleo_draw::fill_round(t, op.x0, op.y0, pw, ph, RADIUS, SURFACE, dclip);
    let ty0 = op.y0.saturating_add(LAUNCH_PAD);
    let n = st.launch_list.len();
    for i in 0..n {
        let y = ty0.saturating_add(i as u32 * LAUNCH_ROW);
        let name = st.launch_list[i].clone();
        let hovered = i == st.launch_sel;
        coeleo_draw::list_row(
            t,
            op.x0.saturating_add(PAD),
            y,
            pw.saturating_sub(PAD * 2),
            LAUNCH_ROW,
            app_label(&name),
            Some(app_icon(&name)),
            hovered,
            false,
            dclip,
        );
    }
    let sep_y = ty0.saturating_add((n as u32).saturating_mul(LAUNCH_ROW));
    if sep_y >= vis.y0 && sep_y < vis.y1 {
        let sx0 = op.x0.saturating_add(PAD).max(vis.x0);
        let sx1 = op.x1.saturating_sub(PAD).min(vis.x1);
        if sx1 > sx0 {
            coeleo_draw::fill_span(t, sx0, sep_y, sx1.saturating_sub(sx0), DIM);
        }
    }
    let labels = ["Restart", "Power off"];
    for f in 0..POWER_FOOTER as usize {
        let y = ty0
            .saturating_add((n as u32).saturating_mul(LAUNCH_ROW))
            .saturating_add(LAUNCH_SEP)
            .saturating_add(f as u32 * LAUNCH_ROW);
        let sel = n + f;
        let hovered = st.launch_sel == sel;
        if hovered {
            if f == 1 {
                coeleo_draw::danger_fill(
                    t,
                    op.x0.saturating_add(PAD),
                    y,
                    pw.saturating_sub(PAD * 2),
                    LAUNCH_ROW,
                    dclip,
                );
            } else {
                coeleo_draw::highlight_fill(
                    t,
                    op.x0.saturating_add(PAD),
                    y,
                    pw.saturating_sub(PAD * 2),
                    LAUNCH_ROW,
                    dclip,
                );
            }
        }
        let color = if f == 1 { DANGER } else { TEXT };
        deco_text(
            st,
            op.x0.saturating_add(PAD),
            y.saturating_add(GAP / 2),
            labels[f],
            color,
            op.x1,
            clip,
            RECT_EMPTY,
        );
    }
}

pub(super) fn paint_desk_menu_clip(st: &mut State, clip: Rect) {
    let op = desk_menu_popup(st);
    let sh = desk_menu_shadow(st);
    if !rect_intersects(sh, clip) {
        return;
    }
    let t = tgt(st.scene_fb);
    let vis_sh = rect_intersect(sh, clip);
    shade_round_ring(
        st, vis_sh, RECT_EMPTY, op.x0, op.y0, op.x1, op.y1, sh.x0, sh.y0, sh.x1, sh.y1,
    );
    let vis = rect_intersect(op, clip);
    if rect_is_empty(vis) {
        return;
    }
    let dclip = Clip {
        x0: vis.x0,
        y0: vis.y0,
        x1: vis.x1,
        y1: vis.y1,
    };
    let pw = op.x1.saturating_sub(op.x0);
    let ph = op.y1.saturating_sub(op.y0);
    coeleo_draw::fill_round(t, op.x0, op.y0, pw, ph, RADIUS, SURFACE, dclip);
    let y = op.y0.saturating_add(LAUNCH_PAD);
    coeleo_draw::list_row(
        t,
        op.x0.saturating_add(PAD),
        y,
        pw.saturating_sub(PAD * 2),
        LAUNCH_ROW,
        "Desktop settings",
        Some(coeleo_draw::Icon::App),
        true,
        false,
        dclip,
    );
}

pub(super) fn paint_files_menu_clip(st: &mut State, clip: Rect) {
    let op = files_menu_popup(st);
    let sh = files_menu_shadow(st);
    if !rect_intersects(sh, clip) {
        return;
    }
    let t = tgt(st.scene_fb);
    let vis_sh = rect_intersect(sh, clip);
    shade_round_ring(
        st, vis_sh, RECT_EMPTY, op.x0, op.y0, op.x1, op.y1, sh.x0, sh.y0, sh.x1, sh.y1,
    );
    let vis = rect_intersect(op, clip);
    if rect_is_empty(vis) {
        return;
    }
    let dclip = Clip {
        x0: vis.x0,
        y0: vis.y0,
        x1: vis.x1,
        y1: vis.y1,
    };
    let pw = op.x1.saturating_sub(op.x0);
    let ph = op.y1.saturating_sub(op.y0);
    coeleo_draw::fill_round(t, op.x0, op.y0, pw, ph, RADIUS, SURFACE, dclip);
    let flags = crate::fm::menu_flags();
    let labels = ["Open", "Go up", "Delete"];
    let enabled = [flags.can_open, flags.can_up, flags.can_delete];
    let mx = st.cx.max(0) as u32;
    let my = st.cy.max(0) as u32;
    for i in 0..3usize {
        let y = op
            .y0
            .saturating_add(LAUNCH_PAD)
            .saturating_add(i as u32 * LAUNCH_ROW);
        let hovered = mx >= op.x0 && mx < op.x1 && my >= y && my < y.saturating_add(LAUNCH_ROW);
        let color = if !enabled[i] {
            DIM
        } else if i == 2 {
            DANGER
        } else {
            TEXT
        };
        coeleo_draw::list_row_fg(
            t,
            op.x0.saturating_add(PAD),
            y,
            pw.saturating_sub(PAD * 2),
            LAUNCH_ROW,
            labels[i],
            None,
            hovered && enabled[i],
            false,
            color,
            dclip,
        );
    }
}

pub(super) fn paint_krunner_clip(st: &mut State, clip: Rect) {
    let pop = krunner_popup(st);
    let sh = crate::krunner::shadow(pop, st.fb.w, h_work(st));
    crate::krunner::paint(
        st.scene_fb,
        crate::krunner::Rect {
            x0: clip.x0,
            y0: clip.y0,
            x1: clip.x1,
            y1: clip.y1,
        },
        pop,
        sh,
        &st.runner_query[..st.runner_query_len],
        &st.runner_list,
        st.runner_sel,
        st.runner_caret,
        st.cx.max(0) as u32,
        st.cy.max(0) as u32,
    );
}

pub(super) fn paint_confirm_clip(st: &mut State, clip: Rect) {
    if st.power_dlg.is_none() {
        return;
    }
    let op = confirm_popup(st);
    let sh = confirm_shadow(st);
    let work = work_rect(st);
    if !rect_intersects(work, clip) && !rect_intersects(sh, clip) {
        return;
    }
    let t = tgt(st.scene_fb);
    let vis_work = rect_intersect(work, clip);
    if !rect_is_empty(vis_work) {
        coeleo_draw::scrim(
            t,
            vis_work.x0,
            vis_work.y0,
            vis_work.x1.saturating_sub(vis_work.x0),
            vis_work.y1.saturating_sub(vis_work.y0),
            Clip {
                x0: vis_work.x0,
                y0: vis_work.y0,
                x1: vis_work.x1,
                y1: vis_work.y1,
            },
        );
    }
    let vis_sh = rect_intersect(sh, clip);
    shade_round_ring(
        st, vis_sh, RECT_EMPTY, op.x0, op.y0, op.x1, op.y1, sh.x0, sh.y0, sh.x1, sh.y1,
    );
    let vis = rect_intersect(op, clip);
    if rect_is_empty(vis) {
        return;
    }
    let dclip = Clip {
        x0: vis.x0,
        y0: vis.y0,
        x1: vis.x1,
        y1: vis.y1,
    };
    let pw = op.x1.saturating_sub(op.x0);
    let ph = op.y1.saturating_sub(op.y0);
    coeleo_draw::fill_stroke_round(t, op.x0, op.y0, pw, ph, RADIUS, SURFACE_RAISED, BORDER_LIGHT, dclip);
    let title = match st.power_dlg {
        Some(PowerKind::Reboot) => "Restart the computer?",
        Some(PowerKind::PowerOff) => "Turn off the computer?",
        None => "",
    };
    deco_text(
        st,
        op.x0.saturating_add(PAD),
        op.y0.saturating_add(PAD),
        title,
        TEXT,
        op.x1,
        clip,
        RECT_EMPTY,
    );
    let (cancel, ok) = confirm_btns(st);
    let mx = st.cx.max(0) as u32;
    let my = st.cy.max(0) as u32;
    let paint_btn = |st: &mut State, r: Rect, label: &str, focused: bool, danger: bool| {
        let t = tgt(st.scene_fb);
        let bw = r.x1.saturating_sub(r.x0);
        let bh = r.y1.saturating_sub(r.y0);
        let hover = mx >= r.x0 && mx < r.x1 && my >= r.y0 && my < r.y1;
        if focused {
            coeleo_draw::highlight_fill(t, r.x0, r.y0, bw, bh, dclip);
        } else if hover {
            if danger {
                coeleo_draw::danger_fill(t, r.x0, r.y0, bw, bh, dclip);
            } else {
                coeleo_draw::hover_fill(t, r.x0, r.y0, bw, bh, dclip);
            }
        } else {
            coeleo_draw::fill_stroke_round(
                t,
                r.x0,
                r.y0,
                bw,
                bh,
                coeleo_theme::RADIUS_SM,
                SURFACE,
                BORDER,
                dclip,
            );
        }
        let color = if danger { DANGER } else { TEXT };
        deco_text(
            st,
            r.x0.saturating_add(PAD / 2),
            r.y0.saturating_add((bh.saturating_sub(coeleo_draw::FONT_H)) / 2),
            label,
            color,
            r.x1,
            clip,
            RECT_EMPTY,
        );
    };
    paint_btn(st, cancel, "Cancel", !st.power_ok, false);
    let ok_label = match st.power_dlg {
        Some(PowerKind::Reboot) => "Restart",
        Some(PowerKind::PowerOff) => "Power off",
        None => "OK",
    };
    paint_btn(st, ok, ok_label, st.power_ok, true);
}

pub(super) fn fill_vis_bg(st: &mut State, vis: Rect, protect: Rect, f: &Frame) {
    if rect_is_empty(protect) {
        fill_vis_bg_rect(st, vis, f);
        return;
    }
    let (parts, n) = rect_sub(vis, protect);
    for k in 0..n {
        if !rect_is_empty(parts[k]) {
            fill_vis_bg_rect(st, parts[k], f);
        }
    }
}

fn fill_vis_bg_rect(st: &mut State, r: Rect, f: &Frame) {
    let bg = BG.to_le_bytes();
    let fh = DECO_H.saturating_add(f.ch);
    let y_inner0 = f.oy.saturating_add(RADIUS);
    let y_inner1 = f.oy.saturating_add(fh).saturating_sub(RADIUS);
    for y in r.y0..r.y1 {
        if y >= y_inner0 && y < y_inner1 {
            fill_span(&st.scene_fb, r.x0, y, r.x1.saturating_sub(r.x0), bg);
            continue;
        }
        for x in r.x0..r.x1 {
            let cov = coeleo_draw::coverage_round(x, y, f.ox, f.oy, f.cw, fh, RADIUS);
            if cov == 0 {
                continue;
            }
            if cov == 255 {
                put_fb(&st.scene_fb, x, y, bg);
            } else {
                put_fb(
                    &st.scene_fb,
                    x,
                    y,
                    blend(get_fb(&st.scene_fb, x as i32, y as i32), BG, cov),
                );
            }
        }
    }
}
pub(super) fn fill_work(st: &mut State) {
    let work = h_work(st);
    fill_rect(st, 0, 0, st.fb.w, work);
}

pub(super) fn fill_rect(st: &mut State, x0: u32, y0: u32, x1: u32, y1: u32) {
    fill_rect_r(st, Rect { x0, y0, x1, y1 });
}

pub(super) fn fill_rect_r(st: &mut State, r: Rect) {
    let work = h_work(st);
    let x0 = r.x0.min(st.fb.w);
    let y0 = r.y0.min(work);
    let x1 = r.x1.min(st.fb.w);
    let y1 = r.y1.min(work);
    if x0 >= x1 || y0 >= y1 {
        return;
    }
    if !st.wallpaper.is_empty() && st.wall_w != 0 && st.wall_h != 0 && st.wall_w == st.fb.w {
        let n = (x1 - x0) as usize;
        for y in y0..y1 {
            let sy = y.min(st.wall_h.saturating_sub(1));
            let src = (sy * st.wall_w + x0) as usize;
            let off = ((y * st.scene_fb.pitch) / 4 + x0) as usize;
            if src + n <= st.wallpaper.len() {
                unsafe {
                    core::ptr::copy_nonoverlapping(
                        st.wallpaper.as_ptr().add(src),
                        (st.scene_fb.addr as *mut u32).add(off),
                        n,
                    );
                }
            }
        }
    } else if !st.wallpaper.is_empty() && st.wall_w != 0 && st.wall_h != 0 {
        for y in y0..y1 {
            let off = ((y * st.scene_fb.pitch) / 4 + x0) as usize;
            unsafe {
                let p = st.scene_fb.addr as *mut u32;
                for i in 0..(x1 - x0) as usize {
                    *p.add(off + i) = wall_px(st, x0 + i as u32, y);
                }
            }
        }
    } else {
        let px = BG.to_le_bytes();
        for y in y0..y1 {
            fill_span(&st.scene_fb, x0, y, x1.saturating_sub(x0), px);
        }
    }
    note(st, Rect { x0, y0, x1, y1 });
}

pub(super) fn fill_revealed(st: &mut State, old_sh: Rect, new_op: Rect) {
    let (parts, n) = rect_sub(old_sh, new_op);
    for k in 0..n {
        fill_rect_r(st, parts[k]);
    }
}
pub(super) fn paint_all_frames(st: &mut State, log_sh: bool) {
    present_damage(st, work_rect(st), log_sh);
}

pub(super) fn present_damage(st: &mut State, clip: Rect, log_sh: bool) {
    present_damage_protect(st, clip, RECT_EMPTY, None, log_sh);
}

pub(super) fn present_damage_protect(
    st: &mut State,
    clip: Rect,
    protect: Rect,
    skip_opaque: Option<usize>,
    log_sh: bool,
) {
    let clip = rect_intersect(clip, work_rect(st));
    if rect_is_empty(clip) {
        return;
    }
    if rect_is_empty(protect) {
        fill_rect_r(st, clip);
    } else {
        let (parts, n) = rect_sub(clip, protect);
        for k in 0..n {
            if !rect_is_empty(parts[k]) {
                fill_rect_r(st, parts[k]);
            }
        }
    }
    let clients = if need_client_snap(st, clip, skip_opaque) {
        crate::win::snapshot_clients()
    } else {
        Vec::new()
    };
    let n = st.frames.len();
    for i in 0..n {
        if st.frames[i].minimized {
            continue;
        }
        if !rect_intersects(shadow_rect(&st.frames[i]), clip) {
            continue;
        }
        paint_shadow_clip(st, i, clip, protect);
        paint_deco_clip(st, i, clip, protect);
        if skip_opaque != Some(i) {
            paint_client_clip(st, i, &clients, clip, protect);
        }
        let focused = visible_top(st) == Some(i);
        paint_frame_outline(st, i, clip, protect, focused);
    }
    if st.launcher_open {
        paint_launcher_clip(st, clip);
    }
    if st.runner_open {
        paint_krunner_clip(st, clip);
    }
    if st.power_dlg.is_some() {
        paint_confirm_clip(st, clip);
    }
    if st.desk_menu_open {
        paint_desk_menu_clip(st, clip);
    }
    if st.files_menu_open {
        paint_files_menu_clip(st, clip);
    }
    if !st.toast.is_empty() {
        paint_toast_clip(st, clip);
    }
    if log_sh {
        log_shadow();
    }
    note(st, clip);
}

pub(super) fn paint_toast_clip(st: &mut State, clip: Rect) {
    let msg = &st.toast;
    let tw = coeleo_draw::text_width(msg);
    let pw = tw.saturating_add(32);
    let ph = 32u32;
    let px = st.fb.w.saturating_sub(pw) / 2;
    let py = panel::work_h(st.fb.h).saturating_sub(40);
    let r = Rect { x0: px, y0: py, x1: px + pw, y1: py + ph };
    if !rect_intersects(r, clip) {
        return;
    }
    let vis = rect_intersect(r, clip);
    let t = tgt(st.scene_fb);
    let dclip = Clip { x0: vis.x0, y0: vis.y0, x1: vis.x1, y1: vis.y1 };
    coeleo_draw::fill_stroke_round(
        t,
        px,
        py,
        pw,
        ph,
        RADIUS,
        SURFACE_RAISED,
        BORDER_LIGHT,
        dclip,
    );
    coeleo_draw::text(
        t,
        px + 16,
        py + (ph - coeleo_draw::FONT_H) / 2,
        msg,
        TEXT,
        vis.x1,
        dclip,
    );
}

pub(super) fn need_client_snap(st: &State, clip: Rect, skip_opaque: Option<usize>) -> bool {
    for (i, f) in st.frames.iter().enumerate() {
        if skip_opaque == Some(i) || f.minimized {
            continue;
        }
        if let FrameKind::Client(_) = f.kind {
            let dest = Rect {
                x0: f.ox,
                y0: f.oy.saturating_add(DECO_H),
                x1: f.ox.saturating_add(f.cw),
                y1: f.oy.saturating_add(DECO_H).saturating_add(f.ch),
            };
            if rect_intersects(dest, clip) {
                return true;
            }
        }
    }
    false
}

pub(super) fn paint_shadow_clip(st: &mut State, i: usize, clip: Rect, protect: Rect) {
    let f = st.frames[i];
    if f.maximized {
        return;
    }
    let x0 = f.ox;
    let y0 = f.oy;
    let x1 = f.ox.saturating_add(f.cw);
    let y1 = f.oy.saturating_add(DECO_H).saturating_add(f.ch);
    let (sx0, sy0, sx1, sy1) = shadow_bounds(&f);
    shade_round_ring(st, clip, protect, x0, y0, x1, y1, sx0, sy0, sx1, sy1);
}

fn shade_round_ring(
    st: &mut State,
    clip: Rect,
    protect: Rect,
    x0: u32,
    y0: u32,
    x1: u32,
    y1: u32,
    sx0: u32,
    sy0: u32,
    sx1: u32,
    sy1: u32,
) {
    let bands = [
        Rect {
            x0: sx0,
            y0: sy0,
            x1: sx1,
            y1: y0,
        },
        Rect {
            x0: sx0,
            y0: y1,
            x1: sx1,
            y1: sy1,
        },
        Rect {
            x0: sx0,
            y0: y0,
            x1: x0,
            y1: y1,
        },
        Rect {
            x0: x1,
            y0: y0,
            x1: sx1,
            y1: y1,
        },
        Rect {
            x0,
            y0,
            x1: x0.saturating_add(RADIUS),
            y1: y0.saturating_add(RADIUS),
        },
        Rect {
            x0: x1.saturating_sub(RADIUS),
            y0,
            x1,
            y1: y0.saturating_add(RADIUS),
        },
        Rect {
            x0,
            y0: y1.saturating_sub(RADIUS),
            x1: x0.saturating_add(RADIUS),
            y1,
        },
        Rect {
            x0: x1.saturating_sub(RADIUS),
            y0: y1.saturating_sub(RADIUS),
            x1,
            y1,
        },
    ];
    for band in bands {
        let vis = rect_intersect(clip, band);
        if rect_is_empty(vis) {
            continue;
        }
        for y in vis.y0..vis.y1 {
            for x in vis.x0..vis.x1 {
                if rect_contains(protect, x, y) {
                    continue;
                }
                shade_px(st, x, y, x0, y0, x1, y1);
            }
        }
    }
}

pub(super) fn paint_vt_except(st: &mut State, clip: Rect, holes: &[Rect]) {
    let Some(vi) = st.frames.iter().position(|f| f.kind == FrameKind::Vt) else {
        return;
    };
    let f = st.frames[vi];
    let dest = rect_intersect(opaque_rect(&f), clip);
    let client = Rect {
        x0: f.ox,
        y0: f.oy.saturating_add(DECO_H),
        x1: f.ox.saturating_add(f.cw),
        y1: f.oy.saturating_add(DECO_H).saturating_add(f.ch),
    };
    let dest = rect_intersect(dest, client);
    if rect_is_empty(dest) {
        return;
    }
    let mut parts = Vec::new();
    parts.push(dest);
    for hole in holes {
        let mut next = Vec::new();
        for p in parts {
            let (subs, n) = rect_sub(p, *hole);
            for k in 0..n {
                if !rect_is_empty(subs[k]) {
                    next.push(subs[k]);
                }
            }
        }
        parts = next;
    }
    let fb = st.scene_fb;
    let cx = f.ox;
    let cy = f.oy.saturating_add(DECO_H);
    let _ = fbterm::with_vt_pixels(|pix, vw, vh| {
        for r in &parts {
            let vis = *r;
            let sx = vis.x0.saturating_sub(cx);
            let sy = vis.y0.saturating_sub(cy);
            let w = vis.x1.saturating_sub(vis.x0).min(vw.saturating_sub(sx));
            let h = vis.y1.saturating_sub(vis.y0).min(vh.saturating_sub(sy));
            if w == 0 || h == 0 {
                continue;
            }
            blit_u32_at_round(
                &fb,
                vis.x0,
                vis.y0,
                pix,
                vw,
                sx,
                sy,
                w,
                h,
                f.ox,
                f.oy,
                f.cw,
                DECO_H.saturating_add(f.ch),
            );
        }
    });
}

pub(super) fn paint_frame_outline(
    st: &mut State,
    i: usize,
    clip: Rect,
    protect: Rect,
    focused: bool,
) {
    let f = st.frames[i];
    if f.maximized {
        return;
    }
    let fh = DECO_H.saturating_add(f.ch);
    let frame_rect = Rect {
        x0: f.ox,
        y0: f.oy,
        x1: f.ox.saturating_add(f.cw),
        y1: f.oy.saturating_add(fh),
    };
    let vis = rect_intersect(frame_rect, clip);
    if rect_is_empty(vis) {
        return;
    }
    let color = if focused { ACCENT } else { BORDER };
    let t = tgt(st.scene_fb);
    let draw = |c: Clip| {
        coeleo_draw::stroke_round(t, f.ox, f.oy, f.cw, fh, RADIUS, color, c);
    };
    deco_clip_draw(vis, protect, draw);
}

pub(super) fn paint_deco_clip(st: &mut State, i: usize, clip: Rect, protect: Rect) {
    let f = st.frames[i];
    let deco = Rect {
        x0: f.ox,
        y0: f.oy,
        x1: f.ox.saturating_add(f.cw),
        y1: f.oy.saturating_add(DECO_H),
    };
    let vis = rect_intersect(deco, clip);
    if rect_is_empty(vis) {
        return;
    }
    let focused = visible_top(st) == Some(i);
    let fh = DECO_H.saturating_add(f.ch);
    let t = tgt(st.scene_fb);
    let dclip = Clip {
        x0: vis.x0,
        y0: vis.y0,
        x1: vis.x1,
        y1: vis.y1,
    };
    if rect_is_empty(protect) {
        coeleo_draw::fill_round(t, f.ox, f.oy, f.cw, fh, RADIUS, SURFACE, dclip);
    } else {
        for y in vis.y0..vis.y1 {
            for x in vis.x0..vis.x1 {
                if rect_contains(protect, x, y) {
                    continue;
                }
                let cov = coeleo_draw::coverage_round(x, y, f.ox, f.oy, f.cw, fh, RADIUS);
                if cov == 0 {
                    continue;
                }
                if cov == 255 {
                    put_fb(&st.scene_fb, x, y, SURFACE.to_le_bytes());
                } else {
                    coeleo_draw::put(
                        t,
                        x,
                        y,
                        coeleo_draw::blend(coeleo_draw::get(t, x, y), SURFACE, cov),
                    );
                }
            }
        }
    }
    let app_ic = frame_icon(f.kind);
    let icon_x = f.ox.saturating_add(PAD);
    let icon_y = f.oy.saturating_add((DECO_H.saturating_sub(coeleo_draw::ICON)) / 2);
    paint_deco_icon(
        st,
        app_ic,
        icon_x,
        icon_y,
        if focused { ACCENT } else { DIM },
        clip,
        protect,
    );

    let title_owned = match f.kind {
        FrameKind::Files => crate::fm::window_title(),
        _ => alloc::string::String::from(frame_title(f.kind)),
    };
    let title_x = icon_x.saturating_add(coeleo_draw::ICON + GAP / 2);
    let text_clip = f.ox.saturating_add(f.cw.saturating_sub(BTN * 3));
    let ty = f.oy.saturating_add(DECO_H.saturating_sub(coeleo_draw::FONT_H) / 2);
    let title_color = if focused { TEXT } else { DIM };
    let max_px = text_clip.saturating_sub(title_x);
    let title = elide_title_px(&title_owned, max_px);
    deco_text(
        st,
        title_x,
        ty,
        &title,
        title_color,
        text_clip,
        clip,
        protect,
    );

    let sep_y = f.oy.saturating_add(DECO_H).saturating_sub(1);
    if sep_y >= vis.y0 && sep_y < vis.y1 {
        let sx0 = f.ox.max(vis.x0);
        let sx1 = f.ox.saturating_add(f.cw).min(vis.x1);
        if sx1 > sx0 {
            coeleo_draw::fill_span(t, sx0, sep_y, sx1.saturating_sub(sx0), BORDER);
        }
    }

    let bx = f.ox.saturating_add(f.cw);
    let by = f.oy.saturating_add(DECO_BTN_PAD);
    let mx = st.cx.max(0) as u32;
    let my = st.cy.max(0) as u32;
    let buttons = [
        bx.saturating_sub(BTN * 3),
        bx.saturating_sub(BTN * 2),
        bx.saturating_sub(BTN),
    ];
    let mut btn_hover = [false; 3];
    for (n, x0) in buttons.iter().copied().enumerate() {
        if mx >= x0 && mx < x0.saturating_add(BTN) && my >= by && my < by.saturating_add(BTN) {
            btn_hover[n] = true;
            if n == 2 {
                coeleo_draw::fill_round(t, x0, by, BTN, BTN, coeleo_theme::RADIUS_SM, DANGER, dclip);
            } else {
                coeleo_draw::fill_round(t, x0, by, BTN, BTN, coeleo_theme::RADIUS_SM, HOVER, dclip);
            }
        }
    }
    let iy = by + (BTN.saturating_sub(coeleo_draw::ICON)) / 2;
    let ix = |x0: u32| x0 + (BTN.saturating_sub(coeleo_draw::ICON)) / 2;
    let min_color = if btn_hover[0] { TEXT } else { title_color };
    let max_color = if btn_hover[1] { TEXT } else { title_color };
    let close_color = if btn_hover[2] { 0x00FF_FFFF } else { title_color };

    paint_deco_icon(
        st,
        coeleo_draw::Icon::Min,
        ix(buttons[0]),
        iy,
        min_color,
        clip,
        protect,
    );
    if f.maximized {
        paint_deco_restore(st, ix(buttons[1]), iy, max_color, clip, protect);
    } else {
        paint_deco_icon(
            st,
            coeleo_draw::Icon::Max,
            ix(buttons[1]),
            iy,
            max_color,
            clip,
            protect,
        );
    }
    paint_deco_icon(
        st,
        coeleo_draw::Icon::Close,
        ix(buttons[2]),
        iy,
        close_color,
        clip,
        protect,
    );
}

fn paint_deco_icon(
    st: &mut State,
    which: coeleo_draw::Icon,
    x: u32,
    y: u32,
    color: u32,
    clip: Rect,
    protect: Rect,
) {
    let t = tgt(st.scene_fb);
    let draw = |c: Clip| {
        coeleo_draw::icon(t, which, x, y, color, c);
    };
    deco_clip_draw(clip, protect, draw);
}

fn paint_deco_restore(st: &mut State, x: u32, y: u32, color: u32, clip: Rect, protect: Rect) {
    let t = tgt(st.scene_fb);
    deco_clip_draw(clip, protect, |c| {
        coeleo_draw::icon_restore(t, x, y, color, c);
    });
}

fn deco_clip_draw(clip: Rect, protect: Rect, mut draw: impl FnMut(Clip)) {
    if rect_is_empty(protect) {
        draw(Clip {
            x0: clip.x0,
            y0: clip.y0,
            x1: clip.x1,
            y1: clip.y1,
        });
        return;
    }
    let (parts, n) = rect_sub(clip, protect);
    for k in 0..n {
        if rect_is_empty(parts[k]) {
            continue;
        }
        draw(Clip {
            x0: parts[k].x0,
            y0: parts[k].y0,
            x1: parts[k].x1,
            y1: parts[k].y1,
        });
    }
}

pub(super) fn put_wall(st: &State, x: u32, y: u32) {
    let c = wall_px(st, x, y);
    put_fb(&st.scene_fb, x, y, c.to_le_bytes());
}

pub(super) fn wall_px(st: &State, x: u32, y: u32) -> u32 {
    if st.wallpaper.is_empty() || st.wall_w == 0 || st.wall_h == 0 {
        return BG;
    }
    let sx = if st.wall_w == st.fb.w {
        x.min(st.wall_w.saturating_sub(1))
    } else {
        x.saturating_mul(st.wall_w) / st.fb.w.max(1)
    };
    let sy = y.min(st.wall_h.saturating_sub(1));
    let src = (sy * st.wall_w + sx) as usize;
    if src < st.wallpaper.len() {
        st.wallpaper[src] & 0x00FF_FFFF
    } else {
        BG
    }
}

fn elide_title_px(s: &str, max_px: u32) -> alloc::string::String {
    if max_px == 0 {
        return alloc::string::String::new();
    }
    if coeleo_draw::text_width(s) <= max_px {
        return alloc::string::String::from(s);
    }
    let dots = coeleo_draw::text_width("...");
    if max_px <= dots {
        return alloc::string::String::from("...");
    }
    let budget = max_px.saturating_sub(dots);
    let mut w = 0u32;
    let mut n = 0usize;
    for c in s.bytes() {
        let a = coeleo_draw::font::advance(c);
        if w.saturating_add(a) > budget {
            break;
        }
        w = w.saturating_add(a);
        n += 1;
    }
    let mut out = alloc::string::String::from(&s[..n.min(s.len())]);
    out.push_str("...");
    out
}

pub(super) fn deco_text(
    st: &mut State,
    x: u32,
    y: u32,
    s: &str,
    color: u32,
    xmax: u32,
    clip: Rect,
    protect: Rect,
) {
    let t = tgt(st.scene_fb);
    let draw = |c: Clip| {
        coeleo_draw::text(t, x, y, s, color, xmax, c);
    };
    if rect_is_empty(protect) {
        draw(Clip {
            x0: clip.x0,
            y0: clip.y0,
            x1: clip.x1,
            y1: clip.y1,
        });
        return;
    }
    let (parts, n) = rect_sub(clip, protect);
    for k in 0..n {
        if rect_is_empty(parts[k]) {
            continue;
        }
        draw(Clip {
            x0: parts[k].x0,
            y0: parts[k].y0,
            x1: parts[k].x1,
            y1: parts[k].y1,
        });
    }
}
pub(super) fn paint_client_clip(
    st: &mut State,
    i: usize,
    clients: &[crate::win::ClientSnap],
    clip: Rect,
    protect: Rect,
) {
    let f = st.frames[i];
    let dest = Rect {
        x0: f.ox,
        y0: f.oy.saturating_add(DECO_H),
        x1: f.ox.saturating_add(f.cw),
        y1: f.oy.saturating_add(DECO_H).saturating_add(f.ch),
    };
    let vis = rect_intersect(dest, clip);
    if rect_is_empty(vis) {
        return;
    }
    match f.kind {
        FrameKind::Vt => {
            fill_vis_bg(st, vis, protect, &f);
            let (parts, n) = if rect_is_empty(protect) {
                ([vis, RECT_EMPTY, RECT_EMPTY, RECT_EMPTY], 1)
            } else {
                rect_sub(vis, protect)
            };
            let fb = st.scene_fb;
            let cx = f.ox;
            let cy = f.oy.saturating_add(DECO_H);
            let fh = DECO_H.saturating_add(f.ch);
            let _ = fbterm::with_vt_pixels(|pix, vw, vh| {
                for k in 0..n {
                    let r = parts[k];
                    if rect_is_empty(r) {
                        continue;
                    }
                    let sx = r.x0.saturating_sub(cx);
                    let sy = r.y0.saturating_sub(cy);
                    let w = r.x1.saturating_sub(r.x0).min(vw.saturating_sub(sx));
                    let h = r.y1.saturating_sub(r.y0).min(vh.saturating_sub(sy));
                    if w == 0 || h == 0 {
                        continue;
                    }
                    blit_u32_at_round(&fb, r.x0, r.y0, pix, vw, sx, sy, w, h, f.ox, f.oy, f.cw, fh);
                }
            });
        }
        FrameKind::Files => {
            let mut sink = FilesSink {
                fb: st.scene_fb,
                ox: f.ox,
                oy: f.oy.saturating_add(DECO_H),
                w: f.cw,
                h: f.ch,
                wx: f.ox,
                wy: f.oy,
                ww: f.cw,
                wh: DECO_H.saturating_add(f.ch),
                clip: vis,
                protect,
            };
            fm::render(f.cw, f.ch, &mut sink);
        }
        FrameKind::Settings => {
            let mut sink = FilesSink {
                fb: st.scene_fb,
                ox: f.ox,
                oy: f.oy.saturating_add(DECO_H),
                w: f.cw,
                h: f.ch,
                wx: f.ox,
                wy: f.oy,
                ww: f.cw,
                wh: DECO_H.saturating_add(f.ch),
                clip: vis,
                protect,
            };
            crate::deskset::render(f.cw, f.ch, &mut sink);
        }
        FrameKind::Client(id) => {
            fill_vis_bg(st, vis, protect, &f);
            let Some(s) = clients.iter().find(|c| c.id == id) else {
                return;
            };
            let (parts, n) = if rect_is_empty(protect) {
                ([vis, RECT_EMPTY, RECT_EMPTY, RECT_EMPTY], 1)
            } else {
                rect_sub(vis, protect)
            };
            let cx = f.ox;
            let cy = f.oy.saturating_add(DECO_H);
            let fh = DECO_H.saturating_add(f.ch);
            for k in 0..n {
                let r = parts[k];
                if rect_is_empty(r) {
                    continue;
                }
                let sx = r.x0.saturating_sub(cx);
                let sy = r.y0.saturating_sub(cy);
                let w = r.x1.saturating_sub(r.x0).min(s.w.saturating_sub(sx));
                let h = r.y1.saturating_sub(r.y0).min(s.h.saturating_sub(sy));
                if w == 0 || h == 0 {
                    continue;
                }
                blit_u32_at_round(
                    &st.scene_fb,
                    r.x0,
                    r.y0,
                    &s.pixels,
                    s.w,
                    sx,
                    sy,
                    w,
                    h,
                    f.ox,
                    f.oy,
                    f.cw,
                    fh,
                );
            }
        }
    }
}
