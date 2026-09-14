//! Files chrome paint.

use alloc::string::String;

use crate::fs::DirEnt;
use coeleo_draw::Icon;
use coeleo_theme::{ACCENT, BG, DIM, PAD, SURFACE, TEXT};

use super::nav::{crumb_start, nav_layout, path_segments};
use super::{
    BTN, COL_H, ChromeHover, Draw, Mode, NAV_H, NavFocus, NavHit, ROW_H, SIDE_W, STATUS_H, SideHit,
    State, visible,
};

pub(super) fn paint_navbar(s: &State, w: u32, d: &mut impl Draw) {
    d.fill(0, 0, w, NAV_H, SURFACE);
    let by = (NAV_H.saturating_sub(coeleo_theme::BUTTON_H)) / 2;
    let nav_h = s.hover;
    let back_en = s.hist_i > 0;
    let fwd_en = s.hist_i + 1 < s.hist.len();
    let up_en = s.cwd != "/";
    d.icon_btn(
        0,
        by,
        Icon::Back,
        matches!(nav_h, Some(ChromeHover::Nav(NavHit::Back))) && back_en,
        back_en,
    );
    d.icon_btn(
        BTN,
        by,
        Icon::Forward,
        matches!(nav_h, Some(ChromeHover::Nav(NavHit::Fwd))) && fwd_en,
        fwd_en,
    );
    d.icon_btn(
        BTN * 2,
        by,
        Icon::Up,
        matches!(nav_h, Some(ChromeHover::Nav(NavHit::Up))) && up_en,
        up_en,
    );
    d.vsep(BTN * 3, 4, NAV_H.saturating_sub(8));
    let (px, path_w, search_w) = nav_layout(w);
    let fh = NAV_H.saturating_sub(4);
    if matches!(s.nav, NavFocus::Path) {
        let path = if s.path_len > 0 {
            core::str::from_utf8(&s.path_edit[..s.path_len]).unwrap_or(&s.cwd)
        } else {
            ""
        };
        d.query_field(px, 2, path_w, fh, path, s.cwd.as_str(), true, false);
    } else {
        paint_crumbs(s, w, d);
    }
    let sx = px + path_w + PAD / 2;
    let q = core::str::from_utf8(&s.search[..s.search_len]).unwrap_or("");
    d.query_field(
        sx,
        2,
        search_w,
        fh,
        q,
        "Search",
        matches!(s.nav, NavFocus::Search),
        true,
    );
}

pub(super) fn paint_crumbs(s: &State, w: u32, d: &mut impl Draw) {
    let (px, path_w, _) = nav_layout(w);
    let segs = path_segments(&s.cwd);
    if segs.is_empty() {
        return;
    }
    let chip_h = coeleo_theme::BUTTON_H;
    let cy = (NAV_H.saturating_sub(chip_h)) / 2;
    let x1 = px.saturating_add(path_w);
    let start = crumb_start(&segs, path_w);
    let mut x = px;
    if start > 0 {
        d.text_elide(
            x,
            cy + (chip_h.saturating_sub(coeleo_draw::FONT_H)) / 2,
            "...",
            DIM,
            x1,
        );
        x = x.saturating_add(coeleo_draw::text_width("..."));
        if x < x1 {
            x = x.saturating_add(d.crumb_sep(x, cy, chip_h));
        }
    }
    for i in start..segs.len() {
        if x >= x1 {
            break;
        }
        let hovered =
            matches!(s.hover, Some(ChromeHover::Nav(NavHit::Crumb(c))) if c as usize == i);
        let cw = d.crumb(x, cy, chip_h, &segs[i].0, hovered);
        x = x.saturating_add(cw);
        if i + 1 < segs.len() && x < x1 {
            x = x.saturating_add(d.crumb_sep(x, cy, chip_h));
        }
    }
}

