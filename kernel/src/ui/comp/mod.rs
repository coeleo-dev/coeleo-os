//! Software compositor: stacked windows, deco, short shadow, dirty cursor.

mod chrome;
mod cursor;
mod damage;
mod fb;
mod files;
mod frames;
mod geom;
mod log;
mod paint;
mod rect;
mod state;

use alloc::vec::Vec;
use core::sync::atomic::Ordering;

use crate::fbterm::{self, FbInfo};
use crate::fm;
use crate::panel;
use crate::serial;
use coeleo_theme::{BG, DECO_BTN_PAD, DECO_H};

use chrome::{
    close_desk_menu, close_files_menu, close_launcher, close_runner, confirm_key,
    desk_menu_activate, files_menu_activate, handle_confirm_click, handle_desk_menu_click,
    handle_files_menu_click, handle_panel_click, launch_index, open_desk_menu, open_files_menu,
    open_runner, panel_hit, present_files_menu, present_krunner, present_launcher, runner_enter,
    runner_launch, runner_refresh, update_hover,
};
use cursor::{CURSOR_H, CURSOR_W, draw_cursor, undraw_cursor};
use damage::note;
use frames::{
    apply_drag, apply_panel_mode, close_frame, focus_frame, hit_test, minimize_frame,
    raise_visible, retarget_focus, set_focus, sync_client_top, toggle_max,
};
use geom::{
    ConfirmHit, confirm_hit, files_menu_row_at, frame_new, h_work, krunner_popup, launcher_popup,
    launcher_row_at, launcher_sel_count, opaque_rect, shadow_bounds, shadow_rect, visible_top,
};
use log::{log_blit, log_cursor, log_panel, log_panel_mode};
use paint::{
    fill_rect_r, fill_work, paint_all_frames, paint_clock, paint_shadow_clip, paint_strut,
    paint_vt_except, present_damage,
};
use rect::{
    RECT_EMPTY, Rect, rect_contains, rect_intersect, rect_intersects, rect_is_empty, rect_union,
};
use state::{
    BTN, CLIENT_TOP, DAMAGE_CAP, DESK_MENU_OPEN, Drag, FILES_MENU_OPEN, FOCUS_DESK, FOCUS_FILES,
    Focus, Frame, FrameKind, Hit, KEY_BACK, KEY_CAP, KEY_DEL, KEY_DOWN, KEY_ENTER, KEY_ESC,
    KEY_HEAD, KEY_LEFT, KEY_Q, KEY_RIGHT, KEY_RUNNER, KEY_TAB, KEY_TAIL, KEY_UP, LAUNCHER_OPEN,
    POWER_OPEN, PowerHover, READY, RUNNER_OPEN, STATE, State,
};

pub enum FilesKey {
    Up,
    Down,
    Enter,
    Backspace,
    Left,
    Right,
    Delete,
}

pub fn init() {
    crate::splash::stop();
    let Some(fb) = fbterm::info() else {
        return;
    };
    if fb.bpp != 32 || fb.w < 480 || fb.h < 200 {
        return;
    }
    fm::init();
    if let Some(cfg) = crate::desk::load_cfg() {
        let _ = panel::set_mode(cfg.mode);
        crate::desk::set_wall_src(cfg.wall);
    }
    let work = panel::work_h(fb.h);
    if work < 64 {
        return;
    }
    let frames = Vec::new();
    let seq = 0u32;
    let n = (fb.w as usize).saturating_mul(fb.h as usize);
    if n == 0 {
        return;
    }
    let mut scene = Vec::new();
    if scene.try_reserve(n).is_err() {
        return;
    }
    scene.resize(n, BG);
    let scene_fb = FbInfo {
        addr: scene.as_mut_ptr() as usize,
        w: fb.w,
        h: fb.h,
        pitch: fb.w.saturating_mul(4),
        bpp: 32,
        split: 0,
    };
    let mut wallpaper = Vec::new();
    let mut wall_w = 0u32;
    let mut wall_h = 0u32;
    match crate::desk::load_current(fb.w, work) {
        Some(w) => {
            wall_w = w.w;
            wall_h = w.h;
            wallpaper = w.pix;
            serial::write_str("desk: wallpaper\n");
        }
        None => serial::write_str("desk: wallpaper fail\n"),
    }
    let (menu_iw, menu_ih, menu_icon) = crate::desk::load_menu_icon().unwrap_or((0, 0, Vec::new()));
    let mut st = State {
        fb,
        scene,
        scene_fb,
        pending: [RECT_EMPTY; DAMAGE_CAP],
        n_pending: 0,
        cursor_old: RECT_EMPTY,
        cx: 56,
        cy: 56,
        drawn: false,
        focus: Focus::Term,
        frames,
        drag: None,
        drag_at: 0,
        drag_shadow_logged: false,
        cursor_log_at: u64::MAX,
        hover: panel::Hit::None,
        pressed: panel::Hit::None,
        launcher_open: false,
        runner_open: false,
        desk_menu_open: false,
        desk_menu_x: 0,
        desk_menu_y: 0,
        files_menu_open: false,
        files_menu_x: 0,
        files_menu_y: 0,
        files_menu_sel: 0,
        btn_held: false,
        launch_sel: 0,
        launch_list: Vec::new(),
        runner_query: [0; 24],
        runner_query_len: 0,
        runner_sel: 0,
        runner_list: Vec::new(),
        panel_opaque: false,
        seq_next: seq,
        wallpaper,
        wall_w,
        wall_h,
        menu_icon,
        menu_iw,
        menu_ih,
        tip: panel::Tip::empty(),
        power_dlg: None,
        power_ok: false,
        power_hover: PowerHover::None,
        deco_hover: None,
        deco_hover_pill: false,
        runner_caret: true,
        title_click_at: 0,
        title_click_kind: None,
    };
    fill_work(&mut st);
    paint_all_frames(&mut st, true);
    paint_strut(&mut st);
    draw_cursor(&mut st);
    serial::write_str("wm: windows\n");
    log_panel();
    log_panel_mode(false);
    log_cursor(st.cx, st.cy);
    FOCUS_FILES.store(false, Ordering::Release);
    FOCUS_DESK.store(false, Ordering::Release);
    *STATE.lock() = Some(st);
    READY.store(true, Ordering::Release);
}

