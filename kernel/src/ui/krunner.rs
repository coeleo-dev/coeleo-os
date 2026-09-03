//! KRunner overlay: centred query, substring filter, hit-test, paint.
//! Spawn and z-order stay in `comp`. Not a `Frame`.

use alloc::string::String;
use alloc::vec::Vec;

use crate::fbterm::FbInfo;
use coeleo_draw::{self, Clip, Icon, Target};
use coeleo_theme::{DIM, RADIUS, SHADOW_A, SHADOW_PX, SURFACE, TEXT};

pub const RUNNER_W: u32 = 480;
pub const QUERY_H: u32 = coeleo_draw::FONT_H + coeleo_theme::GAP;
pub const ROW: u32 = QUERY_H;
pub const PAD: u32 = coeleo_theme::PAD;
pub const HINT: &str = "Esc to close";
pub const HINT_H: u32 = coeleo_draw::FONT_H + coeleo_theme::PAD;

const SHADOW_OFF: u32 = coeleo_theme::SHADOW_OFF;

#[derive(Clone, Copy)]
pub struct Rect {
    pub x0: u32,
    pub y0: u32,
    pub x1: u32,
    pub y1: u32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    Outside,
    Query,
    Row(usize),
}

pub fn popup(fb_w: u32, h_work: u32, n_filtered: usize) -> Rect {
    let w = RUNNER_W.min(fb_w.saturating_sub(16)).max(1);
    let n = n_filtered.max(1) as u32;
    let h = PAD
        .saturating_mul(2)
        .saturating_add(QUERY_H)
        .saturating_add(n.saturating_mul(ROW))
        .saturating_add(HINT_H);
    let x0 = fb_w.saturating_sub(w) / 2;
    let y0 = h_work / 4;
    Rect {
        x0,
        y0,
        x1: x0.saturating_add(w).min(fb_w),
        y1: y0.saturating_add(h).min(h_work),
    }
}

pub fn shadow(popup: Rect, fb_w: u32, h_work: u32) -> Rect {
    let up = SHADOW_PX.saturating_sub(SHADOW_OFF);
    Rect {
        x0: popup.x0.saturating_sub(up),
        y0: popup.y0.saturating_sub(up),
        x1: popup.x1.saturating_add(SHADOW_PX).min(fb_w),
        y1: popup.y1.saturating_add(SHADOW_PX).min(h_work),
    }
}

fn label(name: &str) -> &str {
    match name {
        "files" => "Files",
        "sh" => "Terminal",
        "hello" => "Hello",
        "widgets" => "Widgets",
        "winprobe" => "Winprobe",
        "install" => "Install Coeleo",
        other => other,
    }
}

fn contains_ci(hay: &str, needle: &[u8]) -> bool {
    if needle.is_empty() {
        return true;
    }
    hay.as_bytes()
        .windows(needle.len())
        .any(|w| w.eq_ignore_ascii_case(needle))
}

pub fn filter(names: &[String], query: &[u8]) -> Vec<String> {
    names
        .iter()
        .filter(|n| contains_ci(n, query) || contains_ci(label(n), query))
        .cloned()
        .collect()
}

pub fn hit(popup: Rect, x: u32, y: u32, n_rows: usize) -> Hit {
    if !contains(popup, x, y) {
        return Hit::Outside;
    }
    let q1 = popup.y0.saturating_add(PAD).saturating_add(QUERY_H);
    if y < q1 {
        return Hit::Query;
    }
    let hint_y = popup.y1.saturating_sub(HINT_H);
    if y >= hint_y {
        return Hit::Query;
    }
    let i = (y.saturating_sub(q1) / ROW) as usize;
    if i < n_rows { Hit::Row(i) } else { Hit::Query }
}

