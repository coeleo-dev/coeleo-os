//! Kernel desktop settings: panel mode and wallpaper gallery with thumbnail previews.
//! No compositor lock.

use alloc::string::String;
use alloc::vec::Vec;

use crate::desk;
use super::deskset_thumbs::{self, THUMB_H, THUMB_W};
use crate::fm;
use crate::fs;
use crate::panel;
use coeleo_theme::{
    ACCENT, BG, BORDER, DIM, HIGHLIGHT, PAD, RADIUS_SM, SURFACE, SURFACE_RAISED, TEXT,
};

/// Layout rhythm. Kept together because paint and hit-test must never disagree.
const BTN_H: u32 = 24;
const NAV_H: u32 = 24;
const NAV_BTN_W: u32 = 100;
/// Horizontal window margin, both sides.
const OUTER: u32 = 14;
/// One text row; also the height of a section heading.
const LABEL_H: u32 = coeleo_draw::FONT_H;
/// Breathing room between a section heading and the controls it labels.
const LABEL_GAP: u32 = 10;
/// Breathing room between two sections.
const SECTION_GAP: u32 = 14;
const GRID_GAP_X: u32 = 12;
const GRID_GAP_Y: u32 = 12;
/// Space between a thumbnail and its title.
const THUMB_GAP: u32 = 6;
/// Smallest card that still fits the thumbnail plus one title row.
const MIN_CARD_H: u32 = 96;
/// Taller than this and the card reads as empty.
const MAX_CARD_H: u32 = 132;
/// Text of the badge marking the wallpaper currently in use.
const ACTIVE_LABEL: &str = "Active";
/// Horizontal margin inside the active badge. The glyph ink sits ~3px inside
/// the font box, so a `FONT_H + 2` pill ends up with this much visible padding
/// on all four sides.
const BADGE_PAD: u32 = 4;
const BADGE_H: u32 = coeleo_draw::FONT_H + 2;
const PREV_LABEL: &str = "< Previous";
const NEXT_LABEL: &str = "Next >";
const COLS: usize = 3;
const ROWS: usize = 2;
const ITEMS_PER_PAGE: usize = COLS * ROWS; // 6 items per page

#[derive(Clone, PartialEq, Eq)]
pub enum WallItem {
    Default,
    Path(String),
}

struct State {
    walls: Vec<WallItem>,
    wall_sel: usize,
    page: usize,
    thumbs_cache: Vec<Option<Vec<u32>>>,
}

static STATE: spin::Mutex<State> = spin::Mutex::new(State {
    walls: Vec::new(),
    wall_sel: 0,
    page: 0,
    thumbs_cache: Vec::new(),
});

pub enum Action {
    None,
    SetMode(panel::Mode),
    SetWallDefault,
    SetWallPath(String),
}

/// Every rectangle the settings window uses. `render` and `click_at` both read
/// this, so changing the spacing cannot desync the paint from the hit-test.
struct Layout {
    mode_y: u32,
    btn_w: u32,
    divider_y: u32,
    grid_label_y: u32,
    grid_y0: u32,
    card_w: u32,
    card_h: u32,
    nav_y: u32,
}

fn page_count(walls: usize) -> usize {
    ((walls + ITEMS_PER_PAGE - 1) / ITEMS_PER_PAGE).max(1)
}