pub fn irq_tab() -> bool {
    if RUNNER_OPEN.load(Ordering::Acquire) {
        return true;
    }
    if LAUNCHER_OPEN.load(Ordering::Acquire)
        || POWER_OPEN.load(Ordering::Acquire)
        || DESK_MENU_OPEN.load(Ordering::Acquire)
        || FILES_MENU_OPEN.load(Ordering::Acquire)
        || client_on_top()
        || FOCUS_FILES.load(Ordering::Acquire)
        || FOCUS_DESK.load(Ordering::Acquire)
    {
        push_key(KEY_TAB);
        return true;
    }
    false
}

fn client_on_top() -> bool {
    CLIENT_TOP.load(Ordering::Acquire)
}

pub fn irq_esc() -> bool {
    if !LAUNCHER_OPEN.load(Ordering::Acquire)
        && !RUNNER_OPEN.load(Ordering::Acquire)
        && !POWER_OPEN.load(Ordering::Acquire)
        && !DESK_MENU_OPEN.load(Ordering::Acquire)
        && !FILES_MENU_OPEN.load(Ordering::Acquire)
        && !client_on_top()
        && !FOCUS_FILES.load(Ordering::Acquire)
    {
        return false;
    }
    push_key(KEY_ESC);
    true
}

pub fn irq_krunner() {
    push_key(KEY_RUNNER);
}

pub fn irq_runner_char(b: u8) -> bool {
    if POWER_OPEN.load(Ordering::Acquire) {
        return true;
    }
    if !RUNNER_OPEN.load(Ordering::Acquire) {
        return false;
    }
    if (0x20..=0x7E).contains(&b) {
        push_key(b);
    }
    true
}

pub fn irq_files_char(b: u8) -> bool {
    if POWER_OPEN.load(Ordering::Acquire) {
        return true;
    }
    if RUNNER_OPEN.load(Ordering::Acquire) || LAUNCHER_OPEN.load(Ordering::Acquire) {
        return false;
    }
    if FOCUS_DESK.load(Ordering::Acquire) {
        if (0x20..=0x7E).contains(&b) {
            push_key(b);
            return true;
        }
        return false;
    }
    if !FOCUS_FILES.load(Ordering::Acquire) {
        return false;
    }
    if (0x20..=0x7E).contains(&b) {
        push_key(b);
        return true;
    }
    false
}

pub fn irq_files_key(key: FilesKey) -> bool {
    if POWER_OPEN.load(Ordering::Acquire) {
        let code = match key {
            FilesKey::Up | FilesKey::Down => return true,
            FilesKey::Enter => KEY_ENTER,
            FilesKey::Backspace => KEY_ESC,
            FilesKey::Left => KEY_LEFT,
            FilesKey::Right => KEY_RIGHT,
            FilesKey::Delete => return true,
        };
        push_key(code);
        return true;
    }
    if RUNNER_OPEN.load(Ordering::Acquire) {
        let code = match key {
            FilesKey::Up => KEY_UP,
            FilesKey::Down => KEY_DOWN,
            FilesKey::Enter => KEY_ENTER,
            FilesKey::Backspace => KEY_BACK,
            FilesKey::Left | FilesKey::Right | FilesKey::Delete => return true,
        };
        push_key(code);
        return true;
    }
    if LAUNCHER_OPEN.load(Ordering::Acquire) {
        let code = match key {
            FilesKey::Up => KEY_UP,
            FilesKey::Down => KEY_DOWN,
            FilesKey::Enter => KEY_ENTER,
            FilesKey::Backspace => KEY_ESC,
            FilesKey::Left | FilesKey::Right | FilesKey::Delete => return true,
        };
        push_key(code);
        return true;
    }
    if DESK_MENU_OPEN.load(Ordering::Acquire) {
        let code = match key {
            FilesKey::Enter => KEY_ENTER,
            FilesKey::Backspace => KEY_ESC,
            FilesKey::Up | FilesKey::Down | FilesKey::Left | FilesKey::Right | FilesKey::Delete => {
                return true;
            }
        };
        push_key(code);
        return true;
    }
    if FILES_MENU_OPEN.load(Ordering::Acquire) {
        let code = match key {
            FilesKey::Up => KEY_UP,
            FilesKey::Down => KEY_DOWN,
            FilesKey::Enter => KEY_ENTER,
            FilesKey::Backspace => KEY_ESC,
            FilesKey::Left | FilesKey::Right | FilesKey::Delete => return true,
        };
        push_key(code);
        return true;
    }
    if client_on_top() {
        let code = match key {
            FilesKey::Up => KEY_UP,
            FilesKey::Down => KEY_DOWN,
            FilesKey::Enter => KEY_ENTER,
            FilesKey::Backspace => KEY_BACK,
            FilesKey::Left => KEY_LEFT,
            FilesKey::Right => KEY_RIGHT,
            FilesKey::Delete => return true,
        };
        push_key(code);
        return true;
    }
    if FOCUS_DESK.load(Ordering::Acquire) {
        let code = match key {
            FilesKey::Up => KEY_UP,
            FilesKey::Down => KEY_DOWN,
            FilesKey::Enter => KEY_ENTER,
            FilesKey::Backspace => KEY_BACK,
            FilesKey::Left => KEY_LEFT,
            FilesKey::Right => KEY_RIGHT,
            FilesKey::Delete => return true,
        };
        push_key(code);
        return true;
    }
    if FOCUS_FILES.load(Ordering::Acquire) {
        if crate::fm::wants_arrows() {
            let code = match key {
                FilesKey::Up => KEY_UP,
                FilesKey::Down => KEY_DOWN,
                FilesKey::Enter => KEY_ENTER,
                FilesKey::Backspace => KEY_BACK,
                FilesKey::Left => KEY_LEFT,
                FilesKey::Right => KEY_RIGHT,
                FilesKey::Delete => KEY_DEL,
            };
            push_key(code);
            return true;
        }
        let code = match key {
            FilesKey::Up => KEY_UP,
            FilesKey::Down => KEY_DOWN,
            FilesKey::Enter => KEY_ENTER,
            FilesKey::Backspace => KEY_BACK,
            FilesKey::Delete => KEY_DEL,
            FilesKey::Left | FilesKey::Right => return false,
        };
        push_key(code);
        return true;
    }
    false
}

