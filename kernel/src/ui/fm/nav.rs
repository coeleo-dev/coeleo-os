//! Files navigation hit-test, history, crumbs.

use alloc::string::String;
use alloc::vec::Vec;

use coeleo_theme::{GAP, PAD};

use super::{
    BTN, ChromeHover, HistEnt, HoverPart, Mode, NAV_H, NavFocus, NavHit, PATH_CAP, ROW_H,
    SEARCH_CAP, SIDE_W, STATUS_H, SideHit, State, join, list_y0, parent, reload, visible,
};

pub(super) fn hover_part(h: ChromeHover) -> HoverPart {
    match h {
        ChromeHover::List(i) => HoverPart::List(i),
        ChromeHover::Side(_) => HoverPart::Side,
        ChromeHover::Nav(_) => HoverPart::Nav,
    }
}

pub(super) fn chrome_at(s: &State, lx: u32, ly: u32, w: u32) -> Option<ChromeHover> {
    if ly < NAV_H {
        return Some(ChromeHover::Nav(nav_hit(s, lx, w)));
    }
    if lx < SIDE_W {
        return side_hit(s, ly).map(ChromeHover::Side);
    }
    if !matches!(s.mode, Mode::List) || ly < list_y0() {
        return None;
    }
    let row = s.scroll.saturating_add(((ly - list_y0()) / ROW_H) as usize);
    let n = if s.search_len == 0 {
        s.ents.len()
    } else {
        visible(s).len()
    };
    if row < n {
        Some(ChromeHover::List(row))
    } else {
        None
    }
}

pub(super) fn nav_layout(w: u32) -> (u32, u32, u32) {
    let px = BTN * 3 + PAD / 2;
    let search_w = 88u32.min(w / 4);
    let path_w = w.saturating_sub(px + search_w + PAD);
    (px, path_w, search_w)
}

pub(super) fn nav_hit(s: &State, lx: u32, w: u32) -> NavHit {
    if lx < BTN {
        NavHit::Back
    } else if lx < BTN * 2 {
        NavHit::Fwd
    } else if lx < BTN * 3 {
        NavHit::Up
    } else {
        let (px, path_w, _) = nav_layout(w);
        if lx < px.saturating_add(path_w) {
            if matches!(s.nav, NavFocus::Path) {
                NavHit::Path
            } else {
                crumb_at(s, w, lx).unwrap_or(NavHit::Path)
            }
        } else {
            NavHit::Search
        }
    }
}

pub(super) fn side_hit(s: &State, ly: u32) -> Option<SideHit> {
    let mut y = NAV_H + PAD / 2 + ROW_H;
    if ly >= y && ly < y + ROW_H {
        return Some(SideHit::Root);
    }
    y += ROW_H;
    if s.places_docs {
        if ly >= y && ly < y + ROW_H {
            return Some(SideHit::Docs);
        }
        y += ROW_H;
    }
    y += ROW_H;
    if ly >= y && ly < y + ROW_H {
        return Some(SideHit::Vol);
    }
    y += ROW_H;
    y += ROW_H;
    if s.tree_open {
        let mut i = 0u8;
        for _ent in s.ents.iter().filter(|e| e.is_dir && e.name != "..") {
            if ly >= y && ly < y + ROW_H {
                return Some(SideHit::Tree(i));
            }
            y += ROW_H;
            i = i.saturating_add(1);
        }
    }
    None
}

pub(super) fn click_navbar(s: &mut State, lx: u32, w: u32) {
    match nav_hit(s, lx, w) {
        NavHit::Back => hist_back(s),
        NavHit::Fwd => hist_fwd(s),
        NavHit::Up => go_parent(s),
        NavHit::Path => begin_path_edit(s),
        NavHit::Search => s.nav = NavFocus::Search,
        NavHit::Crumb(i) => {
            let segs = path_segments(&s.cwd);
            let i = i as usize;
            if i + 1 >= segs.len() {
                begin_path_edit(s);
            } else if let Some((_, path)) = segs.get(i) {
                go(s, path.clone(), true);
            }
        }
    }
}

pub(super) fn begin_path_edit(s: &mut State) {
    s.nav = NavFocus::Path;
    let b = s.cwd.as_bytes();
    let n = b.len().min(PATH_CAP);
    s.path_edit[..n].copy_from_slice(&b[..n]);
    s.path_len = n;
}

pub(super) fn click_sidebar(s: &mut State, ly: u32) {
    let mut y = NAV_H + PAD / 2 + ROW_H;
    if ly >= y && ly < y + ROW_H {
        go(s, String::from("/"), true);
        return;
    }
    y += ROW_H;
    if s.places_docs {
        if ly >= y && ly < y + ROW_H {
            go(s, String::from("/docs"), true);
            return;
        }
        y += ROW_H;
    }
    y += ROW_H; // Devices header
    if ly >= y && ly < y + ROW_H {
        go(s, String::from("/"), true);
        return;
    }
    y += ROW_H; // device row
    y += ROW_H; // Tree header
    if s.tree_open {
        let dirs: Vec<String> = s
            .ents
            .iter()
            .filter(|e| e.is_dir && e.name != "..")
            .map(|e| e.name.clone())
            .collect();
        for name in dirs {
            if ly >= y && ly < y + ROW_H {
                let path = join(&s.cwd, &name);
                go(s, path, true);
                return;
            }
            y += ROW_H;
        }
    }
}
pub(super) fn hist_back(s: &mut State) {
    if s.hist_i == 0 {
        return;
    }
    remember_view(s);
    s.hist_i -= 1;
    s.cwd = s.hist[s.hist_i].path.clone();
    reload(s);
    restore_view(s);
}

