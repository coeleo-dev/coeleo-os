//! Immediate-mode widgets. `no_std`, no alloc. Paint via `coeleo-draw`.

#![no_std]

use coeleo_draw::{self, Clip, Icon, Target};
use coeleo_theme::{BG, DANGER, DIM, HIGHLIGHT, PAD, RADIUS_SM, SURFACE, TEXT};

const KIND_MOVE: u32 = 1;
const KIND_DOWN: u32 = 2;
const KIND_KEY: u32 = 3;
const EV_SIZE: usize = 16;

pub const KEY_TAB: u8 = 1;
pub const KEY_UP: u8 = 2;
pub const KEY_DOWN: u8 = 3;
pub const KEY_ENTER: u8 = 4;
pub const KEY_BACK: u8 = 5;
pub const KEY_ESC: u8 = 6;
pub const KEY_LEFT: u8 = 8;
pub const KEY_RIGHT: u8 = 9;
pub const KEY_DEL: u8 = 10;
pub const ROW: u32 = coeleo_draw::ROW;

pub struct Ui {
    ptr: *mut u32,
    w: u32,
    h: u32,
    mouse: Option<(u32, u32)>,
    pos: Option<(u32, u32)>,
    key: Option<u8>,
    dirty: bool,
    first: bool,
    focus: Option<(u32, u32, u32)>,
}

impl Ui {
    pub const fn new() -> Self {
        Self {
            ptr: core::ptr::null_mut(),
            w: 0,
            h: 0,
            mouse: None,
            pos: None,
            key: None,
            dirty: false,
            first: true,
            focus: None,
        }
    }

    pub fn begin(&mut self, buf: &mut [u32], w: u32, h: u32) {
        self.ptr = buf.as_mut_ptr();
        self.w = w;
        self.h = h;
        let n = (w as usize).saturating_mul(h as usize).min(buf.len());
        for p in &mut buf[..n] {
            *p = BG;
        }
        if self.first {
            self.dirty = true;
        }
    }

    pub fn label(&mut self, x: u32, y: u32, s: &str) {
        self.text(x, y, s, TEXT);
    }

    pub fn label_color(&mut self, x: u32, y: u32, s: &str, color: u32) {
        self.text(x, y, s, color);
    }

    pub fn button(&mut self, x: u32, y: u32, w: u32, h: u32, s: &str) -> bool {
        self.button_en(x, y, w, h, s, SURFACE, TEXT, true)
    }

    pub fn button_fill(&mut self, x: u32, y: u32, w: u32, h: u32, s: &str, fill: u32) -> bool {
        self.button_en(x, y, w, h, s, fill, TEXT, true)
    }

    pub fn button_danger(&mut self, x: u32, y: u32, w: u32, h: u32, s: &str) -> bool {
        self.button_en(x, y, w, h, s, SURFACE, DANGER, true)
    }

    pub fn button_en(
        &mut self,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
        s: &str,
        fill: u32,
        fg: u32,
        enabled: bool,
    ) -> bool {
        let hit = enabled && self.down_in(x, y, w, h);
        let hover = enabled && self.pos_in(x, y, w, h);
        let t = self.target();
        let clip = self.clip();
        let bg = if !enabled {
            fill
        } else if hit {
            HIGHLIGHT
        } else if hover {
            coeleo_theme::HOVER
        } else {
            fill
        };
        coeleo_draw::fill_round(t, x, y, w, h, RADIUS_SM, bg, clip);
        let color = if enabled { fg } else { DIM };
        let tw = coeleo_draw::text_width(s);
        let tx = x.saturating_add(w.saturating_sub(tw) / 2);
        let mut ty = y.saturating_add(h.saturating_sub(coeleo_draw::FONT_H) / 2);
        if hit {
            ty = ty.saturating_add(1);
        }
        coeleo_draw::text(t, tx, ty, s, color, x.saturating_add(w), clip);
        if hit {
            self.mouse = None;
            self.dirty = true;
            true
        } else {
            false
        }
    }

    pub fn icon_button(&mut self, x: u32, y: u32, which: Icon) -> bool {
        self.icon_button_en(x, y, which, true)
    }

    pub fn icon_button_en(&mut self, x: u32, y: u32, which: Icon, enabled: bool) -> bool {
        let w = coeleo_theme::BUTTON_H;
        let h = coeleo_theme::BUTTON_H;
        let hit = enabled && self.down_in(x, y, w, h);
        let hover = enabled && self.pos_in(x, y, w, h);
        coeleo_draw::icon_btn(
            self.target(),
            x,
            y,
            which,
            hover,
            hit,
            enabled,
            Some(SURFACE),
            self.clip(),
        );
        if hit {
            self.mouse = None;
            self.dirty = true;
            true
        } else {
            false
        }
    }

    pub fn crumb(&mut self, x: u32, y: u32, h: u32, label: &str) -> bool {
        let w = coeleo_draw::crumb_width(label);
        let hit = self.down_in(x, y, w, h);
        let hover = self.pos_in(x, y, w, h);
        let _ = coeleo_draw::crumb(self.target(), x, y, h, label, hover, self.clip());
        if hit {
            self.mouse = None;
            self.dirty = true;
            true
        } else {
            false
        }
    }

    pub fn tooltip(&mut self, x: u32, y: u32, s: &str) {
        let t = self.target();
        coeleo_draw::tooltip(t, x, y, s, false, self.clip());
        self.dirty = true;
    }

