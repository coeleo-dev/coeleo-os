//! Kernel file-manager panel. Own cwd; no unsafe.

use alloc::string::String;
use alloc::vec::Vec;

use crate::fs::{self, DirEnt, FsError};
use coeleo_draw::{self, Icon};
use coeleo_theme::{ACCENT, BG, DIM, GAP, HIGHLIGHT, HOVER, PAD, RADIUS_SM, SURFACE, TEXT};

pub const TITLE_H: u32 = 32;
pub const NAV_H: u32 = TITLE_H;
pub const SIDE_W: u32 = 160;
pub const COL_H: u32 = coeleo_draw::FONT_H + GAP;
pub const STATUS_H: u32 = coeleo_draw::FONT_H + PAD;
pub const ROW_H: u32 = coeleo_draw::FONT_H + GAP;
const PREVIEW_CAP: usize = 4096;
const SEARCH_CAP: usize = 24;
const BTN: u32 = 24;
const PATH_CAP: usize = 96;

enum Mode {
    List,
    Preview,
    Image,
}

enum NavFocus {
    Path,
    Search,
    List,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum NavHit {
    Back,
    Fwd,
    Path,
    Search,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SideHit {
    Root,
    Docs,
    Vol,
    Tree(u8),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ChromeHover {
    List(usize),
    Side(SideHit),
    Nav(NavHit),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum HoverPart {
    List(usize),
    Side,
    Nav,
}

struct State {
    cwd: String,
    ents: Vec<DirEnt>,
    sel: usize,
    hover: Option<ChromeHover>,
    mode: Mode,
    preview: String,
    image: Option<(u32, u32, Vec<u32>)>,
    scaled: Option<(u32, u32, Vec<u32>)>,
    title: String,
    hist: Vec<String>,
    hist_i: usize,
    search: [u8; SEARCH_CAP],
    search_len: usize,
    nav: NavFocus,
    path_edit: [u8; PATH_CAP],
    path_len: usize,
    tree_open: bool,
    places_docs: bool,
}

static STATE: spin::Mutex<State> = spin::Mutex::new(State {
    cwd: String::new(),
    ents: Vec::new(),
    sel: 0,
    hover: None,
    mode: Mode::List,
    preview: String::new(),
    image: None,
    scaled: None,
    title: String::new(),
    hist: Vec::new(),
    hist_i: 0,
    search: [0; SEARCH_CAP],
    search_len: 0,
    nav: NavFocus::List,
    path_edit: [0; PATH_CAP],
    path_len: 0,
    tree_open: true,
    places_docs: false,
});

pub fn window_title() -> String {
    let s = STATE.lock();
    match s.mode {
        Mode::List => String::from("Files"),
        Mode::Preview | Mode::Image => {
            if s.title.is_empty() || s.title == "files" {
                String::from("Files")
            } else {
                s.title.clone()
            }
        }
    }
}

pub fn init() {
    let mut s = STATE.lock();
    s.cwd = String::from("/");
    s.mode = Mode::List;
    s.preview.clear();
    s.image = None;
    s.scaled = None;
    s.search_len = 0;
    s.nav = NavFocus::List;
    s.path_len = 0;
    s.tree_open = true;
    s.hist.clear();
    s.hist.push(String::from("/"));
    s.hist_i = 0;
    reload(&mut s);
}

pub trait Draw {
    fn fill(&mut self, x: u32, y: u32, w: u32, h: u32, color: u32);
    fn text(&mut self, x: u32, y: u32, s: &str, color: u32);
    fn blit(&mut self, x: u32, y: u32, pix: &[u32], pw: u32, ph: u32) {
        let _ = (x, y, pix, pw, ph);
    }
    fn icon(&mut self, which: Icon, x: u32, y: u32, color: u32) {
        let _ = (which, x, y, color);
    }
    fn fill_round(&mut self, x: u32, y: u32, w: u32, h: u32, _radius: u32, color: u32) {
        self.fill(x, y, w, h, color);
    }
    fn text_elide(&mut self, x: u32, y: u32, s: &str, color: u32, _x1: u32) {
        self.text(x, y, s, color);
    }
}

pub fn list_x0() -> u32 {
    SIDE_W
}

pub fn list_y0() -> u32 {
    NAV_H + COL_H
}

pub fn wants_arrows() -> bool {
    let s = STATE.lock();
    matches!(s.nav, NavFocus::Path | NavFocus::Search)
}

pub fn render(w: u32, h: u32, d: &mut impl Draw) {
    let mut s = STATE.lock();
    d.fill(0, 0, w, h, BG);
    paint_navbar(&s, w, d);
    paint_sidebar(&s, h, d);
    let body_x = SIDE_W;
    let body_w = w.saturating_sub(SIDE_W);
    let body_y = NAV_H;
    let body_h = h.saturating_sub(NAV_H).saturating_sub(STATUS_H);
    match s.mode {
        Mode::List => {
            paint_columns(body_x, body_w, d);
            let shown = visible(&s);
            let mut y = list_y0();
            let bottom = h.saturating_sub(STATUS_H);
            for (i, ent) in shown.iter().enumerate() {
                if y + ROW_H > bottom {
                    break;
                }
                let on = s.sel == i;
                let hv = matches!(s.hover, Some(ChromeHover::List(r)) if r == i);
                if on || hv {
                    let color = if on { HIGHLIGHT } else { HOVER };
                    d.fill_round(
                        body_x.saturating_add(ROW_INSET),
                        y,
                        body_w.saturating_sub(ROW_INSET * 2),
                        ROW_H,
                        RADIUS_SM,
                        color,
                    );
                }
                paint_row(d, body_x, y, body_w, ent);
                y += ROW_H;
            }
        }
        Mode::Preview => {
            let mut y = body_y.saturating_add(PAD);
            let x = body_x.saturating_add(PAD);
            let x1 = w.saturating_sub(PAD);
            for line in s.preview.split('\n') {
                if y + ROW_H > body_y + body_h {
                    break;
                }
                d.text_elide(x, y, line, TEXT, x1);
                y += ROW_H;
            }
        }
        Mode::Image => paint_image(&mut s, body_x, body_y, body_w, body_h, d),
    }
    paint_status(&s, w, h, d);
}

pub fn click_at(lx: u32, ly: u32, w: u32) {
    let mut s = STATE.lock();
    if ly < NAV_H {
        click_navbar(&mut s, lx, w);
        return;
    }
    if lx < SIDE_W {
        click_sidebar(&mut s, ly);
        return;
    }
    match s.mode {
        Mode::List => {
            if ly < list_y0() {
                s.nav = NavFocus::List;
                return;
            }
            let row = ((ly - list_y0()) / ROW_H) as usize;
            let shown = visible(&s);
            if row >= shown.len() {
                s.nav = NavFocus::List;
                return;
            }
            s.sel = row;
            s.nav = NavFocus::List;
            activate(&mut s);
        }
        Mode::Preview | Mode::Image => back_inner(&mut s),
    }
}

pub fn click_row(row: i32) {
    if row < 0 {
        return;
    }
    let mut s = STATE.lock();
    match s.mode {
        Mode::List => {
            let shown = visible(&s);
            if (row as usize) >= shown.len() {
                return;
            }
            s.sel = row as usize;
            activate(&mut s);
        }
        Mode::Preview | Mode::Image => back_inner(&mut s),
    }
}

pub fn hover_at(lx: u32, ly: u32, w: u32) -> Option<(Option<HoverPart>, Option<HoverPart>)> {
    let mut s = STATE.lock();
    let next = chrome_at(&s, lx, ly, w);
    if s.hover == next {
        return None;
    }
    let old = s.hover.map(hover_part);
    s.hover = next;
    Some((old, next.map(hover_part)))
}

fn hover_part(h: ChromeHover) -> HoverPart {
    match h {
        ChromeHover::List(i) => HoverPart::List(i),
        ChromeHover::Side(_) => HoverPart::Side,
        ChromeHover::Nav(_) => HoverPart::Nav,
    }
}

fn chrome_at(s: &State, lx: u32, ly: u32, w: u32) -> Option<ChromeHover> {
    if ly < NAV_H {
        return Some(ChromeHover::Nav(nav_hit(lx, w)));
    }
    if lx < SIDE_W {
        return side_hit(s, ly).map(ChromeHover::Side);
    }
    if !matches!(s.mode, Mode::List) || ly < list_y0() {
        return None;
    }
    let row = ((ly - list_y0()) / ROW_H) as usize;
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

fn nav_layout(w: u32) -> (u32, u32, u32) {
    let px = BTN * 2 + PAD / 2;
    let search_w = 88u32.min(w / 4);
    let path_w = w.saturating_sub(px + search_w + PAD);
    (px, path_w, search_w)
}

fn nav_hit(lx: u32, w: u32) -> NavHit {
    if lx < BTN {
        NavHit::Back
    } else if lx < BTN * 2 {
        NavHit::Fwd
    } else {
        let (px, path_w, _) = nav_layout(w);
        if lx < px.saturating_add(path_w) {
            NavHit::Path
        } else {
            NavHit::Search
        }
    }
}

fn side_hit(s: &State, ly: u32) -> Option<SideHit> {
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

const ROW_INSET: u32 = 4;

pub fn up() {
    let mut s = STATE.lock();
    if !matches!(s.mode, Mode::List) {
        return;
    }
    if s.sel > 0 {
        s.sel -= 1;
    }
}

pub fn down() {
    let mut s = STATE.lock();
    if !matches!(s.mode, Mode::List) {
        return;
    }
    let n = visible(&s).len();
    if n != 0 && s.sel + 1 < n {
        s.sel += 1;
    }
}

pub fn left() {
    let mut s = STATE.lock();
    match s.nav {
        NavFocus::Path => {
            if s.path_len > 0 {
                s.path_len -= 1;
            }
        }
        NavFocus::Search => {
            if s.search_len > 0 {
                s.search_len -= 1;
            }
        }
        NavFocus::List => {}
    }
}

pub fn right() {}

pub fn back() {
    let mut s = STATE.lock();
    match s.nav {
        NavFocus::Path => {
            if s.path_len > 0 {
                s.path_len -= 1;
            } else {
                s.nav = NavFocus::List;
            }
            return;
        }
        NavFocus::Search | NavFocus::List => {
            if s.search_len > 0 && matches!(s.mode, Mode::List) {
                s.search_len -= 1;
                s.sel = 0;
                return;
            }
        }
    }
    back_inner(&mut s);
}

pub fn search_push(c: u8) {
    if !(0x20..=0x7E).contains(&c) {
        return;
    }
    let mut s = STATE.lock();
    match s.nav {
        NavFocus::Path => {
            if s.path_len < PATH_CAP {
                let i = s.path_len;
                s.path_edit[i] = c;
                s.path_len += 1;
            }
        }
        NavFocus::Search | NavFocus::List => {
            if !matches!(s.mode, Mode::List) {
                return;
            }
            if s.search_len < SEARCH_CAP {
                let i = s.search_len;
                s.search[i] = c;
                s.search_len += 1;
                s.sel = 0;
                s.nav = NavFocus::Search;
            }
        }
    }
}

pub fn enter() {
    let mut s = STATE.lock();
    if matches!(s.nav, NavFocus::Path) {
        let p = String::from(core::str::from_utf8(&s.path_edit[..s.path_len]).unwrap_or(""));
        if fs::list(&p).is_ok() {
            go(&mut s, p, true);
            s.nav = NavFocus::List;
        }
        return;
    }
    activate(&mut s);
}

fn paint_navbar(s: &State, w: u32, d: &mut impl Draw) {
    d.fill(0, 0, w, NAV_H, SURFACE);
    let iy = (NAV_H.saturating_sub(coeleo_draw::ICON)) / 2;
    let ty = (NAV_H.saturating_sub(coeleo_draw::FONT_H)) / 2;
    let nav_h = s.hover;
    if matches!(nav_h, Some(ChromeHover::Nav(NavHit::Back))) && s.hist_i > 0 {
        d.fill_round(0, 2, BTN, NAV_H.saturating_sub(4), RADIUS_SM, HOVER);
    }
    if matches!(nav_h, Some(ChromeHover::Nav(NavHit::Fwd))) && s.hist_i + 1 < s.hist.len() {
        d.fill_round(BTN, 2, BTN, NAV_H.saturating_sub(4), RADIUS_SM, HOVER);
    }
    let back = if s.hist_i > 0 { TEXT } else { DIM };
    let fwd = if s.hist_i + 1 < s.hist.len() { TEXT } else { DIM };
    d.icon(Icon::Back, PAD / 2, iy, back);
    d.icon(Icon::Forward, BTN + PAD / 2, iy, fwd);
    let (px, path_w, search_w) = nav_layout(w);
    let fh = NAV_H.saturating_sub(4);
    paint_field(
        d,
        px,
        2,
        path_w,
        fh,
        matches!(s.nav, NavFocus::Path),
    );
    let path = if matches!(s.nav, NavFocus::Path) && s.path_len > 0 {
        core::str::from_utf8(&s.path_edit[..s.path_len]).unwrap_or(&s.cwd)
    } else {
        s.cwd.as_str()
    };
    d.text_elide(px + PAD / 2, ty, path, TEXT, px.saturating_add(path_w).saturating_sub(PAD / 2));
    let sx = px + path_w + PAD / 2;
    paint_field(
        d,
        sx,
        2,
        search_w,
        fh,
        matches!(s.nav, NavFocus::Search),
    );
    d.icon(Icon::Search, sx + 2, iy, DIM);
    let q = core::str::from_utf8(&s.search[..s.search_len]).unwrap_or("");
    let qx = sx + PAD / 2 + coeleo_draw::ICON;
    let q1 = sx.saturating_add(search_w).saturating_sub(PAD / 2);
    if q.is_empty() {
        d.text_elide(qx, ty, "Search", DIM, q1);
    } else {
        d.text_elide(qx, ty, q, TEXT, q1);
    }
}

fn paint_field(d: &mut impl Draw, x: u32, y: u32, w: u32, h: u32, active: bool) {
    if active {
        d.fill_round(x, y, w, h, RADIUS_SM, ACCENT);
        d.fill_round(
            x.saturating_add(1),
            y.saturating_add(1),
            w.saturating_sub(2),
            h.saturating_sub(2),
            RADIUS_SM,
            BG,
        );
    } else {
        d.fill_round(x, y, w, h, RADIUS_SM, BG);
    }
}

fn paint_sidebar(s: &State, h: u32, d: &mut impl Draw) {
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
    d.text(PAD, y, "Tree", DIM);
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

fn paint_side_row(
    s: &State,
    d: &mut impl Draw,
    y: u32,
    hit: SideHit,
    ic: Icon,
    ic_color: u32,
    label: &str,
) {
    let current = match hit {
        SideHit::Root => s.cwd == "/",
        SideHit::Docs => s.cwd == "/docs",
        SideHit::Vol | SideHit::Tree(_) => false,
    };
    let hovered = matches!(s.hover, Some(ChromeHover::Side(h)) if h == hit);
    if current {
        d.fill_round(PAD / 2, y, SIDE_W.saturating_sub(PAD), ROW_H, RADIUS_SM, HIGHLIGHT);
    } else if hovered {
        d.fill_round(PAD / 2, y, SIDE_W.saturating_sub(PAD), ROW_H, RADIUS_SM, HOVER);
    }
    let iy = y + (ROW_H.saturating_sub(coeleo_draw::ICON)) / 2;
    let ty = y + (ROW_H.saturating_sub(coeleo_draw::FONT_H)) / 2;
    d.icon(ic, PAD, iy, ic_color);
    d.text_elide(
        PAD + coeleo_draw::ICON + 2,
        ty,
        label,
        TEXT,
        SIDE_W.saturating_sub(PAD),
    );
}

const SIZE_COL: u32 = 48;
const DATE_COL: u32 = 148;

fn col_size_x(x: u32, w: u32) -> u32 {
    x + w.saturating_sub(SIZE_COL + DATE_COL).max(coeleo_draw::ICON + PAD * 2)
}

fn paint_columns(x: u32, w: u32, d: &mut impl Draw) {
    d.fill(x, NAV_H, w, COL_H, SURFACE);
    let ty = NAV_H + (COL_H.saturating_sub(coeleo_draw::FONT_H)) / 2;
    d.text(x + PAD / 2, ty, "Name", DIM);
    let sz_x = col_size_x(x, w);
    d.text(sz_x, ty, "Size", DIM);
    d.text(sz_x + SIZE_COL, ty, "Date", DIM);
    d.fill(x, NAV_H.saturating_add(COL_H).saturating_sub(1), w, 1, BG);
}

fn paint_row(d: &mut impl Draw, x: u32, y: u32, w: u32, ent: &DirEnt) {
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

fn paint_image(
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

fn fmt_items(n: usize) -> String {
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

fn fmt_size(n: u64) -> String {
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

fn fmt_date(t: (u16, u8, u8, u8, u8, u8)) -> String {
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

fn paint_status(s: &State, w: u32, h: u32, d: &mut impl Draw) {
    let y = h.saturating_sub(STATUS_H);
    d.fill(0, y, w, STATUS_H, SURFACE);
    let ty = y + (STATUS_H.saturating_sub(coeleo_draw::FONT_H)) / 2;
    let shown = visible(s);
    if matches!(s.mode, Mode::List) {
        if let Some(ent) = shown.get(s.sel) {
            if ent.is_dir {
                d.text(PAD, ty, &fmt_items(shown.len()), TEXT);
            } else {
                d.text_elide(PAD, ty, &ent.name, TEXT, w / 2);
                let sz = fmt_size(ent.size);
                let sx = PAD + (ent.name.len() as u32).saturating_mul(coeleo_draw::FONT_W) + PAD;
                d.text_elide(sx.min(w / 2), ty, &sz, DIM, w.saturating_sub(PAD));
            }
        } else {
            d.text(PAD, ty, &fmt_items(0), TEXT);
        }
    } else {
        d.text_elide(PAD, ty, &s.title, TEXT, w.saturating_sub(PAD));
    }
}

fn click_navbar(s: &mut State, lx: u32, w: u32) {
    match nav_hit(lx, w) {
        NavHit::Back => hist_back(s),
        NavHit::Fwd => hist_fwd(s),
        NavHit::Path => {
            s.nav = NavFocus::Path;
            let b = s.cwd.as_bytes();
            let n = b.len().min(PATH_CAP);
            s.path_edit[..n].copy_from_slice(&b[..n]);
            s.path_len = n;
        }
        NavHit::Search => s.nav = NavFocus::Search,
    }
}

fn click_sidebar(s: &mut State, ly: u32) {
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

fn visible(s: &State) -> Vec<DirEnt> {
    if s.search_len == 0 {
        return s.ents.clone();
    }
    let q = &s.search[..s.search_len];
    s.ents
        .iter()
        .filter(|e| {
            let q = core::str::from_utf8(q).unwrap_or("");
            e.name.to_ascii_lowercase().contains(&q.to_ascii_lowercase())
        })
        .cloned()
        .collect()
}

fn back_inner(s: &mut State) {
    match s.mode {
        Mode::Preview | Mode::Image => {
            s.mode = Mode::List;
            s.preview.clear();
            s.image = None;
            s.scaled = None;
            s.title = String::from("files");
        }
        Mode::List => {
            if s.cwd != "/" {
                go(s, parent(&s.cwd), true);
            }
        }
    }
}

fn hist_back(s: &mut State) {
    if s.hist_i == 0 {
        return;
    }
    s.hist_i -= 1;
    s.cwd = s.hist[s.hist_i].clone();
    reload(s);
}

fn hist_fwd(s: &mut State) {
    if s.hist_i + 1 >= s.hist.len() {
        return;
    }
    s.hist_i += 1;
    s.cwd = s.hist[s.hist_i].clone();
    reload(s);
}

fn go(s: &mut State, path: String, record: bool) {
    s.cwd = path;
    if record {
        s.hist.truncate(s.hist_i + 1);
        s.hist.push(s.cwd.clone());
        s.hist_i = s.hist.len() - 1;
    }
    s.search_len = 0;
    reload(s);
}

fn activate(s: &mut State) {
    if !matches!(s.mode, Mode::List) {
        return;
    }
    let shown = visible(s);
    let Some(ent) = shown.get(s.sel).cloned() else {
        return;
    };
    if ent.name == ".." {
        go(s, parent(&s.cwd), true);
        return;
    }
    let path = join(&s.cwd, &ent.name);
    if ent.is_dir {
        go(s, path, true);
        return;
    }
    log_name(&ent.name);
    if is_image_name(&path) {
        match load_image(&path) {
            Ok((w, h, pix)) => {
                crate::serial::write_str("fm: image\n");
                s.image = Some((w, h, pix));
                s.scaled = None;
                s.preview.clear();
                s.mode = Mode::Image;
                s.title = ent.name;
            }
            Err(()) => crate::serial::write_str("fm: not image\n"),
        }
        return;
    }
    match read_text(&path) {
        Ok(text) => {
            crate::serial::write_str(&text);
            if !text.ends_with('\n') {
                crate::serial::write_str("\n");
            }
            s.preview = text;
            s.mode = Mode::Preview;
            s.title = ent.name;
        }
        Err(FsError::NoFs) => crate::serial::write_str("fm: no filesystem\n"),
        Err(_) => crate::serial::write_str("fm: not text\n"),
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

fn load_image(path: &str) -> Result<(u32, u32, Vec<u32>), ()> {
    let bytes = fs::read_file(path).map_err(|_| ())?;
    if bytes.len() > coeleo_image::MAX_BYTES {
        return Err(());
    }
    coeleo_image::decode_rgba(&bytes)
}

fn reload(s: &mut State) {
    s.mode = Mode::List;
    s.preview.clear();
    s.image = None;
    s.scaled = None;
    s.title = String::from("files");
    s.ents.clear();
    s.sel = 0;
    s.hover = None;
    s.places_docs = fs::list("/docs").is_ok();
    match fs::list(&s.cwd) {
        Ok(ents) => {
            if s.cwd != "/" {
                s.ents.push(DirEnt {
                    name: String::from(".."),
                    is_dir: true,
                    size: 0,
                    mtime: None,
                });
            }
            s.ents.extend(ents);
        }
        Err(FsError::NoFs) => {
            crate::serial::write_str("fm: no filesystem\n");
        }
        Err(_) => {}
    }
}

fn read_text(path: &str) -> Result<String, FsError> {
    let mut out = String::new();
    fs::read_chunks(path, |bytes| {
        if out.len().saturating_add(bytes.len()) > PREVIEW_CAP {
            return Ok(());
        }
        for &b in bytes {
            if out.len() >= PREVIEW_CAP {
                break;
            }
            match b {
                b'\n' | b'\r' | b'\t' => out.push(b as char),
                0x20..=0x7E => out.push(b as char),
                _ => return Err(FsError::NotText),
            }
        }
        Ok(())
    })?;
    Ok(out)
}

fn log_name(name: &str) {
    crate::serial::write_str("fm: ");
    crate::serial::write_str(name);
    crate::serial::write_str("\n");
}

fn join(cwd: &str, name: &str) -> String {
    if cwd == "/" {
        let mut s = String::from("/");
        s.push_str(name);
        s
    } else {
        let mut s = String::from(cwd);
        s.push('/');
        s.push_str(name);
        s
    }
}

fn parent(cwd: &str) -> String {
    match cwd.rfind('/') {
        Some(0) | None => String::from("/"),
        Some(i) => String::from(&cwd[..i]),
    }
}
