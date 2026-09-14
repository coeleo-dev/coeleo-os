//! Kernel file-manager panel. Own cwd; no unsafe.

use alloc::string::String;
use alloc::vec::Vec;

use crate::fs::{self, DirEnt, FsError};
use coeleo_draw::{self, Icon};
use coeleo_theme::{BG, DIM, GAP, HIGHLIGHT, HOVER, PAD, RADIUS_SM, TEXT};

mod nav;
mod paint;

use nav::{
    chrome_at, click_navbar, click_sidebar, ensure_visible, go, go_parent, hover_part, typeahead,
};
use paint::{paint_columns, paint_image, paint_navbar, paint_row, paint_sidebar, paint_status};

pub const TITLE_H: u32 = 32;
pub const NAV_H: u32 = TITLE_H;
pub const SIDE_W: u32 = 160;
pub const COL_H: u32 = coeleo_draw::FONT_H + GAP;
pub const STATUS_H: u32 = coeleo_draw::FONT_H + PAD;
pub const ROW_H: u32 = coeleo_draw::FONT_H + GAP;
pub(super) const PREVIEW_CAP: usize = 4096;
pub(super) const SEARCH_CAP: usize = 24;
pub(super) const BTN: u32 = coeleo_theme::DECO_BTN;
pub(super) const PATH_CAP: usize = 96;

pub(super) enum Mode {
    List,
    Preview,
    Image,
}

