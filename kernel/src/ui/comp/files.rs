//! File-manager client paint sink.

use crate::fbterm::FbInfo;
use crate::fm;
use coeleo_draw::{self, Clip, Icon};
use coeleo_theme::RADIUS;

use super::fb::{blend, fill_span, get_fb, put_fb, tgt};
use super::rect::{Rect, rect_contains, rect_intersect, rect_is_empty, rect_sub};

pub(super) struct FilesSink {
    pub(super) fb: FbInfo,
    pub(super) ox: u32,
    pub(super) oy: u32,
    pub(super) w: u32,
    pub(super) h: u32,
    pub(super) wx: u32,
    pub(super) wy: u32,
    pub(super) ww: u32,
    pub(super) wh: u32,
    pub(super) clip: Rect,
    pub(super) protect: Rect,
}

impl FilesSink {
    fn with_clips(&self, mut f: impl FnMut(Clip)) {
        if rect_is_empty(self.protect) {
            f(Clip {
                x0: self.clip.x0,
                y0: self.clip.y0,
                x1: self.clip.x1,
                y1: self.clip.y1,
            });
            return;
        }
        let (parts, n) = rect_sub(self.clip, self.protect);
        for k in 0..n {
            if rect_is_empty(parts[k]) {
                continue;
            }
            f(Clip {
                x0: parts[k].x0,
                y0: parts[k].y0,
                x1: parts[k].x1,
                y1: parts[k].y1,
            });
        }
    }

    pub(super) fn put(&mut self, x: u32, y: u32, color: u32) {
        if !rect_contains(self.clip, x, y) || rect_contains(self.protect, x, y) {
            return;
        }
        let cov = coeleo_draw::coverage_round(x, y, self.wx, self.wy, self.ww, self.wh, RADIUS);
        if cov == 0 {
            return;
        }
        if cov == 255 {
            put_fb(&self.fb, x, y, color.to_le_bytes());
        } else {
            put_fb(
                &self.fb,
                x,
                y,
                blend(get_fb(&self.fb, x as i32, y as i32), color, cov),
            );
        }
    }

    fn fill_vis(&mut self, r: Rect, color: u32) {
        let px = color.to_le_bytes();
        let y_inner0 = self.wy.saturating_add(RADIUS);
        let y_inner1 = self.wy.saturating_add(self.wh).saturating_sub(RADIUS);
        for y in r.y0..r.y1 {
            if y >= y_inner0 && y < y_inner1 {
                fill_span(&self.fb, r.x0, y, r.x1.saturating_sub(r.x0), px);
            } else {
                for x in r.x0..r.x1 {
                    self.put(x, y, color);
                }
            }
        }
    }
}

impl fm::Draw for FilesSink {
    fn fill(&mut self, x: u32, y: u32, rw: u32, rh: u32, color: u32) {
        let x1 = x.min(self.w);
        let y1 = y.min(self.h);
        let x2 = x.saturating_add(rw).min(self.w);
        let y2 = y.saturating_add(rh).min(self.h);
        if x1 >= x2 || y1 >= y2 {
            return;
        }
        let vis = rect_intersect(
            Rect {
                x0: self.ox.saturating_add(x1),
                y0: self.oy.saturating_add(y1),
                x1: self.ox.saturating_add(x2),
                y1: self.oy.saturating_add(y2),
            },
            self.clip,
        );
        if rect_is_empty(vis) {
            return;
        }
        if rect_is_empty(self.protect) {
            self.fill_vis(vis, color);
            return;
        }
        let (parts, n) = rect_sub(vis, self.protect);
        for k in 0..n {
            if !rect_is_empty(parts[k]) {
                self.fill_vis(parts[k], color);
            }
        }
    }

    fn text(&mut self, x: u32, y: u32, s: &str, color: u32) {
        self.text_elide(x, y, s, color, self.w);
    }

    fn fill_round(&mut self, x: u32, y: u32, rw: u32, rh: u32, radius: u32, color: u32) {
        let t = tgt(self.fb);
        let sx = self.ox.saturating_add(x);
        let sy = self.oy.saturating_add(y);
        self.with_clips(|c| {
            coeleo_draw::fill_round(t, sx, sy, rw, rh, radius, color, c);
        });
    }