pub fn keys_empty() -> bool {
    KEY_HEAD.load(Ordering::Acquire) == KEY_TAIL.load(Ordering::Acquire)
}

pub fn files_focused() -> bool {
    FOCUS_FILES.load(Ordering::Acquire)
}

pub fn is_ready() -> bool {
    READY.load(Ordering::Acquire)
}

/// Flanterm wrote the canvas. `clip_canvas` is x,y,w,h on the VT buffer;
/// `None` means the whole client.
pub fn damage_vt(clip_canvas: Option<(u32, u32, u32, u32)>) {
    if !READY.load(Ordering::Acquire) {
        return;
    }
    let mut g = STATE.lock();
    let Some(st) = g.as_mut() else {
        return;
    };
    let Some(vi) = st.frames.iter().position(|f| f.kind == FrameKind::Vt) else {
        return;
    };
    if st.frames[vi].minimized {
        return;
    }
    let f = st.frames[vi];
    let client = Rect {
        x0: f.ox,
        y0: f.oy.saturating_add(DECO_H),
        x1: f.ox.saturating_add(f.cw),
        y1: f.oy.saturating_add(DECO_H).saturating_add(f.ch),
    };
    let clip = match clip_canvas {
        None => client,
        Some((x, y, w, h)) => rect_intersect(
            client,
            Rect {
                x0: f.ox.saturating_add(x),
                y0: f.oy.saturating_add(DECO_H).saturating_add(y),
                x1: f.ox.saturating_add(x).saturating_add(w),
                y1: f
                    .oy
                    .saturating_add(DECO_H)
                    .saturating_add(y)
                    .saturating_add(h),
            },
        ),
    };
    if rect_is_empty(clip) {
        return;
    }
    let mut holes = [RECT_EMPTY; 4];
    let mut nh = 0usize;
    for i in vi + 1..st.frames.len() {
        if st.frames[i].minimized {
            continue;
        }
        if nh < holes.len() {
            holes[nh] = opaque_rect(&st.frames[i]);
            nh += 1;
        }
    }
    undraw_cursor(st);
    paint_vt_except(st, clip, &holes[..nh]);
    for i in vi + 1..st.frames.len() {
        if st.frames[i].minimized {
            continue;
        }
        if rect_intersects(shadow_rect(&st.frames[i]), clip) {
            paint_shadow_clip(st, i, clip, RECT_EMPTY);
        }
    }
    note(st, clip);
    draw_cursor(st);
}

pub fn on_client_create(id: u32, cw: u32, ch: u32, ox: u32, oy: u32) {
    if !READY.load(Ordering::Acquire) {
        return;
    }
    let mut g = STATE.lock();
    let Some(st) = g.as_mut() else {
        return;
    };
    let seq = st.seq_next;
    st.seq_next = st.seq_next.saturating_add(1);
    st.frames
        .push(frame_new(FrameKind::Client(id), ox, oy, cw, ch, seq));
    set_focus(st, Focus::Term);
    sync_client_top(st);
    undraw_cursor(st);
    present_damage(st, shadow_rect(&st.frames[st.frames.len() - 1]), true);
    paint_strut(st);
    draw_cursor(st);
}

