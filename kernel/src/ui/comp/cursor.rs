//! Software cursor sprite.

use super::damage::{flush_damage, flush_pending};
use super::fb::put_fb_i;
use super::rect::{RECT_EMPTY, Rect, rect_is_empty};
use super::state::State;

use coeleo_theme::{SHADOW, TEXT};

pub(super) const CURSOR_W: u32 = 16;
pub(super) const CURSOR_H: u32 = 20;

pub(super) const CURSOR: [u32; 20] = [
    0b1000_0000_0000_0000,
    0b1100_0000_0000_0000,
    0b1110_0000_0000_0000,
    0b1111_0000_0000_0000,
    0b1111_1000_0000_0000,
    0b1111_1100_0000_0000,
    0b1111_1110_0000_0000,
    0b1111_1111_0000_0000,
    0b1111_1111_1000_0000,
    0b1111_1111_1100_0000,
    0b1111_1110_0000_0000,
    0b1110_1110_0000_0000,
    0b1100_0111_0000_0000,
    0b1000_0111_0000_0000,
    0b0000_0011_1000_0000,
    0b0000_0011_1000_0000,
    0b0000_0001_1100_0000,
    0b0000_0001_1100_0000,
    0b0000_0000_1110_0000,
    0b0000_0000_1110_0000,
];
pub(super) fn undraw_cursor(st: &mut State) {
    if !st.drawn {
        return;
    }
    st.cursor_old = cursor_rect(st);
    st.drawn = false;
}

pub(super) fn draw_cursor(st: &mut State) {
    let had = st.n_pending > 0;
    flush_pending(st);
    if !st.drawn {
        if !rect_is_empty(st.cursor_old) {
            flush_damage(st, st.cursor_old);
            st.cursor_old = RECT_EMPTY;
        }
        flush_damage(st, cursor_rect(st));
        paint_sprite(st);
        st.drawn = true;
        return;
    }
    if !had {
        return;
    }
    flush_damage(st, cursor_rect(st));
    paint_sprite(st);
}

pub(super) fn cursor_rect(st: &State) -> Rect {
    let x0 = st.cx.max(0) as u32;
    let y0 = st.cy.max(0) as u32;
    Rect {
        x0,
        y0,
        x1: x0.saturating_add(CURSOR_W).min(st.fb.w),
        y1: y0.saturating_add(CURSOR_H).min(st.fb.h),
    }
}
pub(super) fn paint_sprite(st: &mut State) {
    for row in 0..CURSOR_H {
        let bits = CURSOR[row as usize];
        for col in 0..CURSOR_W {
            let shift = CURSOR_W - 1 - col;
            if bits & (1 << shift) == 0 {
                continue;
            }
            let color = if is_edge(row, col) {
                SHADOW.to_le_bytes()
            } else {
                TEXT.to_le_bytes()
            };
            put_fb_i(&st.fb, st.cx + col as i32, st.cy + row as i32, color);
        }
    }
}

fn filled(row: i32, col: i32) -> bool {
    if row < 0 || col < 0 || row >= CURSOR_H as i32 || col >= CURSOR_W as i32 {
        return false;
    }
    let shift = CURSOR_W - 1 - col as u32;
    CURSOR[row as usize] & (1 << shift) != 0
}

pub(super) fn is_edge(row: u32, col: u32) -> bool {
    if !filled(row as i32, col as i32) {
        return false;
    }
    let r = row as i32;
    let c = col as i32;
    !filled(r, c - 1) || !filled(r, c + 1) || !filled(r - 1, c) || !filled(r + 1, c)
}
