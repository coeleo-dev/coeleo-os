//! Axis-aligned dirty and clip rectangles.

#[derive(Clone, Copy)]
pub(super) struct Rect {
    pub(super) x0: u32,
    pub(super) y0: u32,
    pub(super) x1: u32,
    pub(super) y1: u32,
}

pub(super) const RECT_EMPTY: Rect = Rect {
    x0: 0,
    y0: 0,
    x1: 0,
    y1: 0,
};
pub(super) fn rect_is_empty(r: Rect) -> bool {
    r.x0 >= r.x1 || r.y0 >= r.y1
}

pub(super) fn rect_intersect(a: Rect, b: Rect) -> Rect {
    Rect {
        x0: a.x0.max(b.x0),
        y0: a.y0.max(b.y0),
        x1: a.x1.min(b.x1),
        y1: a.y1.min(b.y1),
    }
}

pub(super) fn rect_union(a: Rect, b: Rect) -> Rect {
    if rect_is_empty(a) {
        return b;
    }
    if rect_is_empty(b) {
        return a;
    }
    Rect {
        x0: a.x0.min(b.x0),
        y0: a.y0.min(b.y0),
        x1: a.x1.max(b.x1),
        y1: a.y1.max(b.y1),
    }
}

pub(super) fn rect_intersects(a: Rect, b: Rect) -> bool {
    !rect_is_empty(rect_intersect(a, b))
}

pub(super) fn rect_contains(a: Rect, x: u32, y: u32) -> bool {
    x >= a.x0 && x < a.x1 && y >= a.y0 && y < a.y1
}

pub(super) fn rect_sub(a: Rect, hole: Rect) -> ([Rect; 4], usize) {
    let i = rect_intersect(a, hole);
    if rect_is_empty(i) {
        return ([a, RECT_EMPTY, RECT_EMPTY, RECT_EMPTY], 1);
    }
    let mut out = [RECT_EMPTY; 4];
    let mut n = 0usize;
    if a.y0 < i.y0 {
        out[n] = Rect {
            x0: a.x0,
            y0: a.y0,
            x1: a.x1,
            y1: i.y0,
        };
        n += 1;
    }
    if i.y1 < a.y1 {
        out[n] = Rect {
            x0: a.x0,
            y0: i.y1,
            x1: a.x1,
            y1: a.y1,
        };
        n += 1;
    }
    if a.x0 < i.x0 {
        out[n] = Rect {
            x0: a.x0,
            y0: i.y0,
            x1: i.x0,
            y1: i.y1,
        };
        n += 1;
    }
    if i.x1 < a.x1 {
        out[n] = Rect {
            x0: i.x1,
            y0: i.y0,
            x1: a.x1,
            y1: i.y1,
        };
        n += 1;
    }
    (out, n)
}