pub fn on_client_close(id: u32) {
    if !READY.load(Ordering::Acquire) {
        return;
    }
    let mut g = STATE.lock();
    let Some(st) = g.as_mut() else {
        return;
    };
    let Some(i) = st
        .frames
        .iter()
        .position(|f| f.kind == FrameKind::Client(id))
    else {
        return;
    };
    let old = shadow_bounds(&st.frames[i]);
    st.frames.remove(i);
    if st
        .drag
        .as_ref()
        .is_some_and(|d| d.kind == FrameKind::Client(id))
    {
        st.drag = None;
    }
    retarget_focus(st);
    undraw_cursor(st);
    fill_rect_r(
        st,
        Rect {
            x0: old.0,
            y0: old.1,
            x1: old.2,
            y1: old.3,
        },
    );
    present_damage(
        st,
        Rect {
            x0: old.0,
            y0: old.1,
            x1: old.2,
            y1: old.3,
        },
        true,
    );
    paint_strut(st);
    draw_cursor(st);
}

fn push_key(k: u8) {
    let h = KEY_HEAD.load(Ordering::Relaxed);
    let next = (h + 1) % KEY_CAP;
    if next == KEY_TAIL.load(Ordering::Acquire) {
        return;
    }
    unsafe {
        KEY_Q[h] = k;
    }
    KEY_HEAD.store(next, Ordering::Release);
}

fn pop_key() -> Option<u8> {
    let t = KEY_TAIL.load(Ordering::Relaxed);
    let h = KEY_HEAD.load(Ordering::Acquire);
    if t == h {
        return None;
    }
    let k = unsafe { KEY_Q[t] };
    KEY_TAIL.store((t + 1) % KEY_CAP, Ordering::Release);
    Some(k)
}