pub(super) fn paint_sidebar(s: &State, h: u32, d: &mut impl Draw) {
    let sh = h.saturating_sub(NAV_H).saturating_sub(STATUS_H);
    d.fill(0, NAV_H, SIDE_W, sh, SURFACE);
    d.fill(SIDE_W.saturating_sub(1), NAV_H, 1, sh, BG);
    let mut y = NAV_H + PAD / 2;
    d.text(PAD, y, "Places", DIM);
    y += ROW_H;
    paint_side_row(s, d, y, SideHit::Root, Icon::Folder, ACCENT, "/");
    y += ROW_H;
    if s.places_docs {
        paint_side_row(s, d, y, SideHit::Docs, Icon::Folder, ACCENT, "docs");
        y += ROW_H;
    }
    d.text(PAD, y, "Devices", DIM);
    y += ROW_H;
    let vol = crate::fs::volume_label();
    let label = vol.as_deref().unwrap_or("Live disk");
    paint_side_row(s, d, y, SideHit::Vol, Icon::App, TEXT, label);
    y += ROW_H;
    d.text(PAD, y, "Folders", DIM);
    y += ROW_H;
    if s.tree_open {
        let mut i = 0u8;
        for ent in s.ents.iter().filter(|e| e.is_dir && e.name != "..") {
            if y + ROW_H > NAV_H + sh {
                break;
            }
            paint_side_row(s, d, y, SideHit::Tree(i), Icon::Folder, ACCENT, &ent.name);
            y += ROW_H;
            i = i.saturating_add(1);
        }
    }
}

pub(super) fn paint_side_row(
    s: &State,
    d: &mut impl Draw,
    y: u32,
    hit: SideHit,
    ic: Icon,
    _ic_color: u32,
    label: &str,
) {
    let current = match hit {
        SideHit::Root => s.cwd == "/",
        SideHit::Docs => s.cwd == "/docs",
        SideHit::Vol | SideHit::Tree(_) => false,
    };
    let hovered = matches!(s.hover, Some(ChromeHover::Side(h)) if h == hit);
    d.list_row(
        PAD / 2,
        y,
        SIDE_W.saturating_sub(PAD),
        ROW_H,
        label,
        Some(ic),
        current,
        hovered && !current,
    );
}

pub(super) const SIZE_COL: u32 = 48;
pub(super) const DATE_COL: u32 = 148;

pub(super) fn col_size_x(x: u32, w: u32) -> u32 {
    x + w
        .saturating_sub(SIZE_COL + DATE_COL)
        .max(coeleo_draw::ICON + PAD * 2)
}

pub(super) fn paint_columns(x: u32, w: u32, d: &mut impl Draw) {
    d.fill(x, NAV_H, w, COL_H, SURFACE);
    let ty = NAV_H + (COL_H.saturating_sub(coeleo_draw::FONT_H)) / 2;
    d.text(x + PAD / 2, ty, "Name", DIM);
    let sz_x = col_size_x(x, w);
    d.text(sz_x, ty, "Size", DIM);
    d.text(sz_x + SIZE_COL, ty, "Date", DIM);
    d.fill(x, NAV_H.saturating_add(COL_H).saturating_sub(1), w, 1, BG);
}

pub(super) fn paint_row(d: &mut impl Draw, x: u32, y: u32, w: u32, ent: &DirEnt) {
    let ic = if ent.is_dir { Icon::Folder } else { Icon::App };
    let iy = y + (ROW_H.saturating_sub(coeleo_draw::ICON)) / 2;
    let ty = y + (ROW_H.saturating_sub(coeleo_draw::FONT_H)) / 2;
    d.icon(ic, x + PAD / 2, iy, if ent.is_dir { ACCENT } else { TEXT });
    let sz_x = col_size_x(x, w);
    let name_x = x + PAD / 2 + coeleo_draw::ICON + PAD / 2;
    d.text_elide(name_x, ty, &ent.name, TEXT, sz_x);
    if !ent.is_dir && ent.name != ".." {
        d.text_elide(sz_x, ty, &fmt_size(ent.size), TEXT, sz_x + SIZE_COL);
    }
    if let Some(t) = ent.mtime {
        d.text_elide(sz_x + SIZE_COL, ty, &fmt_date(t), DIM, x.saturating_add(w));
    } else if ent.name != ".." {
        d.text_elide(sz_x + SIZE_COL, ty, "—", DIM, x.saturating_add(w));
    }
}

pub(super) fn paint_image(
    s: &mut State,
    body_x: u32,
    body_y: u32,
    body_w: u32,
    body_h: u32,
    d: &mut impl Draw,
) {
    let Some((iw, ih, _)) = s.image.as_ref() else {
        return;
    };
    let iw = *iw;
    let ih = *ih;
    let dw = body_w.saturating_sub(PAD * 2);
    let dh = body_h.saturating_sub(PAD * 2);
    if dw == 0 || dh == 0 {
        return;
    }
    let stale = match s.scaled.as_ref() {
        Some((sw, sh, _)) => *sw != dw || *sh != dh,
        None => true,
    };
    if stale {
        let pix = s.image.as_ref().map(|im| im.2.as_slice()).unwrap_or(&[]);
        match coeleo_image::scale_box(pix, iw, ih, dw, dh) {
            Ok(scaled) => s.scaled = Some((dw, dh, scaled)),
            Err(()) => {
                d.text(body_x + PAD, body_y + PAD, "fm: not image", TEXT);
                return;
            }
        }
    }
    if let Some((_, _, scaled)) = s.scaled.as_ref() {
        d.blit(body_x + PAD, body_y + PAD, scaled, dw, dh);
    }
}

