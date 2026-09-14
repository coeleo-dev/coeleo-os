//! Dirty-rect coalesce and flush to the framebuffer.

use super::geom::{fb_rect, is_strut_rect};
use super::rect::{Rect, rect_intersect, rect_intersects, rect_is_empty, rect_union};
use super::state::{DAMAGE_CAP, State};

pub(super) fn rect_gap2(a: Rect, b: Rect) -> u64 {
    if rect_intersects(a, b) {
        return 0;
    }
    let gx = if a.x1 < b.x0 {
        u64::from(b.x0 - a.x1)
    } else if b.x1 < a.x0 {
        u64::from(a.x0 - b.x1)
    } else {
        0
    };
    let gy = if a.y1 < b.y0 {
        u64::from(b.y0 - a.y1)
    } else if b.y1 < a.y0 {
        u64::from(a.y0 - b.y1)
    } else {
        0
    };
    gx.saturating_mul(gx).saturating_add(gy.saturating_mul(gy))
}

pub(super) fn pending_remove(st: &mut State, i: usize) {
    if i >= st.n_pending {
        return;
    }
    st.n_pending -= 1;
    if i < st.n_pending {
        st.pending[i] = st.pending[st.n_pending];
    }
}

pub(super) fn coalesce_pending(st: &mut State) {
    loop {
        let mut merged = false;
        let n = st.n_pending;
        'pairs: for i in 0..n {
            for j in (i + 1)..n {
                if rect_intersects(st.pending[i], st.pending[j]) {
                    st.pending[i] = rect_union(st.pending[i], st.pending[j]);
                    pending_remove(st, j);
                    merged = true;
                    break 'pairs;
                }
            }
        }
        if !merged {
            break;
        }
    }
}

pub(super) fn pair_ok(st: &State, a: Rect, b: Rect) -> bool {
    if rect_intersects(a, b) {
        return true;
    }
    !(is_strut_rect(st, a) || is_strut_rect(st, b))
}

pub(super) fn merge_closest_pair(st: &mut State) {
    if st.n_pending < 2 {
        return;
    }
    let mut best: Option<(usize, usize, u64)> = None;
    for i in 0..st.n_pending {
        for j in (i + 1)..st.n_pending {
            let a = st.pending[i];
            let b = st.pending[j];
            if !pair_ok(st, a, b) {
                continue;
            }
            let d = rect_gap2(a, b);
            if best.map_or(true, |(_, _, bd)| d < bd) {
                best = Some((i, j, d));
            }
        }
    }
    let Some((i, j, _)) = best else {
        return;
    };
    let (lo, hi) = if i < j { (i, j) } else { (j, i) };
    st.pending[lo] = rect_union(st.pending[lo], st.pending[hi]);
    pending_remove(st, hi);
    coalesce_pending(st);
}

pub(super) fn merge_into_nearest(st: &mut State, cur: Rect) {
    let mut best: Option<(usize, u64)> = None;
    for i in 0..st.n_pending {
        let p = st.pending[i];
        if !pair_ok(st, p, cur) {
            continue;
        }
        let d = rect_gap2(p, cur);
        if best.map_or(true, |(_, bd)| d < bd) {
            best = Some((i, d));
        }
    }
    if let Some((i, _)) = best {
        st.pending[i] = rect_union(st.pending[i], cur);
        coalesce_pending(st);
    } else {
        merge_closest_pair(st);
        if st.n_pending < DAMAGE_CAP {
            st.pending[st.n_pending] = cur;
            st.n_pending += 1;
        }
    }
}

pub(super) fn note(st: &mut State, r: Rect) {
    let mut cur = rect_intersect(r, fb_rect(st));
    if rect_is_empty(cur) {
        return;
    }
    let mut i = 0;
    while i < st.n_pending {
        if rect_intersects(st.pending[i], cur) {
            cur = rect_union(st.pending[i], cur);
            pending_remove(st, i);
            i = 0;
            continue;
        }
        i += 1;
    }
    if st.n_pending < DAMAGE_CAP {
        st.pending[st.n_pending] = cur;
        st.n_pending += 1;
        return;
    }
    if is_strut_rect(st, cur) {
        merge_closest_pair(st);
        if st.n_pending < DAMAGE_CAP {
            st.pending[st.n_pending] = cur;
            st.n_pending += 1;
        } else {
            merge_into_nearest(st, cur);
        }
        return;
    }
    merge_into_nearest(st, cur);
}

pub(super) fn flush_pending(st: &mut State) {
    let n = st.n_pending;
    let rects = st.pending;
    st.n_pending = 0;
    for i in 0..n {
        flush_damage(st, rects[i]);
    }
}

pub(super) fn flush_damage(st: &State, clip: Rect) {
    let clip = rect_intersect(
        clip,
        Rect {
            x0: 0,
            y0: 0,
            x1: st.fb.w,
            y1: st.fb.h,
        },
    );
    if rect_is_empty(clip) {
        return;
    }
    let w = clip.x1.saturating_sub(clip.x0);
    let bytes = (w as usize).saturating_mul(4);
    let src_pitch = st.scene_fb.pitch;
    let dst_pitch = st.fb.pitch;
    let src = st.scene.as_ptr() as *const u8;
    let dst = st.fb.addr as *mut u8;
    for y in clip.y0..clip.y1 {
        let so = (y * src_pitch + clip.x0 * 4) as usize;
        let d = (y * dst_pitch + clip.x0 * 4) as usize;
        unsafe {
            core::ptr::copy_nonoverlapping(src.add(so), dst.add(d), bytes);
        }
    }
}