pub fn poll() {
    if !READY.load(Ordering::Acquire) {
        return;
    }
    crate::sched::reap_orphans();
    while let Some(k) = pop_key() {
        apply_key(k);
    }
    present_windows();
    {
        let mut g = STATE.lock();
        if let Some(st) = g.as_mut() {
            if tick_runner_caret(st) {
                undraw_cursor(st);
                draw_cursor(st);
            }
        }
    }
    let Some(ev) = crate::mouse::drain() else {
        return;
    };
    let mut g = STATE.lock();
    let Some(st) = g.as_mut() else {
        return;
    };
    undraw_cursor(st);
    if ev.dx != 0 || ev.dy != 0 {
        st.cx = (st.cx + i32::from(ev.dx)).clamp(0, st.fb.w.saturating_sub(1) as i32);
        st.cy = (st.cy + i32::from(ev.dy)).clamp(0, st.fb.h.saturating_sub(1) as i32);
        let t = crate::clock::ticks();
        if st.cursor_log_at == u64::MAX || t.saturating_sub(st.cursor_log_at) >= 50 {
            log_cursor(st.cx, st.cy);
            log_blit(CURSOR_W, CURSOR_H);
            st.cursor_log_at = t;
        }
    }
    if st.drag.is_some() && (ev.dx != 0 || ev.dy != 0) {
        let t = crate::clock::ticks();
        if st.drag_at == 0 || t.saturating_sub(st.drag_at) >= 2 {
            st.drag_at = t.max(1);
            apply_drag(st);
        }
    }
    if ev.left_up && !ev.left_down {
        st.btn_held = false;
    }
    let click = ev.left_down && !st.btn_held;
    if ev.left_down {
        st.btn_held = true;
    }
    let x = st.cx as u32;
    let y = st.cy as u32;
    let work = h_work(st);
    if st.drag.is_none() && (ev.dx != 0 || ev.dy != 0) {
        update_hover(st, x, y);
        update_deco_hover(st, x, y);
        if st.power_dlg.is_some() {
            update_power_hover(st, x, y);
        }
        if st.launcher_open {
            if let Some(row) = launcher_row_at(st, x, y) {
                if row != st.launch_sel {
                    st.launch_sel = row;
                    present_launcher(st);
                }
            }
        }
        if st.runner_open {
            if let crate::krunner::Hit::Row(i) =
                crate::krunner::hit(krunner_popup(st), x, y, st.runner_list.len())
            {
                if i != st.runner_sel {
                    st.runner_sel = i;
                    present_krunner(st);
                }
            }
        }
        if st.files_menu_open {
            if let Some(row) = files_menu_row_at(st, x, y) {
                if row != st.files_menu_sel {
                    st.files_menu_sel = row;
                    present_files_menu(st);
                }
            }
        }
        if y < work {
            if let Some(Hit::Client(i)) = hit_test(st, x, y) {
                match st.frames[i].kind {
                    FrameKind::Files => {
                        let f = st.frames[i];
                        let lx = x.saturating_sub(f.ox);
                        let ly = y.saturating_sub(f.oy.saturating_add(DECO_H));
                        if let Some((old, new)) = fm::hover_at(lx, ly, f.cw) {
                            for part in [old, new].into_iter().flatten() {
                                let r = files_hover_rect(&f, part);
                                if !rect_is_empty(r) {
                                    present_damage(st, r, false);
                                }
                            }
                        }
                    }
                    FrameKind::Client(id) => {
                        let f = st.frames[i];
                        let lx = x as i32 - f.ox as i32;
                        let ly = y as i32 - f.oy as i32 - DECO_H as i32;
                        crate::win::push_client_move(id, lx, ly);
                    }
                    FrameKind::Vt | FrameKind::Settings => {}
                }
            }
        }
    }
    if ev.right_down && st.drag.is_none() {
        handle_right_click(st, x, y, work);
        draw_cursor(st);
        return;
    }
    if click {
        if st.power_dlg.is_some() {
            handle_confirm_click(st, x, y);
            draw_cursor(st);
            return;
        }
        if st.desk_menu_open {
            let _ = handle_desk_menu_click(st, x, y);
            draw_cursor(st);
            return;
        }
        if st.files_menu_open {
            let _ = handle_files_menu_click(st, x, y);
            draw_cursor(st);
            return;
        }
        if st.runner_open {
            match crate::krunner::hit(krunner_popup(st), x, y, st.runner_list.len()) {
                crate::krunner::Hit::Row(i) => {
                    runner_launch(st, i);
                    draw_cursor(st);
                    return;
                }
                crate::krunner::Hit::Query => {
                    draw_cursor(st);
                    return;
                }
                crate::krunner::Hit::Outside => {
                    if y < work {
                        close_runner(st);
                    }
                }
            }
        }
        if st.launcher_open {
            if let Some(row) = launcher_row_at(st, x, y) {
                launch_index(st, row);
                draw_cursor(st);
                return;
            }
            if y < work
                && launcher_row_at(st, x, y).is_none()
                && !rect_contains(launcher_popup(st), x, y)
            {
                close_launcher(st);
            }
        }
        let ph = panel_hit(st, x, y);
        if ph != panel::Hit::None || y >= work {
            if st.drag.is_none() {
                handle_panel_click(st, x, y);
            }
        } else if let Some(hit) = hit_test(st, x, y) {
            match hit {
                Hit::Close(i) => match st.frames[i].kind {
                    FrameKind::Client(id) => {
                        drop(g);
                        crate::win::drop_id(id);
                        on_client_close(id);
                        return;
                    }
                    FrameKind::Vt | FrameKind::Files | FrameKind::Settings => close_frame(st, i),
                },
                Hit::Min(i) => minimize_frame(st, i),
                Hit::Max(i) => toggle_max(st, i),
                Hit::Title(i) => {
                    raise_visible(st, i);
                    let i = st.frames.len() - 1;
                    let t = crate::clock::ticks();
                    let kind = st.frames[i].kind;
                    let dbl = st.title_click_kind == Some(kind)
                        && t.saturating_sub(st.title_click_at) < 40;
                    if dbl {
                        st.title_click_kind = None;
                        toggle_max(st, i);
                    } else {
                        st.title_click_at = t;
                        st.title_click_kind = Some(kind);
                        let f = st.frames[i];
                        if !f.maximized {
                            st.drag_at = 0;
                            st.drag_shadow_logged = false;
                            st.drag = Some(Drag {
                                kind: f.kind,
                                gx: st.cx - f.ox as i32,
                                gy: st.cy - f.oy as i32,
                                edge: None,
                                ox0: f.ox,
                                oy0: f.oy,
                                cw0: f.cw,
                                ch0: f.ch,
                            });
                        }
                    }
                    present_damage(st, shadow_rect(&st.frames[i]), false);
                    paint_strut(st);
                }
                Hit::Resize(i, edge) => {
                    raise_visible(st, i);
                    let f = st.frames[st.frames.len() - 1];
                    if !f.maximized {
                        st.drag_at = 0;
                        st.drag_shadow_logged = false;
                        st.drag = Some(Drag {
                            kind: f.kind,
                            gx: st.cx,
                            gy: st.cy,
                            edge: Some(edge),
                            ox0: f.ox,
                            oy0: f.oy,
                            cw0: f.cw,
                            ch0: f.ch,
                        });
                    }
                    present_damage(st, shadow_rect(&st.frames[st.frames.len() - 1]), false);
                    paint_strut(st);
                }
                Hit::Client(i) => {
                    raise_visible(st, i);
                    match st.frames[st.frames.len() - 1].kind {
                        FrameKind::Vt => {
                            set_focus(st, Focus::Term);
                            present_damage(st, shadow_rect(&st.frames[st.frames.len() - 1]), false);
                            paint_strut(st);
                        }
                        FrameKind::Files => {
                            set_focus(st, Focus::Files);
                            let last = st.frames[st.frames.len() - 1];
                            let lx = x.saturating_sub(last.ox);
                            let ly = y.saturating_sub(last.oy.saturating_add(DECO_H));
                            drop(g);
                            fm::click_at(lx, ly, last.cw);
                            let mut g = STATE.lock();
                            if let Some(st) = g.as_mut() {
                                undraw_cursor(st);
                                present_damage(
                                    st,
                                    shadow_rect(&st.frames[st.frames.len() - 1]),
                                    false,
                                );
                                paint_strut(st);
                                draw_cursor(st);
                            }
                            return;
                        }
                        FrameKind::Settings => {
                            set_focus(st, Focus::Desk);
                            let last = st.frames[st.frames.len() - 1];
                            let lx = x.saturating_sub(last.ox);
                            let ly = y.saturating_sub(last.oy.saturating_add(DECO_H));
                            drop(g);
                            let a = crate::deskset::click_at(lx, ly, last.cw);
                            apply_desk_action(a);
                            refresh_settings();
                            return;
                        }
                        FrameKind::Client(id) => {
                            let last = &st.frames[st.frames.len() - 1];
                            let lx = x as i32 - last.ox as i32;
                            let ly = y as i32 - last.oy as i32 - DECO_H as i32;
                            crate::win::push_client_down(id, lx, ly);
                            present_damage(st, shadow_rect(&st.frames[st.frames.len() - 1]), false);
                            paint_strut(st);
                        }
                    }
                }
            }
        }
    }
    if ev.left_up {
        if let Some(d) = st.drag {
            apply_drag(st);
            if d.edge.is_some() {
                serial::write_str("wm: resize\n");
            }
        }
        st.drag = None;
        st.drag_at = 0;
        if st.pressed != panel::Hit::None {
            st.pressed = panel::Hit::None;
            paint_strut(st);
        }
    }
    draw_cursor(st);
}