fn layout(w: u32, h: u32) -> Layout {
    let mode_y = PAD + LABEL_H + LABEL_GAP;
    let divider_y = mode_y + BTN_H + SECTION_GAP;
    let grid_label_y = divider_y + 1 + SECTION_GAP;
    let grid_y0 = grid_label_y + LABEL_H + LABEL_GAP;

    let btn_w = w.saturating_sub(OUTER * 2).saturating_sub(GRID_GAP_X) / 2;
    let card_w = w
        .saturating_sub(OUTER * 2)
        .saturating_sub((COLS as u32 - 1) * GRID_GAP_X)
        / COLS as u32;

    // Rows stretch to fill the gap between the grid top and the nav bar, so the
    // window has no dead space. The clamps keep the title legible and the card
    // from reading as empty.
    let nav_y = h.saturating_sub(OUTER).saturating_sub(NAV_H);
    let free = nav_y
        .saturating_sub(grid_y0)
        .saturating_sub(GRID_GAP_Y)
        .saturating_sub((ROWS as u32 - 1) * GRID_GAP_Y);
    let card_h = (free / ROWS as u32).clamp(MIN_CARD_H, MAX_CARD_H);

    Layout {
        mode_y,
        btn_w,
        divider_y,
        grid_label_y,
        grid_y0,
        card_w,
        card_h,
        nav_y,
    }
}

pub fn refresh() {
    let mut s = STATE.lock();
    s.walls.clear();
    s.thumbs_cache.clear();

    // 1. Default embedded wallpaper
    s.walls.push(WallItem::Default);

    // 2. Scan disk folders for wallpapers
    push_images(&mut s.walls, "/wallpapers");
    push_images(&mut s.walls, "/docs/wallpapers");
    push_images(&mut s.walls, "/docs");
    push_images(&mut s.walls, "/");

    // Deduplicate any repeated paths
    dedup_walls(&mut s.walls);

    // Sort items after Default so gallery order is consistent
    if s.walls.len() > 2 {
        s.walls[1..].sort_by(|a, b| {
            let ta = title_for_item(a);
            let tb = title_for_item(b);
            ta.cmp(&tb)
        });
    }

    // Initialize thumbnail cache
    let num_walls = s.walls.len();
    s.thumbs_cache.resize(num_walls, None);

    // Synchronize selection with active desk wallpaper
    let cur = desk::wall_src();
    if let Some(pos) = s.walls.iter().position(|it| match (it, &cur) {
        (WallItem::Default, desk::WallSrc::Default) => true,
        (WallItem::Path(p1), desk::WallSrc::Path(p2)) => p1 == p2,
        _ => false,
    }) {
        s.wall_sel = pos;
        s.page = pos / ITEMS_PER_PAGE;
    } else {
        if s.wall_sel >= s.walls.len() {
            s.wall_sel = 0;
        }
        s.page = s.wall_sel / ITEMS_PER_PAGE;
    }
    s.page = s.page.min(page_count(s.walls.len()) - 1);
}

