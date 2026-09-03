//! Kernel desktop settings: panel mode and wallpaper. No compositor lock.

use alloc::string::String;
use alloc::vec::Vec;

use crate::fm;
use crate::fs;
use crate::panel;
use coeleo_theme::{ACCENT, BG, DIM, GAP, HIGHLIGHT, PAD, SURFACE, TEXT};

const BTN_H: u32 = 24;
const ROW_H: u32 = coeleo_draw::FONT_H + GAP;

#[derive(Clone)]
enum WallItem {
    Default,
    Path(String),
}

struct State {
    walls: Vec<WallItem>,
    wall_sel: usize,
}

static STATE: spin::Mutex<State> = spin::Mutex::new(State {
    walls: Vec::new(),
    wall_sel: 0,
});

pub enum Action {
    None,
    SetMode(panel::Mode),
    SetWallDefault,
    SetWallPath(String),
}

pub fn refresh() {
    let mut s = STATE.lock();
    s.walls.clear();
    s.walls.push(WallItem::Default);
    push_images(&mut s.walls, "/");
    push_images(&mut s.walls, "/docs");
    if s.wall_sel >= s.walls.len() {
        s.wall_sel = 0;
    }
}

fn push_images(out: &mut Vec<WallItem>, dir: &str) {
    let Ok(ents) = fs::list(dir) else {
        return;
    };
    for e in ents {
        if e.is_dir {
            continue;
        }
        let path = join(dir, &e.name);
        if is_image_name(&path) {
            out.push(WallItem::Path(path));
        }
    }
}

fn join(dir: &str, name: &str) -> String {
    if dir == "/" {
        let mut s = String::from("/");
        s.push_str(name);
        s
    } else {
        let mut s = String::from(dir);
        if !s.ends_with('/') {
            s.push('/');
        }
        s.push_str(name);
        s
    }
}

fn is_image_name(path: &str) -> bool {
    let mut magic = [0u8; 8];
    match fs::read_at(path, 0, &mut magic) {
        Ok(n) if n >= 2 && magic[0] == 0xFF && magic[1] == 0xD8 => true,
        Ok(n) if n >= 8 && magic.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) => {
            true
        }
        _ => false,
    }
}

fn label(item: &WallItem) -> &str {
    match item {
        WallItem::Default => "Default",
        WallItem::Path(p) => p.rsplit('/').next().unwrap_or(p.as_str()),
    }
}

pub fn render(w: u32, h: u32, d: &mut impl fm::Draw) {
    let s = STATE.lock();
    d.fill(0, 0, w, h, BG);
    let mut y = PAD;
    d.text(PAD, y, "Taskbar", TEXT);
    y += ROW_H;
    let bw = w.saturating_sub(PAD * 3) / 2;
    let float_on = panel::mode() == panel::Mode::Float;
    if float_on {
        d.fill(PAD, y, bw, BTN_H, HIGHLIGHT);
    } else {
        d.fill(PAD, y, bw, BTN_H, SURFACE);
    }
    d.text(PAD + GAP, y + (BTN_H.saturating_sub(coeleo_draw::FONT_H)) / 2, "Float", TEXT);
    let x1 = PAD * 2 + bw;
    if !float_on {
        d.fill(x1, y, bw, BTN_H, HIGHLIGHT);
    } else {
        d.fill(x1, y, bw, BTN_H, SURFACE);
    }
    d.text(x1 + GAP, y + (BTN_H.saturating_sub(coeleo_draw::FONT_H)) / 2, "Full", TEXT);
    y += BTN_H + PAD;
    d.text(PAD, y, "Wallpaper", TEXT);
    y += ROW_H;
    for (i, item) in s.walls.iter().enumerate() {
        if y + ROW_H > h {
            break;
        }
        if i == s.wall_sel {
            d.fill(PAD, y, w.saturating_sub(PAD * 2), ROW_H, HIGHLIGHT);
        }
        d.text(PAD + GAP, y + (ROW_H.saturating_sub(coeleo_draw::FONT_H)) / 2, label(item), TEXT);
        y += ROW_H;
    }
    let _ = (ACCENT, DIM);
}

fn mode_row_y() -> u32 {
    PAD + ROW_H
}

fn list_y0() -> u32 {
    PAD + ROW_H + BTN_H + PAD + ROW_H
}

pub fn click_at(lx: u32, ly: u32, w: u32) -> Action {
    refresh();
    let my = mode_row_y();
    if ly >= my && ly < my + BTN_H {
        let bw = w.saturating_sub(PAD * 3) / 2;
        if lx >= PAD && lx < PAD + bw {
            return Action::SetMode(panel::Mode::Float);
        }
        let x1 = PAD * 2 + bw;
        if lx >= x1 && lx < x1 + bw {
            return Action::SetMode(panel::Mode::Full);
        }
        return Action::None;
    }
    let mut s = STATE.lock();
    let y0 = list_y0();
    if ly < y0 {
        return Action::None;
    }
    let row = ((ly - y0) / ROW_H) as usize;
    if row >= s.walls.len() {
        return Action::None;
    }
    s.wall_sel = row;
    match s.walls.get(row) {
        Some(WallItem::Default) => Action::SetWallDefault,
        Some(WallItem::Path(p)) => Action::SetWallPath(p.clone()),
        None => Action::None,
    }
}

const KEY_UP: u8 = 2;
const KEY_DOWN: u8 = 3;
const KEY_ENTER: u8 = 4;
const KEY_LEFT: u8 = 8;
const KEY_RIGHT: u8 = 9;

pub fn key(k: u8) -> Action {
    refresh();
    let mut s = STATE.lock();
    match k {
        KEY_LEFT => Action::SetMode(panel::Mode::Float),
        KEY_RIGHT => Action::SetMode(panel::Mode::Full),
        KEY_UP => {
            if s.wall_sel > 0 {
                s.wall_sel -= 1;
            }
            Action::None
        }
        KEY_DOWN => {
            if s.wall_sel + 1 < s.walls.len() {
                s.wall_sel += 1;
            }
            Action::None
        }
        KEY_ENTER => match s.walls.get(s.wall_sel) {
            Some(WallItem::Default) => Action::SetWallDefault,
            Some(WallItem::Path(p)) => Action::SetWallPath(p.clone()),
            None => Action::None,
        },
        _ => Action::None,
    }
}