fn handle_right_click(st: &mut State, x: u32, y: u32, work: u32) {
    if st.power_dlg.is_some() {
        return;
    }
    if st.files_menu_open {
        close_files_menu(st);
        return;
    }
    if st.desk_menu_open {
        close_desk_menu(st);
        return;
    }
    if st.launcher_open {
        close_launcher(st);
        return;
    }
    if st.runner_open {
        close_runner(st);
        return;
    }
    if y >= work {
        return;
    }
    if let Some(Hit::Client(i)) = hit_test(st, x, y) {
        if st.frames[i].kind == FrameKind::Files {
            raise_visible(st, i);
            set_focus(st, Focus::Files);
            let f = st.frames[st.frames.len() - 1];
            let lx = x.saturating_sub(f.ox);
            let ly = y.saturating_sub(f.oy.saturating_add(DECO_H));
            if fm::right_click_at(lx, ly, f.cw) {
                open_files_menu(st, x, y);
            }
        }
        return;
    }
    if hit_test(st, x, y).is_some() {
        return;
    }
    open_desk_menu(st, x, y);
}

fn apply_key(k: u8) {
    let mut g = STATE.lock();
    let Some(st) = g.as_mut() else {
        return;
    };
    if k == KEY_RUNNER {
        if st.power_dlg.is_some() {
            draw_cursor(st);
            return;
        }
        if st.runner_open {
            close_runner(st);
        } else {
            open_runner(st);
        }
        draw_cursor(st);
        return;
    }
    if st.power_dlg.is_some() {
        confirm_key(st, k);
        draw_cursor(st);
        return;
    }
    if st.runner_open {
        match k {
            KEY_ESC => close_runner(st),
            KEY_UP => {
                if st.runner_sel > 0 {
                    st.runner_sel -= 1;
                    present_krunner(st);
                }
            }
            KEY_DOWN => {
                if !st.runner_list.is_empty() && st.runner_sel + 1 < st.runner_list.len() {
                    st.runner_sel += 1;
                    present_krunner(st);
                }
            }
            KEY_ENTER => runner_enter(st),
            KEY_BACK => {
                if st.runner_query_len > 0 {
                    st.runner_query_len -= 1;
                    runner_refresh(st);
                }
            }
            c if c >= 32 => {
                if st.runner_query_len < 24 {
                    st.runner_query[st.runner_query_len] = c;
                    st.runner_query_len += 1;
                    runner_refresh(st);
                }
            }
            _ => {}
        }
        draw_cursor(st);
        return;
    }
    if st.launcher_open {
        match k {
            KEY_ESC => close_launcher(st),
            KEY_TAB => {
                close_launcher(st);
                drop(g);
                apply_key_tab();
                return;
            }
            KEY_UP => {
                if st.launch_sel > 0 {
                    st.launch_sel -= 1;
                    present_launcher(st);
                }
            }
            KEY_DOWN => {
                if st.launch_sel + 1 < launcher_sel_count(st) {
                    st.launch_sel += 1;
                    present_launcher(st);
                }
            }
            KEY_ENTER => {
                let row = st.launch_sel;
                launch_index(st, row);
            }
            _ => {}
        }
        draw_cursor(st);
        return;
    }
    if st.desk_menu_open {
        match k {
            KEY_ESC => close_desk_menu(st),
            KEY_ENTER => desk_menu_activate(st),
            _ => {}
        }
        draw_cursor(st);
        return;
    }
    if st.files_menu_open {
        match k {
            KEY_ESC => close_files_menu(st),
            KEY_ENTER => files_menu_activate(st),
            KEY_UP => {
                if st.files_menu_sel > 0 {
                    st.files_menu_sel -= 1;
                    present_files_menu(st);
                }
            }
            KEY_DOWN => {
                if st.files_menu_sel + 1 < 3 {
                    st.files_menu_sel += 1;
                    present_files_menu(st);
                }
            }
            _ => {}
        }
        draw_cursor(st);
        return;
    }
    let top_kind = visible_top(st).map(|i| st.frames[i].kind);
    let client_id = match top_kind {
        Some(FrameKind::Client(id)) => Some(id),
        _ => None,
    };
    drop(g);
    if let Some(id) = client_id {
        match k {
            KEY_TAB => apply_key_tab(),
            KEY_ESC | KEY_ENTER | KEY_LEFT | KEY_RIGHT | KEY_UP | KEY_DOWN | KEY_BACK => {
                crate::win::push_client_key(id, k);
            }
            c if c >= 32 => {
                crate::win::push_client_key(id, c);
            }
            _ => {}
        }
        return;
    }
    if matches!(top_kind, Some(FrameKind::Settings)) {
        if k == KEY_TAB {
            apply_key_tab();
            return;
        }
        let a = crate::deskset::key(k);
        apply_desk_action(a);
        refresh_settings();
        return;
    }
    match k {
        KEY_TAB => apply_key_tab(),
        KEY_LEFT => {
            fm::left();
            refresh_files();
        }
        KEY_RIGHT => {
            fm::right();
            refresh_files();
        }
        KEY_UP | KEY_DOWN | KEY_ENTER | KEY_BACK | KEY_ESC | KEY_DEL => {
            match k {
                KEY_UP => fm::up(),
                KEY_DOWN => fm::down(),
                KEY_ENTER => fm::enter(),
                KEY_BACK => fm::back(),
                KEY_ESC => fm::esc(),
                KEY_DEL => fm::delete_sel(),
                _ => {}
            }
            refresh_files();
        }
        c if c >= 32 => {
            fm::search_push(c);
            refresh_files();
        }
        _ => {}
    }
}