pub fn paint(
    fb: FbInfo,
    clip: Rect,
    popup: Rect,
    shadow: Rect,
    query: &[u8],
    rows: &[String],
    sel: usize,
    caret_on: bool,
) {
    let t = Target {
        addr: fb.addr,
        w: fb.w,
        h: fb.h,
        pitch: fb.pitch,
    };
    let vis_sh = intersect(shadow, clip);
    if empty(vis_sh) {
        return;
    }
    shade_ring(t, vis_sh, popup, shadow);
    let vis = intersect(popup, clip);
    if empty(vis) {
        return;
    }
    let dclip = Clip {
        x0: vis.x0,
        y0: vis.y0,
        x1: vis.x1,
        y1: vis.y1,
    };
    let pw = popup.x1.saturating_sub(popup.x0);
    let ph = popup.y1.saturating_sub(popup.y0);
    coeleo_draw::fill_round(t, popup.x0, popup.y0, pw, ph, RADIUS, SURFACE, dclip);
    let qy = popup.y0.saturating_add(PAD);
    let qw = pw.saturating_sub(PAD * 2);
    let qs = core::str::from_utf8(query).unwrap_or("");
    coeleo_draw::query_field(
        t,
        popup.x0.saturating_add(PAD),
        qy,
        qw,
        QUERY_H,
        qs,
        "Search...",
        caret_on,
        true,
        dclip,
    );
    let ry0 = qy.saturating_add(QUERY_H);
    if rows.is_empty() {
        coeleo_draw::text(
            t,
            popup.x0.saturating_add(PAD),
            ry0.saturating_add(PAD / 2),
            "No matches",
            DIM,
            popup.x1,
            dclip,
        );
    } else {
        for (i, name) in rows.iter().enumerate() {
            let y = ry0.saturating_add(i as u32 * ROW);
            let ic = match name.as_str() {
                "files" => Icon::Folder,
                "sh" => Icon::Terminal,
                _ => Icon::App,
            };
            coeleo_draw::list_row(
                t,
                popup.x0.saturating_add(PAD),
                y,
                pw.saturating_sub(PAD * 2),
                ROW,
                label(name),
                Some(ic),
                i == sel,
                false,
                dclip,
            );
        }
    }
    let hy = popup.y1.saturating_sub(HINT_H);
    coeleo_draw::text(
        t,
        popup.x0.saturating_add(PAD),
        hy.saturating_add(PAD / 2),
        HINT,
        TEXT,
        popup.x1,
        dclip,
    );
}

fn shade_ring(t: Target, clip: Rect, popup: Rect, shadow: Rect) {
    let x0 = popup.x0;
    let y0 = popup.y0;
    let x1 = popup.x1;
    let y1 = popup.y1;
    let bands = [
        Rect {
            x0: shadow.x0,
            y0: shadow.y0,
            x1: shadow.x1,
            y1: y0,
        },
        Rect {
            x0: shadow.x0,
            y0: y1,
            x1: shadow.x1,
            y1: shadow.y1,
        },
        Rect {
            x0: shadow.x0,
            y0: y0,
            x1: x0,
            y1: y1,
        },
        Rect {
            x0: x1,
            y0: y0,
            x1: shadow.x1,
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
        let vis = intersect(band, clip);
        if empty(vis) {
            continue;
        }
        for y in vis.y0..vis.y1 {
            for x in vis.x0..vis.x1 {
                shade_px(t, x, y, popup);
            }
        }
    }
}

fn shade_px(t: Target, x: u32, y: u32, popup: Rect) {
    let w = popup.x1.saturating_sub(popup.x0);
    let h = popup.y1.saturating_sub(popup.y0);
    let d = coeleo_draw::round_sdf(x, y, popup.x0, popup.y0, w, h, RADIUS);
    if d == 0 || d > SHADOW_PX {
        return;
    }
    let a = SHADOW_A[(d as usize).min(SHADOW_A.len() - 1)];
    coeleo_draw::shade_px(t, x, y, a);
}

fn contains(r: Rect, x: u32, y: u32) -> bool {
    x >= r.x0 && x < r.x1 && y >= r.y0 && y < r.y1
}

fn empty(r: Rect) -> bool {
    r.x0 >= r.x1 || r.y0 >= r.y1
}

fn intersect(a: Rect, b: Rect) -> Rect {
    Rect {
        x0: a.x0.max(b.x0),
        y0: a.y0.max(b.y0),
        x1: a.x1.min(b.x1),
        y1: a.y1.min(b.y1),
    }
}
