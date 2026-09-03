//! Panel clicks, launcher, KRunner (policy). Spawn stays here.

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::Ordering;

use crate::fs;
use crate::panel;
use crate::serial;

use super::frames::{
    close_frame, ensure_frame, focus_frame, minimize_frame, raise, raise_visible, restore_min,
};
use super::geom::{
    confirm_hit, desk_menu_popup, desk_menu_row_at, desk_menu_shadow, krunner_shadow,
    launcher_shadow, shadow_rect, task_frame, task_infos, visible_top, work_rect, ConfirmHit,
    LAUNCH_NAMES,
};
use super::paint::{fill_rect_r, paint_strut, present_damage};
use super::rect::{rect_contains, rect_union, Rect};
use super::state::{
    DESK_MENU_OPEN, LAUNCHER_OPEN, POWER_OPEN, RUNNER_OPEN, FrameKind, PowerKind, State,
};

pub(super) fn clock_now<'a>(buf: &'a mut [u8; 16]) -> &'a str {
    crate::clock::format_panel(buf)
}

pub(super) fn panel_hit(st: &State, x: u32, y: u32) -> panel::Hit {
    let mut time = [0u8; 16];
    let clock = clock_now(&mut time);
    let tasks = task_infos(st);
    panel::hit(x, y, st.fb.w, st.fb.h, &tasks, clock)
}

pub(super) fn update_hover(st: &mut State, x: u32, y: u32) {
    let nh = panel_hit(st, x, y);
    if nh != st.hover {
        let old = st.tip;
        st.hover = nh;
        if !old.is_empty() {
            fill_rect_r(
                st,
                Rect {
                    x0: old.x,
                    y0: old.y,
                    x1: old.x.saturating_add(old.w),
                    y1: old.y.saturating_add(old.h),
                },
            );
            present_damage(
                st,
                Rect {
                    x0: old.x,
                    y0: old.y,
                    x1: old.x.saturating_add(old.w),
                    y1: old.y.saturating_add(old.h),
                },
                false,
            );
        }
        paint_strut(st);
    }
}

pub(super) fn handle_panel_click(st: &mut State, x: u32, y: u32) {
    let h = panel_hit(st, x, y);
    st.pressed = h;
    match h {
        panel::Hit::Launcher => {
            if st.launcher_open {
                close_launcher(st);
            } else {
                open_launcher(st);
            }
        }
        panel::Hit::KRunner => {
            if st.runner_open {
                close_runner(st);
            } else {
                open_runner(st);
            }
        }
        panel::Hit::Task(k) => {
            close_launcher(st);
            close_runner(st);
            handle_task_click(st, k);
        }
        panel::Hit::TaskClose(k) => {
            close_launcher(st);
            close_runner(st);
            handle_task_close(st, k);
        }
        panel::Hit::Clock | panel::Hit::None => {
            close_launcher(st);
            close_runner(st);
            close_desk_menu(st);
        }
    }
    paint_strut(st);
}

pub(super) fn handle_task_click(st: &mut State, k: usize) {
    let Some(i) = task_frame(st, k) else {
        return;
    };
    let top = visible_top(st);
    if st.frames[i].minimized {
        restore_min(st, i);
        return;
    }
    if top == Some(i) {
        minimize_frame(st, i);
        return;
    }
    raise_visible(st, i);
    focus_frame(st, i);
    present_damage(st, shadow_rect(&st.frames[st.frames.len() - 1]), false);
}

pub(super) fn handle_task_close(st: &mut State, k: usize) {
    let Some(i) = task_frame(st, k) else {
        return;
    };
    close_frame(st, i);
}

pub(super) fn launcher_entries() -> Vec<String> {
    let mut found = Vec::new();
    found.push(String::from("files"));
    found.push(String::from("sh"));
    if let Ok(ents) = fs::list("/") {
        for e in ents {
            if e.is_dir {
                continue;
            }
            if LAUNCH_NAMES.iter().any(|n| *n == e.name.as_str()) {
                push_unique(&mut found, e.name);
            }
        }
    }
    if let Ok(ents) = fs::list("/bin") {
        for e in ents {
            if e.is_dir {
                continue;
            }
            push_unique(&mut found, e.name);
        }
    }
    found
}