fn refresh_files() {
    let mut g = STATE.lock();
    if let Some(st) = g.as_mut() {
        undraw_cursor(st);
        if let Some(f) = st
            .frames
            .iter()
            .find(|f| f.kind == FrameKind::Files && !f.minimized)
        {
            present_damage(st, shadow_rect(f), false);
        }
        paint_strut(st);
        draw_cursor(st);
    }
}

fn persist_desk() {
    crate::desk::save_cfg(&crate::desk::DeskCfg {
        mode: panel::mode(),
        wall: crate::desk::wall_src(),
    });
}

fn apply_desk_action(a: crate::deskset::Action) {
    match a {
        crate::deskset::Action::None => {}
        crate::deskset::Action::SetMode(m) => {
            let mut g = STATE.lock();
            if let Some(st) = g.as_mut() {
                undraw_cursor(st);
                apply_panel_mode(st, m);
                persist_desk();
                draw_cursor(st);
            }
        }
        crate::deskset::Action::SetWallDefault => apply_wall(crate::desk::WallSrc::Default),
        crate::deskset::Action::SetWallPath(p) => apply_wall(crate::desk::WallSrc::Path(p)),
    }
}

fn apply_wall(src: crate::desk::WallSrc) {
    let prev = crate::desk::wall_src();
    crate::desk::set_wall_src(src);
    let mut g = STATE.lock();
    let Some(st) = g.as_mut() else {
        return;
    };
    undraw_cursor(st);
    let hw = h_work(st);
    match crate::desk::load_current(st.fb.w, hw) {
        Some(w) => {
            st.wallpaper = w.pix;
            st.wall_w = w.w;
            st.wall_h = w.h;
            serial::write_str("desk: wallpaper\n");
            fill_work(st);
            paint_all_frames(st, false);
            paint_strut(st);
            drop(g);
            persist_desk();
        }
        None => {
            crate::desk::set_wall_src(prev);
            serial::write_str("desk: wallpaper fail\n");
            drop(g);
        }
    }
    let mut g = STATE.lock();
    if let Some(st) = g.as_mut() {
        draw_cursor(st);
    }
}

fn refresh_settings() {
    let mut g = STATE.lock();
    if let Some(st) = g.as_mut() {
        undraw_cursor(st);
        if let Some(f) = st
            .frames
            .iter()
            .find(|f| f.kind == FrameKind::Settings && !f.minimized)
        {
            present_damage(st, shadow_rect(f), false);
        }
        paint_strut(st);
        draw_cursor(st);
    }
}

fn apply_key_tab() {
    let mut g = STATE.lock();
    let Some(st) = g.as_mut() else {
        return;
    };
    if st.frames.is_empty() {
        set_focus(st, Focus::Term);
        return;
    }
    undraw_cursor(st);
    let cur = visible_top(st).unwrap_or(0);
    let n = st.frames.len();
    let next = (cur + 1) % n;
    st.frames[next].minimized = false;
    raise_visible(st, next);
    focus_frame(st, st.frames.len() - 1);
    let clip = shadow_rect(&st.frames[st.frames.len() - 1]);
    present_damage(st, clip, false);
    paint_strut(st);
    draw_cursor(st);
}

pub fn undraw() {
    if !READY.load(Ordering::Acquire) {
        return;
    }
    let mut g = STATE.lock();
    if let Some(st) = g.as_mut() {
        undraw_cursor(st);
    }
}

pub fn redraw() {
    if !READY.load(Ordering::Acquire) {
        return;
    }
    let mut g = STATE.lock();
    if let Some(st) = g.as_mut() {
        draw_cursor(st);
    }
}

fn tick_runner_caret(st: &mut State) -> bool {
    if !st.runner_open {
        return false;
    }
    let on = (crate::clock::ticks() / 50) % 2 == 0;
    if st.runner_caret == on {
        return false;
    }
    st.runner_caret = on;
    present_krunner(st);
    true
}

fn deco_hit(h: Option<Hit>) -> Option<Hit> {
    match h {
        Some(Hit::Min(_) | Hit::Max(_) | Hit::Close(_) | Hit::Title(_)) => h,
        _ => None,
    }
}

fn deco_frame(h: Hit) -> Option<usize> {
    match h {
        Hit::Min(i) | Hit::Max(i) | Hit::Close(i) | Hit::Title(i) => Some(i),
        _ => None,
    }
}