pub(super) fn fmt_items(n: usize) -> String {
    let mut s = String::new();
    let mut buf = [0u8; 20];
    let mut i = 20;
    let mut x = n;
    if x == 0 {
        s.push('0');
    } else {
        while x > 0 {
            i -= 1;
            buf[i] = b'0' + (x % 10) as u8;
            x /= 10;
        }
        s.push_str(core::str::from_utf8(&buf[i..]).unwrap_or("0"));
    }
    if n == 1 {
        s.push_str(" item");
    } else {
        s.push_str(" items");
    }
    s
}

pub(super) fn fmt_size(n: u64) -> String {
    if n < 1024 {
        let mut s = String::new();
        // small int
        let mut buf = [0u8; 20];
        let mut i = 20;
        let mut x = n;
        if x == 0 {
            return String::from("0");
        }
        while x > 0 {
            i -= 1;
            buf[i] = b'0' + (x % 10) as u8;
            x /= 10;
        }
        s.push_str(core::str::from_utf8(&buf[i..]).unwrap_or("0"));
        s
    } else {
        let kb = n / 1024;
        let mut s = String::new();
        let mut buf = [0u8; 20];
        let mut i = 20;
        let mut x = kb;
        if x == 0 {
            s.push('0');
        } else {
            while x > 0 {
                i -= 1;
                buf[i] = b'0' + (x % 10) as u8;
                x /= 10;
            }
            s.push_str(core::str::from_utf8(&buf[i..]).unwrap_or("0"));
        }
        s.push_str("K");
        s
    }
}

pub(super) fn fmt_date(t: (u16, u8, u8, u8, u8, u8)) -> String {
    let (y, mo, d, h, mi, _) = t;
    let mut s = String::new();
    fn push_u(s: &mut String, n: u32, w: usize) {
        let mut buf = [b'0'; 4];
        let mut x = n;
        for i in (0..w).rev() {
            buf[i] = b'0' + (x % 10) as u8;
            x /= 10;
        }
        s.push_str(core::str::from_utf8(&buf[4 - w..4]).unwrap_or(""));
    }
    push_u(&mut s, y as u32, 4);
    s.push('-');
    push_u(&mut s, mo as u32, 2);
    s.push('-');
    push_u(&mut s, d as u32, 2);
    s.push(' ');
    push_u(&mut s, h as u32, 2);
    s.push(':');
    push_u(&mut s, mi as u32, 2);
    s
}

pub(super) fn paint_status(s: &State, w: u32, h: u32, d: &mut impl Draw) {
    let y = h.saturating_sub(STATUS_H);
    d.fill(0, y, w, STATUS_H, SURFACE);
    let ty = y + (STATUS_H.saturating_sub(coeleo_draw::FONT_H) / 2);
    if !s.status_msg.is_empty() {
        d.text_elide(PAD, ty, &s.status_msg, TEXT, w.saturating_sub(PAD));
        return;
    }
    let shown = visible(s);
    if matches!(s.mode, Mode::List) {
        d.text(PAD, ty, &fmt_items(shown.len()), TEXT);
        if let Some(ent) = shown.get(s.sel) {
            if !ent.is_dir {
                let mid = w / 2;
                d.text_elide(mid, ty, &ent.name, TEXT, w.saturating_sub(PAD));
                let sz = fmt_size(ent.size);
                let sx = mid
                    .saturating_add(coeleo_draw::text_width(&ent.name))
                    .saturating_add(PAD);
                d.text_elide(
                    sx.min(w.saturating_sub(PAD)),
                    ty,
                    &sz,
                    DIM,
                    w.saturating_sub(PAD),
                );
            }
        }
    } else {
        d.text_elide(PAD, ty, &s.title, TEXT, w / 2);
        d.text_elide(w / 2, ty, "Esc to close", DIM, w.saturating_sub(PAD));
    }
}