    fn text_elide(&mut self, x: u32, y: u32, s: &str, color: u32, x1: u32) {
        let t = tgt(self.fb);
        let tx = self.ox.saturating_add(x);
        let ty = self.oy.saturating_add(y);
        let xmax = self.ox.saturating_add(x1.min(self.w));
        self.with_clips(|c| {
            coeleo_draw::text_elide(t, tx, ty, s, color, xmax, c);
        });
    }

    fn blit(&mut self, x: u32, y: u32, pix: &[u32], pw: u32, ph: u32) {
        for row in 0..ph {
            for col in 0..pw {
                let px_x = x.saturating_add(col);
                let py = y.saturating_add(row);
                if px_x >= self.w || py >= self.h {
                    continue;
                }
                let sx = self.ox + px_x;
                let sy = self.oy + py;
                if rect_contains(self.clip, sx, sy) && !rect_contains(self.protect, sx, sy) {
                    let c = pix[(row * pw + col) as usize] & 0x00FF_FFFF;
                    self.put(sx, sy, c);
                }
            }
        }
    }

    fn icon(&mut self, which: Icon, x: u32, y: u32, color: u32) {
        let t = tgt(self.fb);
        let ix = self.ox.saturating_add(x);
        let iy = self.oy.saturating_add(y);
        self.with_clips(|c| {
            coeleo_draw::icon(t, which, ix, iy, color, c);
        });
    }

    fn icon_btn(&mut self, x: u32, y: u32, which: Icon, hovered: bool, enabled: bool) {
        let t = tgt(self.fb);
        let sx = self.ox.saturating_add(x);
        let sy = self.oy.saturating_add(y);
        self.with_clips(|c| {
            coeleo_draw::icon_btn(t, sx, sy, which, hovered, false, enabled, None, c);
        });
    }

    fn query_field(
        &mut self,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
        text: &str,
        placeholder: &str,
        caret: bool,
        search_icon: bool,
    ) {
        let t = tgt(self.fb);
        let sx = self.ox.saturating_add(x);
        let sy = self.oy.saturating_add(y);
        self.with_clips(|c| {
            coeleo_draw::query_field(t, sx, sy, w, h, text, placeholder, caret, search_icon, c);
        });
    }

    fn crumb(&mut self, x: u32, y: u32, h: u32, label: &str, hovered: bool) -> u32 {
        let t = tgt(self.fb);
        let sx = self.ox.saturating_add(x);
        let sy = self.oy.saturating_add(y);
        let mut w = 0u32;
        self.with_clips(|c| {
            w = coeleo_draw::crumb(t, sx, sy, h, label, hovered, c);
        });
        if w == 0 {
            coeleo_draw::crumb_width(label)
        } else {
            w
        }
    }

    fn crumb_sep(&mut self, x: u32, y: u32, h: u32) -> u32 {
        let t = tgt(self.fb);
        let sx = self.ox.saturating_add(x);
        let sy = self.oy.saturating_add(y);
        let mut w = 0u32;
        self.with_clips(|c| {
            w = coeleo_draw::crumb_sep(t, sx, sy, h, c);
        });
        if w == 0 {
            coeleo_draw::text_width(">").saturating_add(coeleo_theme::GAP / 2)
        } else {
            w
        }
    }

    fn vsep(&mut self, x: u32, y: u32, h: u32) {
        let t = tgt(self.fb);
        let sx = self.ox.saturating_add(x);
        let sy = self.oy.saturating_add(y);
        self.with_clips(|c| {
            coeleo_draw::vsep(t, sx, sy, h, c);
        });
    }

    fn list_row(
        &mut self,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
        label: &str,
        icon: Option<Icon>,
        selected: bool,
        hovered: bool,
    ) {
        let t = tgt(self.fb);
        let sx = self.ox.saturating_add(x);
        let sy = self.oy.saturating_add(y);
        self.with_clips(|c| {
            coeleo_draw::list_row(t, sx, sy, w, h, label, icon, selected, hovered, c);
        });
    }
}