fn dedup_walls(walls: &mut Vec<WallItem>) {
    let mut i = 0;
    while i < walls.len() {
        let mut j = i + 1;
        while j < walls.len() {
            if walls[i] == walls[j] {
                walls.remove(j);
            } else {
                j += 1;
            }
        }
        i += 1;
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

pub fn title_for_item(item: &WallItem) -> String {
    match item {
        WallItem::Default => String::from("Default"),
        WallItem::Path(p) => {
            if let Some(entry) = deskset_thumbs::find_thumb(p) {
                String::from(entry.title)
            } else {
                let filename = p.rsplit('/').next().unwrap_or(p.as_str());
                let clean = filename
                    .strip_prefix("wallpaper_")
                    .unwrap_or(filename)
                    .strip_suffix(".png")
                    .or_else(|| filename.strip_suffix(".jpg"))
                    .unwrap_or(filename);
                clean.replace('_', " ")
            }
        }
    }
}

fn ensure_thumb(s: &mut State, idx: usize) -> Option<&[u32]> {
    let num_walls = s.walls.len();
    if idx >= num_walls {
        return None;
    }
    if s.thumbs_cache.len() <= idx {
        s.thumbs_cache.resize(num_walls, None);
    }
    if s.thumbs_cache[idx].is_none() {
        let item = s.walls[idx].clone();
        let decoded = match &item {
            WallItem::Default => {
                let entry = deskset_thumbs::find_thumb("default")?;
                coeleo_image::decode_rgba(entry.jpeg).ok().map(|(_, _, pix)| pix)
            }
            WallItem::Path(path) => {
                if let Some(entry) = deskset_thumbs::find_thumb(path) {
                    coeleo_image::decode_rgba(entry.jpeg).ok().map(|(_, _, pix)| pix)
                } else if let Ok(bytes) = fs::read_file(path) {
                    coeleo_image::decode_rgba(&bytes).ok().and_then(|(w, h, pix)| {
                        coeleo_image::scale_box(&pix, w, h, THUMB_W, THUMB_H).ok()
                    })
                } else {
                    None
                }
            }
        };
        s.thumbs_cache[idx] = decoded;
    }
    s.thumbs_cache[idx].as_deref()
}

pub fn render(w: u32, h: u32, d: &mut impl fm::Draw) {
    let mut s = STATE.lock();
    d.fill(0, 0, w, h, BG);
    let lay = layout(w, h);

    // --- Section 1: Taskbar Style ---
    d.text(OUTER, PAD, "Taskbar Style", TEXT);
    let txt_y = lay.mode_y + (BTN_H.saturating_sub(coeleo_draw::FONT_H)) / 2;
    let float_on = panel::mode() == panel::Mode::Float;

    // Float Button
    let btn_bg1 = if float_on { HIGHLIGHT } else { SURFACE };
    d.fill_round(OUTER, lay.mode_y, lay.btn_w, BTN_H, RADIUS_SM, btn_bg1);
    text_centered(d, OUTER, lay.btn_w, txt_y, "Float", TEXT);

    // Full Button
    let x_full = OUTER + lay.btn_w + GRID_GAP_X;
    let btn_bg2 = if !float_on { HIGHLIGHT } else { SURFACE };
    d.fill_round(x_full, lay.mode_y, lay.btn_w, BTN_H, RADIUS_SM, btn_bg2);
    text_centered(d, x_full, lay.btn_w, txt_y, "Full", TEXT);

    // Divider
    d.fill(OUTER, lay.divider_y, w.saturating_sub(OUTER * 2), 1, BORDER);

    // --- Section 2: Desktop Wallpapers Header ---
    d.text(OUTER, lay.grid_label_y, "Wallpapers", TEXT);
    let total_walls = s.walls.len();
    let total_pages = page_count(total_walls);
    // Painting never mutates state; the page is clamped here for display only.
    let page = s.page.min(total_pages - 1);
    let grid_y0 = lay.grid_y0;

    // Current active wallpaper from desk
    let active_src = desk::wall_src();

    // --- Section 3: Wallpaper Cards Grid (3 cols x 2 rows) ---
    let page_start = page * ITEMS_PER_PAGE;
    let col_w = lay.card_w;
    let col_h = lay.card_h;
    // Centre the thumbnail + title block vertically inside the card.
    let block_h = THUMB_H + THUMB_GAP + LABEL_H;

    for row in 0..ROWS {
        for col in 0..COLS {
            let item_idx = page_start + row * COLS + col;
            if item_idx >= total_walls {
                continue;
            }

            let cx = OUTER + col as u32 * (col_w + GRID_GAP_X);
            let cy = grid_y0 + row as u32 * (col_h + GRID_GAP_Y);

            let is_selected = item_idx == s.wall_sel;
            let is_active = match (&s.walls[item_idx], &active_src) {
                (WallItem::Default, desk::WallSrc::Default) => true,
                (WallItem::Path(p1), desk::WallSrc::Path(p2)) => p1 == p2,
                _ => false,
            };

            // Card background & highlight border
            if is_selected {
                // 2px blue accent border
                d.fill_round(cx, cy, col_w, col_h, RADIUS_SM, ACCENT);
                d.fill_round(cx + 2, cy + 2, col_w - 4, col_h - 4, RADIUS_SM, SURFACE_RAISED);
            } else {
                d.fill_round(cx, cy, col_w, col_h, RADIUS_SM, SURFACE);
            }

            // Blit thumbnail preview, centred horizontally and vertically
            // together with its title.
            let thumb_x = cx + (col_w.saturating_sub(THUMB_W)) / 2;
            let thumb_y = cy + (col_h.saturating_sub(block_h)) / 2;

            if let Some(pix) = ensure_thumb(&mut s, item_idx) {
                d.blit(thumb_x, thumb_y, pix, THUMB_W, THUMB_H);
            } else {
                // Fallback placeholder box
                d.fill(thumb_x, thumb_y, THUMB_W, THUMB_H, BG);
                let ph_y = thumb_y + (THUMB_H.saturating_sub(coeleo_draw::FONT_H)) / 2;
                text_centered(d, thumb_x, THUMB_W, ph_y, "Image", DIM);
            }

            // Border around thumbnail
            let frame_color = if is_selected { ACCENT } else { BORDER };
            d.fill(thumb_x, thumb_y, THUMB_W, 1, frame_color);
            d.fill(thumb_x, thumb_y + THUMB_H - 1, THUMB_W, 1, frame_color);
            d.fill(thumb_x, thumb_y, 1, THUMB_H, frame_color);
            d.fill(thumb_x + THUMB_W - 1, thumb_y, 1, THUMB_H, frame_color);

            // Active badge in top right of thumbnail. The pill is sized from the
            // text's visible ink and placed by its left bearing, so the label
            // ends up with equal padding instead of hugging the far corner.
            if is_active {
                let (ilo, ihi) = coeleo_draw::text_ink(ACTIVE_LABEL);
                let badge_w = ihi.saturating_sub(ilo) + 2 * BADGE_PAD;
                let bx = thumb_x + THUMB_W.saturating_sub(badge_w + 3);
                let by = thumb_y + 3;
                d.fill_round(bx, by, badge_w, BADGE_H, 3, ACCENT);
                let tx = bx.saturating_add(BADGE_PAD).saturating_sub(ilo);
                let ty = by + (BADGE_H.saturating_sub(coeleo_draw::FONT_H)) / 2;
                d.text(tx, ty, ACTIVE_LABEL, TEXT);
            }

            // Title below thumbnail
            let title = title_for_item(&s.walls[item_idx]);
            let title_y = thumb_y + THUMB_H + THUMB_GAP;
            let title_color = if is_selected { TEXT } else { DIM };
            d.text_elide(cx + 8, title_y, &title, title_color, cx + col_w - 8);
        }
    }

    // --- Section 4: Bottom Navigation Bar ---
    let nav_y = lay.nav_y;
    let nav_txt_y = nav_y + (NAV_H.saturating_sub(coeleo_draw::FONT_H)) / 2;
    let has_prev = page > 0;

    // Previous Button
    let prev_bg = if has_prev { SURFACE_RAISED } else { BG };
    let prev_text_c = if has_prev { TEXT } else { DIM };
    d.fill_round(OUTER, nav_y, NAV_BTN_W, NAV_H, RADIUS_SM, prev_bg);
    text_centered(d, OUTER, NAV_BTN_W, nav_txt_y, PREV_LABEL, prev_text_c);

    // Page indicator (Centered)
    let page_str = alloc::format!("Page {} of {}", page + 1, total_pages);
    text_centered(d, 0, w, nav_txt_y, &page_str, TEXT);

    // Next Button
    let has_next = page + 1 < total_pages;
    let next_bg = if has_next { SURFACE_RAISED } else { BG };
    let next_text_c = if has_next { TEXT } else { DIM };
    let next_x = w.saturating_sub(OUTER + NAV_BTN_W);
    d.fill_round(next_x, nav_y, NAV_BTN_W, NAV_H, RADIUS_SM, next_bg);
    text_centered(d, next_x, NAV_BTN_W, nav_txt_y, NEXT_LABEL, next_text_c);
}

/// Centre `s` in `[x, x + w)` by its visible ink. `text_width` includes the
/// glyph side bearings, so centring on it leaves a label a couple of pixels off.
fn text_centered(d: &mut impl fm::Draw, x: u32, w: u32, y: u32, s: &str, color: u32) {
    let (ilo, ihi) = coeleo_draw::text_ink(s);
    let tx = x.saturating_add(w.saturating_sub(ihi.saturating_sub(ilo)) / 2);
    d.text(tx.saturating_sub(ilo), y, s, color);
}

pub fn click_at(lx: u32, ly: u32, w: u32, h: u32) -> Action {
    // NOTE: no refresh() here — it would reset page/wall_sel to the active
    // wallpaper on every click, so a page-2 click would jump back to page 1.
    let lay = layout(w, h);

    // 1. Taskbar mode buttons
    if ly >= lay.mode_y && ly < lay.mode_y + BTN_H {
        if lx >= OUTER && lx < OUTER + lay.btn_w {
            return Action::SetMode(panel::Mode::Float);
        }
        let x_full = OUTER + lay.btn_w + GRID_GAP_X;
        if lx >= x_full && lx < x_full + lay.btn_w {
            return Action::SetMode(panel::Mode::Full);
        }
        return Action::None;
    }

    let mut s = STATE.lock();
    let total_walls = s.walls.len();
    let total_pages = page_count(total_walls);

    // 2. Navigation buttons
    if ly >= lay.nav_y && ly < lay.nav_y + NAV_H {
        if lx >= OUTER && lx < OUTER + NAV_BTN_W {
            if s.page > 0 {
                s.page -= 1;
            }
            return Action::None;
        }
        let next_x = w.saturating_sub(OUTER + NAV_BTN_W);
        if lx >= next_x && lx < next_x + NAV_BTN_W {
            if s.page + 1 < total_pages {
                s.page += 1;
            }
            return Action::None;
        }
        return Action::None;
    }

    // 3. Wallpaper Card Grid. The gaps between cards are dead zones: a click
    // there must not select a neighbour.
    let Some(rel_y) = ly.checked_sub(lay.grid_y0) else {
        return Action::None;
    };
    let row_pitch = lay.card_h + GRID_GAP_Y;
    let row = (rel_y / row_pitch) as usize;
    if row >= ROWS || rel_y - row as u32 * row_pitch >= lay.card_h {
        return Action::None;
    }

    let Some(rel_x) = lx.checked_sub(OUTER) else {
        return Action::None;
    };
    let col_pitch = lay.card_w + GRID_GAP_X;
    let col = (rel_x / col_pitch) as usize;
    if col >= COLS || rel_x - col as u32 * col_pitch >= lay.card_w {
        return Action::None;
    }

    let item_idx = s.page * ITEMS_PER_PAGE + row * COLS + col;
    if item_idx >= total_walls {
        return Action::None;
    }

    s.wall_sel = item_idx;
    match s.walls.get(item_idx) {
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
    // NOTE: no refresh() here — see click_at; keeps page/wall_sel between keys.
    let mut s = STATE.lock();
    let total_walls = s.walls.len();
    if total_walls == 0 {
        return Action::None;
    }

    match k {
        KEY_LEFT => {
            if s.wall_sel > 0 {
                s.wall_sel -= 1;
                s.page = s.wall_sel / ITEMS_PER_PAGE;
            }
            Action::None
        }
        KEY_RIGHT => {
            if s.wall_sel + 1 < total_walls {
                s.wall_sel += 1;
                s.page = s.wall_sel / ITEMS_PER_PAGE;
            }
            Action::None
        }
        KEY_UP => {
            if s.wall_sel >= COLS {
                s.wall_sel -= COLS;
                s.page = s.wall_sel / ITEMS_PER_PAGE;
            }
            Action::None
        }
        KEY_DOWN => {
            if s.wall_sel + COLS < total_walls {
                s.wall_sel += COLS;
                s.page = s.wall_sel / ITEMS_PER_PAGE;
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