    pub fn hovering(&self, x: u32, y: u32, w: u32, h: u32) -> bool {
        self.pos_in(x, y, w, h)
    }

    pub fn key(&self) -> Option<u8> {
        self.key
    }

    pub fn down_outside(&self, x: u32, y: u32, w: u32, h: u32) -> bool {
        self.mouse.is_some() && !self.down_in(x, y, w, h)
    }

    pub fn text_field(&mut self, x: u32, y: u32, w: u32, buf: &mut [u8]) -> bool {
        self.field(x, y, w, buf, "", false)
    }

    pub fn search_field(&mut self, x: u32, y: u32, w: u32, buf: &mut [u8]) -> bool {
        self.field(x, y, w, buf, "Search...", true)
    }

    fn field(
        &mut self,
        x: u32,
        y: u32,
        w: u32,
        buf: &mut [u8],
        placeholder: &str,
        search_icon: bool,
    ) -> bool {
        let h = coeleo_draw::FONT_H.saturating_add(PAD);
        let hit = self.down_in(x, y, w, h);
        if hit {
            self.focus = Some((x, y, w));
            self.mouse = None;
            self.dirty = true;
        }
        let focused = self.focus == Some((x, y, w));
        if focused {
            self.edit_buf(buf);
        }
        coeleo_draw::query_field(
            self.target(),
            x,
            y,
            w,
            h,
            cstr(buf),
            placeholder,
            focused,
            search_icon,
            self.clip(),
        );
        hit
    }

    fn edit_buf(&mut self, buf: &mut [u8]) {
        let Some(k) = self.key else {
            return;
        };
        let n = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        if k == KEY_BACK {
            if n > 0 {
                buf[n - 1] = 0;
                self.dirty = true;
            }
            self.key = None;
            return;
        }
        if (0x20..=0x7E).contains(&k) && n + 1 < buf.len() {
            buf[n] = k;
            buf[n + 1] = 0;
            self.dirty = true;
            self.key = None;
        }
    }

    pub fn toolbar(&mut self, x: u32, y: u32, w: u32, h: u32) {
        let t = self.target();
        coeleo_draw::fill_round(t, x, y, w, h, RADIUS_SM, SURFACE, self.clip());
    }

    pub fn disc(&mut self, x: u32, y: u32, d: u32, color: u32) {
        let t = self.target();
        coeleo_draw::fill_round(t, x, y, d, d, d / 2, color, self.clip());
        self.dirty = true;
    }

    pub fn list_row(&mut self, x: u32, y: u32, w: u32, s: &str, sel: bool) -> bool {
        let h = coeleo_draw::ROW;
        let hover = self.pos_in(x, y, w, h);
        coeleo_draw::list_row(
            self.target(),
            x,
            y,
            w,
            h,
            s,
            None,
            sel,
            hover && !sel,
            self.clip(),
        );
        let hit = self.down_in(x, y, w, h);
        if hit {
            self.mouse = None;
            self.dirty = true;
        }
        hit
    }

    pub fn end(&mut self) -> bool {
        self.mouse = None;
        self.key = None;
        let dirty = self.dirty;
        self.dirty = false;
        self.first = false;
        dirty
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        let mut i = 0usize;
        while i + EV_SIZE <= bytes.len() {
            let kind = u32::from_le_bytes(slice4(bytes, i));
            let x = i32::from_le_bytes(slice4(bytes, i + 4));
            let y = i32::from_le_bytes(slice4(bytes, i + 8));
            let key = u32::from_le_bytes(slice4(bytes, i + 12));
            if kind == KIND_KEY {
                self.key = Some(key as u8);
                self.dirty = true;
                i += EV_SIZE;
                continue;
            }
            if x >= 0 && y >= 0 {
                let p = (x as u32, y as u32);
                if kind == KIND_DOWN {
                    self.mouse = Some(p);
                    self.pos = Some(p);
                    self.dirty = true;
                } else if kind == KIND_MOVE {
                    if self.pos != Some(p) {
                        self.pos = Some(p);
                        self.dirty = true;
                    }
                }
            }
            i += EV_SIZE;
        }
    }

    fn down_in(&self, x: u32, y: u32, w: u32, h: u32) -> bool {
        in_rect(self.mouse, x, y, w, h)
    }

    fn pos_in(&self, x: u32, y: u32, w: u32, h: u32) -> bool {
        in_rect(self.pos, x, y, w, h)
    }

    fn text(&mut self, x: u32, y: u32, s: &str, color: u32) {
        coeleo_draw::text(self.target(), x, y, s, color, self.w, self.clip());
    }

    fn target(&self) -> Target {
        Target {
            addr: self.ptr as usize,
            w: self.w,
            h: self.h,
            pitch: self.w.saturating_mul(4),
        }
    }

    fn clip(&self) -> Clip {
        Clip::all(self.w, self.h)
    }
}

fn in_rect(p: Option<(u32, u32)>, x: u32, y: u32, w: u32, h: u32) -> bool {
    let Some((mx, my)) = p else {
        return false;
    };
    mx >= x && my >= y && mx < x.saturating_add(w) && my < y.saturating_add(h)
}

fn slice4(bytes: &[u8], i: usize) -> [u8; 4] {
    let mut a = [0u8; 4];
    a.copy_from_slice(&bytes[i..i + 4]);
    a
}

fn cstr(buf: &[u8]) -> &str {
    let n = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    core::str::from_utf8(&buf[..n]).unwrap_or("")
}