fn deco_bar(st: &State, i: usize) -> Rect {
    let f = st.frames[i];
    Rect {
        x0: f.ox,
        y0: f.oy,
        x1: f.ox.saturating_add(f.cw),
        y1: f.oy.saturating_add(DECO_H),
    }
}

fn deco_pill_at(st: &State, x: u32, y: u32) -> bool {
    let Some(h) = deco_hit(hit_test(st, x, y)) else {
        return false;
    };
    let Some(i) = deco_frame(h) else {
        return false;
    };
    if i >= st.frames.len() {
        return false;
    }
    let f = st.frames[i];
    let by = f.oy.saturating_add(DECO_BTN_PAD);
    let bx = f.ox.saturating_add(f.cw);
    let in_x = x >= bx.saturating_sub(BTN.saturating_mul(3)) && x < bx;
    let in_y = y >= by && y < by.saturating_add(BTN);
    in_x && in_y
}

fn files_row_rect(f: &Frame, row: usize) -> Rect {
    let Some(rel) = fm::row_client_y(row) else {
        return RECT_EMPTY;
    };
    let top = f.oy.saturating_add(DECO_H).saturating_add(rel);
    let y1 = top.saturating_add(fm::ROW_H);
    let bot =
        f.oy.saturating_add(DECO_H)
            .saturating_add(f.ch)
            .saturating_sub(fm::STATUS_H);
    Rect {
        x0: f.ox.saturating_add(fm::list_x0()),
        y0: top.min(bot),
        x1: f.ox.saturating_add(f.cw),
        y1: y1.min(bot),
    }
}

fn files_hover_rect(f: &Frame, part: fm::HoverPart) -> Rect {
    match part {
        fm::HoverPart::List(row) => files_row_rect(f, row),
        fm::HoverPart::Side => {
            let y0 = f.oy.saturating_add(DECO_H).saturating_add(fm::NAV_H);
            let y1 =
                f.oy.saturating_add(DECO_H)
                    .saturating_add(f.ch)
                    .saturating_sub(fm::STATUS_H);
            Rect {
                x0: f.ox,
                y0,
                x1: f.ox.saturating_add(fm::SIDE_W),
                y1,
            }
        }
        fm::HoverPart::Nav => Rect {
            x0: f.ox,
            y0: f.oy.saturating_add(DECO_H),
            x1: f.ox.saturating_add(f.cw),
            y1: f.oy.saturating_add(DECO_H).saturating_add(fm::NAV_H),
        },
    }
}

fn update_deco_hover(st: &mut State, x: u32, y: u32) {
    let nh = deco_hit(hit_test(st, x, y));
    let pill = deco_pill_at(st, x, y);
    if nh == st.deco_hover && pill == st.deco_hover_pill {
        return;
    }
    let old = st.deco_hover;
    st.deco_hover = nh;
    st.deco_hover_pill = pill;
    let mut r = RECT_EMPTY;
    for h in [old, nh].into_iter().flatten() {
        if let Some(i) = deco_frame(h) {
            if i < st.frames.len() {
                let bar = deco_bar(st, i);
                r = if rect_is_empty(r) {
                    bar
                } else {
                    rect_union(r, bar)
                };
            }
        }
    }
    if !rect_is_empty(r) {
        present_damage(st, r, false);
    }
}

fn update_power_hover(st: &mut State, x: u32, y: u32) {
    let nh = match confirm_hit(st, x, y) {
        ConfirmHit::Cancel => PowerHover::Cancel,
        ConfirmHit::Ok => PowerHover::Ok,
        _ => PowerHover::None,
    };
    if nh == st.power_hover {
        return;
    }
    st.power_hover = nh;
    chrome::present_confirm(st);
}

pub fn irq_client_char(b: u8) -> bool {
    if POWER_OPEN.load(Ordering::Acquire)
        || RUNNER_OPEN.load(Ordering::Acquire)
        || LAUNCHER_OPEN.load(Ordering::Acquire)
        || DESK_MENU_OPEN.load(Ordering::Acquire)
        || FILES_MENU_OPEN.load(Ordering::Acquire)
    {
        return false;
    }
    if !client_on_top() {
        return false;
    }
    if (0x20..=0x7E).contains(&b) {
        push_key(b);
        return true;
    }
    false
}

fn present_windows() {
    let frames = crate::win::drain_dirty();
    if frames.is_empty() {
        return;
    }
    let mut g = STATE.lock();
    let Some(st) = g.as_mut() else {
        return;
    };
    undraw_cursor(st);
    let mut clip = RECT_EMPTY;
    for f in &frames {
        clip = rect_union(
            clip,
            Rect {
                x0: f.sx,
                y0: f.sy,
                x1: f.sx.saturating_add(f.w),
                y1: f.sy.saturating_add(f.h),
            },
        );
    }
    present_damage(st, clip, false);
    for f in &frames {
        log_blit(f.w, f.h);
    }
    draw_cursor(st);
}

pub fn repaint_strut() -> bool {
    if !READY.load(Ordering::Acquire) {
        return false;
    }
    let mut g = STATE.lock();
    let Some(st) = g.as_mut() else {
        return false;
    };
    undraw_cursor(st);
    paint_clock(st);
    draw_cursor(st);
    true
}