pub(super) fn hist_fwd(s: &mut State) {
    if s.hist_i + 1 >= s.hist.len() {
        return;
    }
    remember_view(s);
    s.hist_i += 1;
    s.cwd = s.hist[s.hist_i].path.clone();
    reload(s);
    restore_view(s);
}

pub(super) fn go(s: &mut State, path: String, record: bool) {
    if record {
        remember_view(s);
        s.hist.truncate(s.hist_i + 1);
        s.hist.push(HistEnt {
            path: path.clone(),
            sel: 0,
            scroll: 0,
        });
        s.hist_i = s.hist.len() - 1;
    }
    s.cwd = path;
    s.search_len = 0;
    s.prefix_len = 0;
    s.nav = NavFocus::List;
    reload(s);
}

pub(super) fn go_parent(s: &mut State) {
    if s.cwd != "/" {
        go(s, parent(&s.cwd), true);
    }
}

pub(super) fn remember_view(s: &mut State) {
    if let Some(e) = s.hist.get_mut(s.hist_i) {
        e.sel = s.sel;
        e.scroll = s.scroll;
        e.path = s.cwd.clone();
    }
}

pub(super) fn restore_view(s: &mut State) {
    if let Some(e) = s.hist.get(s.hist_i) {
        let n = visible(s).len();
        s.sel = if n == 0 { 0 } else { e.sel.min(n - 1) };
        s.scroll = e.scroll.min(s.sel);
        ensure_visible(s);
    }
}
pub(super) fn path_segments(cwd: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    out.push((String::from("/"), String::from("/")));
    if cwd == "/" {
        return out;
    }
    let rest = cwd.trim_start_matches('/');
    let mut acc = String::new();
    for part in rest.split('/') {
        if part.is_empty() {
            continue;
        }
        acc.push('/');
        acc.push_str(part);
        out.push((String::from(part), acc.clone()));
    }
    out
}

pub(super) fn crumb_sep_w() -> u32 {
    coeleo_draw::text_width(">").saturating_add(GAP / 2)
}

pub(super) fn crumbs_span(segs: &[(String, String)], start: usize) -> u32 {
    let sep = crumb_sep_w();
    let mut w = 0u32;
    if start > 0 {
        w = coeleo_draw::text_width("...").saturating_add(sep);
    }
    for i in start..segs.len() {
        if i > start {
            w = w.saturating_add(sep);
        }
        w = w.saturating_add(coeleo_draw::crumb_width(&segs[i].0));
    }
    w
}

pub(super) fn crumb_start(segs: &[(String, String)], path_w: u32) -> usize {
    if segs.is_empty() {
        return 0;
    }
    let mut start = 0usize;
    while start + 1 < segs.len() && crumbs_span(segs, start) > path_w {
        start += 1;
    }
    start
}

pub(super) fn crumb_at(s: &State, w: u32, lx: u32) -> Option<NavHit> {
    let (px, path_w, _) = nav_layout(w);
    if lx < px || lx >= px.saturating_add(path_w) {
        return None;
    }
    let segs = path_segments(&s.cwd);
    let start = crumb_start(&segs, path_w);
    let sep = crumb_sep_w();
    let mut x = px;
    if start > 0 {
        x = x.saturating_add(coeleo_draw::text_width("...").saturating_add(sep));
    }
    for i in start..segs.len() {
        let cw = coeleo_draw::crumb_width(&segs[i].0);
        if lx >= x && lx < x.saturating_add(cw) {
            return Some(NavHit::Crumb(i as u8));
        }
        x = x.saturating_add(cw);
        if i + 1 < segs.len() {
            x = x.saturating_add(sep);
        }
    }
    None
}

pub(super) fn typeahead(s: &mut State, c: u8) {
    let t = crate::clock::ticks();
    if t.saturating_sub(s.prefix_at) >= 100 {
        s.prefix_len = 0;
    }
    if s.prefix_len < SEARCH_CAP {
        s.prefix[s.prefix_len] = c.to_ascii_lowercase();
        s.prefix_len += 1;
    }
    s.prefix_at = t;
    let p = &s.prefix[..s.prefix_len];
    let shown = visible(s);
    for (i, e) in shown.iter().enumerate() {
        if e.name.to_ascii_lowercase().as_bytes().starts_with(p) {
            s.sel = i;
            ensure_visible(s);
            return;
        }
    }
}

pub(super) fn page_rows(s: &State) -> usize {
    let body = s.last_h.saturating_sub(list_y0()).saturating_sub(STATUS_H);
    (body / ROW_H).max(1) as usize
}

pub(super) fn ensure_visible(s: &mut State) {
    if s.last_h == 0 {
        return;
    }
    let page = page_rows(s);
    let n = visible(s).len();
    if n == 0 {
        s.scroll = 0;
        s.sel = 0;
        return;
    }
    if s.sel >= n {
        s.sel = n - 1;
    }
    if s.sel < s.scroll {
        s.scroll = s.sel;
    } else if s.sel >= s.scroll + page {
        s.scroll = s.sel + 1 - page;
    }
}
