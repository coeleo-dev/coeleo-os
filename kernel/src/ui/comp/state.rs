//! Compositor session: frames, focus, overlays, damage queue.

use alloc::string::String;
use alloc::vec::Vec;
use core::sync::atomic::{AtomicBool, AtomicUsize};

use spin::Mutex;

use crate::fbterm::FbInfo;
use crate::panel;
use coeleo_theme::DECO_BTN;

use super::rect::Rect;

pub(super) const BTN: u32 = DECO_BTN;
pub(super) const DAMAGE_CAP: usize = 8;
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Focus {
    Term,
    Files,
    Desk,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum FrameKind {
    Vt,
    Files,
    Settings,
    Client(u32),
}

#[derive(Clone, Copy)]
pub(super) struct Frame {
    pub(super) kind: FrameKind,
    pub(super) ox: u32,
    pub(super) oy: u32,
    pub(super) cw: u32,
    pub(super) ch: u32,
    pub(super) seq: u32,
    pub(super) minimized: bool,
    pub(super) maximized: bool,
    pub(super) saved_ox: u32,
    pub(super) saved_oy: u32,
    pub(super) saved_cw: u32,
    pub(super) saved_ch: u32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Edge {
    N,
    S,
    E,
    W,
    Ne,
    Nw,
    Se,
    Sw,
}

#[derive(Clone, Copy)]
pub(super) struct Drag {
    pub(super) kind: FrameKind,
    pub(super) gx: i32,
    pub(super) gy: i32,
    pub(super) edge: Option<Edge>,
    pub(super) ox0: u32,
    pub(super) oy0: u32,
    pub(super) cw0: u32,
    pub(super) ch0: u32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Hit {
    Title(usize),
    Close(usize),
    Min(usize),
    Max(usize),
    Client(usize),
    Resize(usize, Edge),
}
pub(super) struct State {
    pub(super) fb: FbInfo,
    pub(super) scene: Vec<u32>,
    pub(super) scene_fb: FbInfo,
    pub(super) pending: [Rect; DAMAGE_CAP],
    pub(super) n_pending: usize,
    pub(super) cursor_old: Rect,
    pub(super) cx: i32,
    pub(super) cy: i32,
    pub(super) drawn: bool,
    pub(super) focus: Focus,
    pub(super) frames: Vec<Frame>,
    pub(super) drag: Option<Drag>,
    pub(super) drag_at: u64,
    pub(super) drag_shadow_logged: bool,
    pub(super) cursor_log_at: u64,
    pub(super) hover: panel::Hit,
    pub(super) pressed: panel::Hit,
    pub(super) launcher_open: bool,
    pub(super) runner_open: bool,
    pub(super) desk_menu_open: bool,
    pub(super) desk_menu_x: u32,
    pub(super) desk_menu_y: u32,
    pub(super) files_menu_open: bool,
    pub(super) files_menu_x: u32,
    pub(super) files_menu_y: u32,
    pub(super) files_menu_sel: usize,
    pub(super) btn_held: bool,
    pub(super) launch_sel: usize,
    pub(super) launch_list: Vec<String>,
    pub(super) runner_query: [u8; 24],
    pub(super) runner_query_len: usize,
    pub(super) runner_sel: usize,
    pub(super) runner_list: Vec<String>,
    pub(super) panel_opaque: bool,
    pub(super) seq_next: u32,
    pub(super) wallpaper: Vec<u32>,
    pub(super) wall_w: u32,
    pub(super) wall_h: u32,
    pub(super) menu_icon: Vec<u32>,
    pub(super) menu_iw: u32,
    pub(super) menu_ih: u32,
    pub(super) tip: panel::Tip,
    pub(super) power_dlg: Option<PowerKind>,
    pub(super) power_ok: bool,
    pub(super) power_hover: PowerHover,
    pub(super) deco_hover: Option<Hit>,
    pub(super) deco_hover_pill: bool,
    pub(super) runner_caret: bool,
    pub(super) title_click_at: u64,
    pub(super) title_click_kind: Option<FrameKind>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum PowerHover {
    None,
    Cancel,
    Ok,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum PowerKind {
    Reboot,
    PowerOff,
}

pub(super) static STATE: Mutex<Option<State>> = Mutex::new(None);
pub(super) static READY: AtomicBool = AtomicBool::new(false);
pub(super) static FOCUS_FILES: AtomicBool = AtomicBool::new(false);
pub(super) static FOCUS_DESK: AtomicBool = AtomicBool::new(false);
pub(super) static CLIENT_TOP: AtomicBool = AtomicBool::new(false);
pub(super) static LAUNCHER_OPEN: AtomicBool = AtomicBool::new(false);
pub(super) static RUNNER_OPEN: AtomicBool = AtomicBool::new(false);
pub(super) static POWER_OPEN: AtomicBool = AtomicBool::new(false);
pub(super) static DESK_MENU_OPEN: AtomicBool = AtomicBool::new(false);
pub(super) static FILES_MENU_OPEN: AtomicBool = AtomicBool::new(false);

pub(super) const KEY_CAP: usize = 16;
pub(super) static mut KEY_Q: [u8; KEY_CAP] = [0; KEY_CAP];
pub(super) static KEY_HEAD: AtomicUsize = AtomicUsize::new(0);
pub(super) static KEY_TAIL: AtomicUsize = AtomicUsize::new(0);

pub(super) const KEY_TAB: u8 = 1;
pub(super) const KEY_UP: u8 = 2;
pub(super) const KEY_DOWN: u8 = 3;
pub(super) const KEY_ENTER: u8 = 4;
pub(super) const KEY_BACK: u8 = 5;
pub(super) const KEY_ESC: u8 = 6;
pub(super) const KEY_RUNNER: u8 = 7;
pub(super) const KEY_LEFT: u8 = 8;
pub(super) const KEY_RIGHT: u8 = 9;
pub(super) const KEY_DEL: u8 = 10;