pub(super) enum NavFocus {
    Path,
    Search,
    List,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum NavHit {
    Back,
    Fwd,
    Up,
    Path,
    Search,
    Crumb(u8),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum SideHit {
    Root,
    Docs,
    Vol,
    Tree(u8),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ChromeHover {
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

pub(super) struct HistEnt {
    path: String,
    sel: usize,
    scroll: usize,
}

pub(super) struct State {
    cwd: String,
    ents: Vec<DirEnt>,
    sel: usize,
    hover: Option<ChromeHover>,
    mode: Mode,
    preview: String,
    image: Option<(u32, u32, Vec<u32>)>,
    scaled: Option<(u32, u32, Vec<u32>)>,
    title: String,
    hist: Vec<HistEnt>,
    hist_i: usize,
    search: [u8; SEARCH_CAP],
    search_len: usize,
    nav: NavFocus,
    path_edit: [u8; PATH_CAP],
    path_len: usize,
    tree_open: bool,
    places_docs: bool,
    last_sel: usize,
    last_click_at: u64,
    prefix: [u8; SEARCH_CAP],
    prefix_len: usize,
    prefix_at: u64,
    scroll: usize,
    last_h: u32,
    status_msg: String,
}

pub(super) static STATE: spin::Mutex<State> = spin::Mutex::new(State {
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
    last_sel: usize::MAX,
    last_click_at: 0,
    prefix: [0; SEARCH_CAP],
    prefix_len: 0,
    prefix_at: 0,
    scroll: 0,
    last_h: 0,
    status_msg: String::new(),
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
    s.hist.push(HistEnt {
        path: String::from("/"),
        sel: 0,
        scroll: 0,
    });
    s.hist_i = 0;
    s.last_sel = usize::MAX;
    s.last_click_at = 0;
    s.prefix_len = 0;
    s.scroll = 0;
    s.status_msg.clear();
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
    fn icon_btn(&mut self, x: u32, y: u32, which: Icon, hovered: bool, enabled: bool) {
        let _ = (x, y, which, hovered, enabled);
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
        let _ = (x, y, w, h, text, placeholder, caret, search_icon);
    }
    fn crumb(&mut self, x: u32, y: u32, h: u32, label: &str, hovered: bool) -> u32 {
        let _ = (x, y, h, label, hovered);
        0
    }
    fn crumb_sep(&mut self, x: u32, y: u32, h: u32) -> u32 {
        let _ = (x, y, h);
        0
    }
    fn vsep(&mut self, x: u32, y: u32, h: u32) {
        let _ = (x, y, h);
    }
    #[allow(dead_code)]
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
        self.list_row_colored(x, y, w, h, label, icon, TEXT, selected, hovered);
    }
    fn list_row_colored(
        &mut self,
        x: u32,
        y: u32,
        w: u32,
        h: u32,
        label: &str,
        icon: Option<Icon>,
        icon_color: u32,
        selected: bool,
        hovered: bool,
    ) {
        if selected {
            self.fill_round(x, y, w, h, RADIUS_SM, HIGHLIGHT);
        } else if hovered {
            self.fill_round(x, y, w, h, RADIUS_SM, HOVER);
        }
        let mut tx = x.saturating_add(PAD / 2);
        if let Some(ic) = icon {
            self.icon(
                ic,
                tx,
                y.saturating_add(h.saturating_sub(coeleo_draw::ICON) / 2),
                icon_color,
            );
            tx = tx.saturating_add(coeleo_draw::ICON + PAD / 2);
        }
        let ty = y.saturating_add(h.saturating_sub(coeleo_draw::FONT_H) / 2);
        self.text_elide(tx, ty, label, TEXT, x.saturating_add(w));
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
    s.last_h = h;
    ensure_visible(&mut s);
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
            if shown.is_empty() {
                let msg = if s.search_len > 0 {
                    "No items match"
                } else {
                    "This folder is empty"
                };
                d.text(
                    body_x.saturating_add(PAD),
                    list_y0().saturating_add(PAD),
                    msg,
                    DIM,
                );
            } else {
                let mut y = list_y0();
                let bottom = h.saturating_sub(STATUS_H);
                for (i, ent) in shown.iter().enumerate().skip(s.scroll) {
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
    s.status_msg.clear();
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
            let vis = ((ly - list_y0()) / ROW_H) as usize;
            let row = s.scroll.saturating_add(vis);
            let shown = visible(&s);
            if row >= shown.len() {
                s.nav = NavFocus::List;
                return;
            }
            s.nav = NavFocus::List;
            let t = crate::clock::ticks();
            let dbl = row == s.last_sel && t.saturating_sub(s.last_click_at) < 40;
            s.sel = row;
            s.last_sel = row;
            s.last_click_at = t;
            if dbl {
                activate(&mut s);
            }
        }
        Mode::Preview | Mode::Image => back_inner(&mut s),
    }
}

pub fn click_row(row: i32) {
    if row < 0 {
        return;
    }
    let mut s = STATE.lock();
    s.status_msg.clear();
    match s.mode {
        Mode::List => {
            let shown = visible(&s);
            let row = row as usize;
            if row >= shown.len() {
                return;
            }
            let t = crate::clock::ticks();
            let dbl = row == s.last_sel && t.saturating_sub(s.last_click_at) < 40;
            s.sel = row;
            s.last_sel = row;
            s.last_click_at = t;
            s.nav = NavFocus::List;
            if dbl {
                activate(&mut s);
            }
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

pub(super) const ROW_INSET: u32 = 4;

pub fn up() {
    let mut s = STATE.lock();
    s.status_msg.clear();
    if !matches!(s.mode, Mode::List) {
        return;
    }
    if s.sel > 0 {
        s.sel -= 1;
        ensure_visible(&mut s);
    }
}

pub fn down() {
    let mut s = STATE.lock();
    s.status_msg.clear();
    if !matches!(s.mode, Mode::List) {
        return;
    }
    let n = visible(&s).len();
    if n != 0 && s.sel + 1 < n {
        s.sel += 1;
        ensure_visible(&mut s);
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
    s.status_msg.clear();
    match s.nav {
        NavFocus::Path => {
            if s.path_len > 0 {
                s.path_len -= 1;
            } else {
                s.nav = NavFocus::List;
            }
            return;
        }
        NavFocus::Search => {
            if s.search_len > 0 && matches!(s.mode, Mode::List) {
                s.search_len -= 1;
                s.sel = 0;
                s.scroll = 0;
                return;
            }
            s.nav = NavFocus::List;
            return;
        }
        NavFocus::List => {}
    }
    back_inner(&mut s);
}

pub fn search_push(c: u8) {
    if !(0x20..=0x7E).contains(&c) {
        return;
    }
    let mut s = STATE.lock();
    s.status_msg.clear();
    match s.nav {
        NavFocus::Path => {
            if s.path_len < PATH_CAP {
                let i = s.path_len;
                s.path_edit[i] = c;
                s.path_len += 1;
            }
        }
        NavFocus::Search => {
            if !matches!(s.mode, Mode::List) {
                return;
            }
            if s.search_len < SEARCH_CAP {
                let i = s.search_len;
                s.search[i] = c;
                s.search_len += 1;
                s.sel = 0;
                s.scroll = 0;
            }
        }
        NavFocus::List => {
            if matches!(s.mode, Mode::List) {
                typeahead(&mut s, c);
            }
        }
    }
}

pub fn enter() {
    let mut s = STATE.lock();
    s.status_msg.clear();
    if matches!(s.nav, NavFocus::Path) {
        let p = String::from(core::str::from_utf8(&s.path_edit[..s.path_len]).unwrap_or(""));
        if fs::list(&p).is_ok() {
            go(&mut s, p, true);
            s.nav = NavFocus::List;
        } else {
            s.status_msg = String::from("Path not found");
            crate::serial::write_str("fm: no such path\n");
        }
        return;
    }
    activate(&mut s);
}

pub fn copy_path() {
    let mut g = STATE.lock();
    let st = &mut *g;
    let n = visible(st).len();
    if n == 0 || st.sel >= n {
        return;
    }
    let e = visible(st)[st.sel].clone();
    drop(g);
    let mut buf = String::new();
    let st_g = STATE.lock();
    if st_g.cwd != "/" {
        buf.push_str(&st_g.cwd);
        buf.push('/');
    } else {
        buf.push('/');
    }
    buf.push_str(&e.name);
    crate::ui::comp::clip::sys_clipboard(1, buf.as_ptr() as u64, buf.len() as u64);
}

pub fn esc() {
    let mut s = STATE.lock();
    match s.mode {
        Mode::Preview | Mode::Image => {
            s.status_msg.clear();
            back_inner(&mut s);
        }
        Mode::List => match s.nav {
            NavFocus::Search if s.search_len > 0 => {
                s.search_len = 0;
                s.sel = 0;
                s.scroll = 0;
                s.status_msg.clear();
            }
            NavFocus::Search | NavFocus::Path => {
                s.nav = NavFocus::List;
                s.status_msg.clear();
            }
            NavFocus::List => {}
        },
    }
}

pub fn delete_sel() {
    let mut s = STATE.lock();
    s.status_msg.clear();
    if !matches!(s.mode, Mode::List) {
        return;
    }
    let shown = visible(&s);
    let Some(ent) = shown.get(s.sel).cloned() else {
        s.status_msg = String::from("Nothing to delete");
        return;
    };
    if ent.is_dir {
        s.status_msg = String::from("Cannot delete folder");
        crate::serial::write_str("fm: not a file\n");
        return;
    }
    let path = join(&s.cwd, &ent.name);
    match fs::remove(&path) {
        Ok(()) => {
            crate::serial::write_str("fm: deleted ");
            crate::serial::write_str(&ent.name);
            crate::serial::write_str("\n");
            let keep = s.sel;
            reload(&mut s);
            let n = s.ents.len();
            if n == 0 {
                s.sel = 0;
            } else {
                s.sel = keep.min(n - 1);
            }
            ensure_visible(&mut s);
        }
        Err(_) => {
            s.status_msg = String::from("Cannot delete folder");
            crate::serial::write_str("fm: not a file\n");
        }
    }
}

pub fn right_click_at(lx: u32, ly: u32, w: u32) -> bool {
    let _ = w;
    let mut s = STATE.lock();
    if !matches!(s.mode, Mode::List) {
        return false;
    }
    if ly < list_y0() || lx < SIDE_W {
        return false;
    }
    let vis = ((ly - list_y0()) / ROW_H) as usize;
    let row = s.scroll.saturating_add(vis);
    let shown = visible(&s);
    if row >= shown.len() {
        return false;
    }
    s.sel = row;
    s.nav = NavFocus::List;
    true
}

pub struct MenuFlags {
    pub can_open: bool,
    pub can_up: bool,
    pub can_delete: bool,
}

pub fn menu_flags() -> MenuFlags {
    let s = STATE.lock();
    let shown = visible(&s);
    let ent = shown.get(s.sel);
    MenuFlags {
        can_open: ent.is_some(),
        can_up: s.cwd != "/",
        can_delete: ent.is_some_and(|e| !e.is_dir),
    }
}

pub fn menu_open() {
    let mut s = STATE.lock();
    s.status_msg.clear();
    activate(&mut s);
}

pub fn menu_go_up() {
    let mut s = STATE.lock();
    s.status_msg.clear();
    go_parent(&mut s);
}

pub fn row_client_y(row: usize) -> Option<u32> {
    let s = STATE.lock();
    if row < s.scroll {
        return None;
    }
    Some(list_y0() + ((row - s.scroll) as u32).saturating_mul(ROW_H))
}

pub(super) fn visible(s: &State) -> Vec<DirEnt> {
    if s.search_len == 0 {
        return s.ents.clone();
    }
    let q = &s.search[..s.search_len];
    s.ents
        .iter()
        .filter(|e| {
            let q = core::str::from_utf8(q).unwrap_or("");
            e.name
                .to_ascii_lowercase()
                .contains(&q.to_ascii_lowercase())
        })
        .cloned()
        .collect()
}

pub(super) fn back_inner(s: &mut State) {
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

pub(super) fn activate(s: &mut State) {
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

pub(super) fn reload(s: &mut State) {
    s.mode = Mode::List;
    s.preview.clear();
    s.image = None;
    s.scaled = None;
    s.title = String::from("files");
    s.ents.clear();
    s.sel = 0;
    s.scroll = 0;
    s.hover = None;
    s.places_docs = fs::list("/docs").is_ok();
    match fs::list(&s.cwd) {
        Ok(ents) => {
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

pub(super) fn join(cwd: &str, name: &str) -> String {
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

pub(super) fn parent(cwd: &str) -> String {
    match cwd.rfind('/') {
        Some(0) | None => String::from("/"),
        Some(i) => String::from(&cwd[..i]),
    }
}