pub(super) fn push_unique(found: &mut Vec<String>, name: String) {
    if found.iter().any(|n| n == &name) {
        return;
    }
    found.push(name);
}

pub(super) fn spawn_app(name: &str) -> Option<u32> {
    let mut bin = String::from("/bin/");
    bin.push_str(name);
    crate::sched::spawn_path(&bin).or_else(|| crate::sched::spawn_path(name))
}
pub(super) fn open_launcher(st: &mut State) {
    close_runner(st);
    close_desk_menu(st);
    st.launch_list = launcher_entries();
    st.launch_sel = 0;
    st.launcher_open = true;
    LAUNCHER_OPEN.store(true, Ordering::Release);
    serial::write_str("launcher: open\n");
    present_launcher(st);
}

pub(super) fn close_launcher(st: &mut State) {
    if !st.launcher_open {
        return;
    }
    let r = launcher_shadow(st);
    st.launcher_open = false;
    LAUNCHER_OPEN.store(false, Ordering::Release);
    st.launch_list.clear();
    serial::write_str("launcher: close\n");
    fill_rect_r(st, r);
    present_damage(st, r, false);
}

pub(super) fn present_launcher(st: &mut State) {
    present_damage(st, launcher_shadow(st), false);
}

pub(super) fn open_runner(st: &mut State) {
    close_launcher(st);
    close_desk_menu(st);
    st.runner_query_len = 0;
    st.runner_list = crate::krunner::filter(&launcher_entries(), &[]);
    st.runner_sel = 0;
    st.runner_open = true;
    RUNNER_OPEN.store(true, Ordering::Release);
    serial::write_str("krunner: open\n");
    present_krunner(st);
}

pub(super) fn close_runner(st: &mut State) {
    if !st.runner_open {
        return;
    }
    let r = krunner_shadow(st);
    st.runner_open = false;
    RUNNER_OPEN.store(false, Ordering::Release);
    st.runner_query_len = 0;
    st.runner_list.clear();
    serial::write_str("krunner: close\n");
    fill_rect_r(st, r);
    present_damage(st, r, false);
}

pub(super) fn present_krunner(st: &mut State) {
    present_damage(st, krunner_shadow(st), false);
}

pub(super) fn open_desk_menu(st: &mut State, x: u32, y: u32) {
    close_launcher(st);
    close_runner(st);
    st.desk_menu_x = x;
    st.desk_menu_y = y;
    st.desk_menu_open = true;
    DESK_MENU_OPEN.store(true, Ordering::Release);
    serial::write_str("desk: menu\n");
    present_desk_menu(st);
}

pub(super) fn close_desk_menu(st: &mut State) {
    if !st.desk_menu_open {
        return;
    }
    let r = desk_menu_shadow(st);
    st.desk_menu_open = false;
    DESK_MENU_OPEN.store(false, Ordering::Release);
    fill_rect_r(st, r);
    present_damage(st, r, false);
}

pub(super) fn present_desk_menu(st: &mut State) {
    present_damage(st, desk_menu_shadow(st), false);
}

pub(super) fn desk_menu_activate(st: &mut State) {
    close_desk_menu(st);
    crate::deskset::refresh();
    let Some(i) = ensure_frame(st, FrameKind::Settings) else {
        return;
    };
    st.frames[i].minimized = false;
    raise(st, i);
    focus_frame(st, st.frames.len() - 1);
    present_damage(st, shadow_rect(&st.frames[st.frames.len() - 1]), false);
    paint_strut(st);
}

pub(super) fn handle_desk_menu_click(st: &mut State, x: u32, y: u32) -> bool {
    if !st.desk_menu_open {
        return false;
    }
    if desk_menu_row_at(st, x, y) {
        desk_menu_activate(st);
        return true;
    }
    if !rect_contains(desk_menu_popup(st), x, y) {
        close_desk_menu(st);
    }
    true
}

pub(super) fn runner_refresh(st: &mut State) {
    let old = krunner_shadow(st);
    st.runner_list =
        crate::krunner::filter(&launcher_entries(), &st.runner_query[..st.runner_query_len]);
    if st.runner_list.is_empty() {
        st.runner_sel = 0;
    } else if st.runner_sel >= st.runner_list.len() {
        st.runner_sel = st.runner_list.len() - 1;
    }
    fill_rect_r(st, old);
    present_damage(st, rect_union(old, krunner_shadow(st)), false);
}

pub(super) fn runner_enter(st: &mut State) {
    if st.runner_list.is_empty() {
        return;
    }
    runner_launch(st, st.runner_sel);
}

pub(super) fn runner_launch(st: &mut State, row: usize) {
    let Some(name) = st.runner_list.get(row).cloned() else {
        return;
    };
    close_runner(st);
    if focus_named(st, &name) {
        return;
    }
    match spawn_app(&name) {
        Some(_) => {
            serial::write_str("run: ");
            serial::write_str(&name);
            serial::write_str("\n");
        }
        None => serial::write_str("run: not found\n"),
    }
}

pub(super) fn launch_index(st: &mut State, row: usize) {
    let n = st.launch_list.len();
    if row == n {
        open_power_dlg(st, PowerKind::Reboot);
        return;
    }
    if row == n + 1 {
        open_power_dlg(st, PowerKind::PowerOff);
        return;
    }
    let Some(name) = st.launch_list.get(row).cloned() else {
        close_launcher(st);
        return;
    };
    close_launcher(st);
    if focus_named(st, &name) {
        return;
    }
    let _ = spawn_app(&name);
}

pub(super) fn open_power_dlg(st: &mut State, kind: PowerKind) {
    close_launcher(st);
    close_runner(st);
    close_desk_menu(st);
    st.power_dlg = Some(kind);
    st.power_ok = false;
    POWER_OPEN.store(true, Ordering::Release);
    match kind {
        PowerKind::Reboot => serial::write_str("power: confirm reboot\n"),
        PowerKind::PowerOff => serial::write_str("power: confirm poweroff\n"),
    }
    present_confirm(st);
}

pub(super) fn close_power_dlg(st: &mut State) {
    if st.power_dlg.is_none() {
        return;
    }
    let r = work_rect(st);
    st.power_dlg = None;
    st.power_ok = false;
    st.power_hover = super::state::PowerHover::None;
    POWER_OPEN.store(false, Ordering::Release);
    fill_rect_r(st, r);
    present_damage(st, r, false);
}

pub(super) fn present_confirm(st: &mut State) {
    present_damage(st, work_rect(st), false);
}

pub(super) fn handle_confirm_click(st: &mut State, x: u32, y: u32) {
    match confirm_hit(st, x, y) {
        ConfirmHit::Cancel | ConfirmHit::Outside => close_power_dlg(st),
        ConfirmHit::Ok => confirm_apply(st),
        ConfirmHit::Body => {}
    }
}

pub(super) fn confirm_apply(st: &mut State) {
    let kind = st.power_dlg;
    close_power_dlg(st);
    let _ = crate::fd::sys_sync();
    match kind {
        Some(PowerKind::Reboot) => crate::acpi::reboot(),
        Some(PowerKind::PowerOff) => {
            let _ = crate::acpi::poweroff();
        }
        None => {}
    }
}

pub(super) fn confirm_key(st: &mut State, k: u8) {
    use super::state::{KEY_ENTER, KEY_ESC, KEY_LEFT, KEY_RIGHT, KEY_TAB};
    match k {
        KEY_ESC => close_power_dlg(st),
        KEY_LEFT | KEY_TAB => {
            st.power_ok = !st.power_ok;
            present_confirm(st);
        }
        KEY_RIGHT => {
            st.power_ok = true;
            present_confirm(st);
        }
        KEY_ENTER => {
            if st.power_ok {
                confirm_apply(st);
            } else {
                close_power_dlg(st);
            }
        }
        _ => {}
    }
}

pub(super) fn focus_named(st: &mut State, name: &str) -> bool {
    let kind = match name {
        "sh" => FrameKind::Vt,
        "files" => FrameKind::Files,
        _ => return false,
    };
    let Some(i) = ensure_frame(st, kind) else {
        return true;
    };
    st.frames[i].minimized = false;
    raise(st, i);
    focus_frame(st, st.frames.len() - 1);
    present_damage(st, shadow_rect(&st.frames[st.frames.len() - 1]), false);
    paint_strut(st);
    true
}
