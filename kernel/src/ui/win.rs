//! Client surfaces: bounce buffer in the kernel. mmap is appendix A, not this phase.
//!
//! Mouse clicks are *copied* into `poll_input` (surface-local). The compositor
//! still owns cursor and focus.

use alloc::vec::Vec;
use core::mem::size_of;

use spin::Mutex;

use crate::fbterm;
use crate::fd;
use crate::panel;
use crate::serial;
use crate::vmm;
use coeleo_theme::{DECO_H, SHADOW_PX};

const ERR: u64 = u64::MAX;
const MAX_SURFACES: usize = 8;
const MAX_DIM: u32 = 256;
const ORIGIN_X: u32 = 16;
const ORIGIN_Y: u32 = 8;
const KIND_MOVE: u32 = 1;
const KIND_DOWN: u32 = 2;
const KIND_KEY: u32 = 3;
const IN_CAP: usize = 8;
const EV_SIZE: usize = 16;

const _: () = assert!(size_of::<InputEv>() == EV_SIZE);

/// Packed 16-byte event copied to userspace by [`sys_poll_input`].
#[repr(C)]
#[derive(Clone, Copy)]
pub struct InputEv {
    pub kind: u32,
    pub x: i32,
    pub y: i32,
    pub key: u32,
}

#[derive(Clone, Copy)]
struct Queued {
    pid: u32,
    ev: InputEv,
}

struct Surface {
    id: u32,
    pid: u32,
    w: u32,
    h: u32,
    ox: u32,
    oy: u32,
    buf: Vec<u32>,
    user_ptr: u64,
    dirty: Option<(u32, u32, u32, u32)>,
}

struct Win {
    slots: [Option<Surface>; MAX_SURFACES],
    q: [Queued; IN_CAP],
    qhead: usize,
    qtail: usize,
}

static WIN: Mutex<Win> = Mutex::new(Win {
    slots: [const { None }; MAX_SURFACES],
    q: [Queued {
        pid: 0,
        ev: InputEv {
            kind: 0,
            x: 0,
            y: 0,
            key: 0,
        },
    }; IN_CAP],
    qhead: 0,
    qtail: 0,
});

pub struct DirtyFrame {
    pub sx: u32,
    pub sy: u32,
    pub w: u32,
    pub h: u32,
    pub pixels: Vec<u32>,
}

pub struct ClientSnap {
    pub id: u32,
    pub ox: u32,
    pub oy: u32,
    pub w: u32,
    pub h: u32,
    pub pixels: Vec<u32>,
}

pub fn sys_create(w: u64, h: u64, buf: u64) -> u64 {
    if !crate::comp::is_ready() {
        return ERR;
    }
    let Some(pid) = crate::sched::current_pid() else {
        return ERR;
    };
    let w = w as u32;
    let h = h as u32;
    if w == 0 || h == 0 || w > MAX_DIM || h > MAX_DIM {
        return ERR;
    }
    let nbytes = (w as u64).saturating_mul(h as u64).saturating_mul(4);
    if nbytes == 0 || !vmm::user_slice_ok(buf, nbytes) {
        return ERR;
    }
    let npx = (w as usize).saturating_mul(h as usize);
    let mut pixels = Vec::new();
    if pixels.try_reserve(npx).is_err() {
        return ERR;
    }
    pixels.resize(npx, 0);
    if copy_pixels(buf, &mut pixels).is_err() {
        return ERR;
    }
    let (ox, oy) = place(w, h);
    let mut g = WIN.lock();
    let free_slot = g.slots.iter().position(|s| s.is_none());
    if free_slot.is_none() {
        return ERR;
    }
    let slot = free_slot.unwrap();
    let id = slot as u32 + 1;
    g.slots[slot] = Some(Surface {
        id,
        pid,
        w,
        h,
        ox,
        oy,
        buf: pixels,
        user_ptr: buf,
        dirty: None,
    });
    log_create(id);
    drop(g);
    crate::comp::on_client_create(id, w, h, ox, oy);
    id as u64
}

pub fn sys_damage(id: u64, xy: u64, wh: u64) -> u64 {
    let id = id as u32;
    let x = xy as u32;
    let y = (xy >> 32) as u32;
    let w = wh as u32;
    let h = (wh >> 32) as u32;
    let Some(pid) = crate::sched::current_pid() else {
        return ERR;
    };
    let mut g = WIN.lock();
    let Some(surf) = g
        .slots
        .iter_mut()
        .flatten()
        .find(|s| s.id == id && s.pid == pid)
    else {
        return ERR;
    };
    let Some((x, y, w, h)) = clip(surf.w, surf.h, x, y, w, h) else {
        return ERR;
    };
    let nbytes = (surf.w as u64)
        .saturating_mul(surf.h as u64)
        .saturating_mul(4);
    if !vmm::user_slice_ok(surf.user_ptr, nbytes) {
        return ERR;
    }
    if copy_pixels(surf.user_ptr, &mut surf.buf).is_err() {
        return ERR;
    }
    surf.dirty = Some((x, y, w, h));
    0
}

/// Copy queued events for the current process. Does not consume compositor mouse.
pub fn sys_poll_input(buf: u64, len: u64) -> u64 {
    let Some(pid) = crate::sched::current_pid() else {
        return 0;
    };
    if len < EV_SIZE as u64 {
        return 0;
    }
    let nfit = ((len as usize) / EV_SIZE).min(IN_CAP);
    let nbytes = nfit * EV_SIZE;
    if !vmm::user_slice_ok(buf, nbytes as u64) {
        return ERR;
    }
    let mut tmp = [0u8; IN_CAP * EV_SIZE];
    let copied = {
        let mut g = WIN.lock();
        g.take_for_pid(pid, &mut tmp, nfit)
    };
    if copied == 0 {
        return 0;
    }
    if fd::copy_to_user(buf, &tmp[..copied]).is_err() {
        return ERR;
    }
    copied as u64
}

pub fn push_client_down(id: u32, lx: i32, ly: i32) {
    let mut g = WIN.lock();
    let Some(pid) = g.slots.iter().flatten().find(|s| s.id == id).map(|s| s.pid) else {
        return;
    };
    g.push_ev(
        pid,
        InputEv {
            kind: KIND_DOWN,
            x: lx,
            y: ly,
            key: 0,
        },
    );
}

pub fn push_client_move(id: u32, lx: i32, ly: i32) {
    let mut g = WIN.lock();
    let Some(pid) = g.slots.iter().flatten().find(|s| s.id == id).map(|s| s.pid) else {
        return;
    };
    let last = (g.qhead + IN_CAP - 1) % IN_CAP;
    if g.qhead != g.qtail && g.q[last].pid == pid && g.q[last].ev.kind == KIND_MOVE {
        g.q[last].ev.x = lx;
        g.q[last].ev.y = ly;
        return;
    }
    g.push_ev(
        pid,
        InputEv {
            kind: KIND_MOVE,
            x: lx,
            y: ly,
            key: 0,
        },
    );
}

pub fn push_client_key(id: u32, key: u8) {
    let mut g = WIN.lock();
    let Some(pid) = g.slots.iter().flatten().find(|s| s.id == id).map(|s| s.pid) else {
        return;
    };
    g.push_ev(
        pid,
        InputEv {
            kind: KIND_KEY,
            x: 0,
            y: 0,
            key: u32::from(key),
        },
    );
}

pub fn set_frame_pos(id: u32, ox: u32, oy: u32) {
    let mut g = WIN.lock();
    if let Some(surf) = g.slots.iter_mut().flatten().find(|s| s.id == id) {
        surf.ox = ox;
        surf.oy = oy;
    }
}

pub fn drop_id(id: u32) {
    let mut g = WIN.lock();
    for slot in g.slots.iter_mut() {
        if slot.as_ref().is_some_and(|s| s.id == id) {
            *slot = None;
        }
    }
    log_close(id);
}

pub fn drop_pid(pid: u32) {
    let mut ids = [0u32; MAX_SURFACES];
    let mut n = 0usize;
    let mut g = WIN.lock();
    for slot in g.slots.iter_mut() {
        if slot.as_ref().is_some_and(|s| s.pid == pid) {
            let id = slot.as_ref().unwrap().id;
            ids[n] = id;
            n += 1;
            *slot = None;
        }
    }
    g.retain_q(|q| q.pid != pid);
    drop(g);
    for i in 0..n {
        crate::comp::on_client_close(ids[i]);
    }
}

pub fn snapshot_clients() -> Vec<ClientSnap> {
    let g = WIN.lock();
    let mut out = Vec::new();
    for s in g.slots.iter().flatten() {
        let mut pixels = Vec::new();
        if pixels.try_reserve(s.buf.len()).is_err() {
            continue;
        }
        pixels.extend_from_slice(&s.buf);
        out.push(ClientSnap {
            id: s.id,
            ox: s.ox,
            oy: s.oy,
            w: s.w,
            h: s.h,
            pixels,
        });
    }
    out
}

pub fn drain_dirty() -> Vec<DirtyFrame> {
    let mut g = WIN.lock();
    let mut out = Vec::new();
    for surf in g.slots.iter_mut().flatten() {
        let Some((x, y, w, h)) = surf.dirty.take() else {
            continue;
        };
        let mut pixels = Vec::new();
        if pixels
            .try_reserve((w as usize).saturating_mul(h as usize))
            .is_err()
        {
            continue;
        }
        for row in 0..h {
            let src = ((y + row) * surf.w + x) as usize;
            pixels.extend_from_slice(&surf.buf[src..src + w as usize]);
        }
        out.push(DirtyFrame {
            sx: surf.ox.saturating_add(x),
            sy: surf.oy.saturating_add(DECO_H).saturating_add(y),
            w,
            h,
            pixels,
        });
    }
    out
}

impl Win {
    fn push_ev(&mut self, pid: u32, ev: InputEv) {
        let next = (self.qhead + 1) % IN_CAP;
        if next == self.qtail {
            return;
        }
        self.q[self.qhead] = Queued { pid, ev };
        self.qhead = next;
    }

    fn take_for_pid(&mut self, pid: u32, out: &mut [u8], nfit: usize) -> usize {
        let mut keep = [Queued {
            pid: 0,
            ev: InputEv {
                kind: 0,
                x: 0,
                y: 0,
                key: 0,
            },
        }; IN_CAP];
        let mut kn = 0usize;
        let mut copied = 0usize;
        let mut t = self.qtail;
        let h = self.qhead;
        while t != h {
            let q = self.q[t];
            t = (t + 1) % IN_CAP;
            if q.pid == pid && copied < nfit {
                let o = copied * EV_SIZE;
                ev_bytes(&q.ev, &mut out[o..o + EV_SIZE]);
                copied += 1;
            } else {
                keep[kn] = q;
                kn += 1;
            }
        }
        self.q[..kn].copy_from_slice(&keep[..kn]);
        self.qtail = 0;
        self.qhead = kn;
        copied * EV_SIZE
    }

    fn retain_q(&mut self, pred: impl Fn(&Queued) -> bool) {
        let mut keep = [Queued {
            pid: 0,
            ev: InputEv {
                kind: 0,
                x: 0,
                y: 0,
                key: 0,
            },
        }; IN_CAP];
        let mut kn = 0usize;
        let mut t = self.qtail;
        let h = self.qhead;
        while t != h {
            let q = self.q[t];
            t = (t + 1) % IN_CAP;
            if pred(&q) {
                keep[kn] = q;
                kn += 1;
            }
        }
        self.q[..kn].copy_from_slice(&keep[..kn]);
        self.qtail = 0;
        self.qhead = kn;
    }
}

fn ev_bytes(ev: &InputEv, dst: &mut [u8]) {
    dst[0..4].copy_from_slice(&ev.kind.to_le_bytes());
    dst[4..8].copy_from_slice(&ev.x.to_le_bytes());
    dst[8..12].copy_from_slice(&ev.y.to_le_bytes());
    dst[12..16].copy_from_slice(&ev.key.to_le_bytes());
}

fn copy_pixels(user_ptr: u64, dst: &mut [u32]) -> Result<(), ()> {
    let nbytes = dst.len() * 4;
    let bytes = unsafe { core::slice::from_raw_parts_mut(dst.as_mut_ptr() as *mut u8, nbytes) };
    fd::copy_from_user(user_ptr, nbytes, bytes)
}

fn clip(sw: u32, sh: u32, x: u32, y: u32, w: u32, h: u32) -> Option<(u32, u32, u32, u32)> {
    if x >= sw || y >= sh {
        return None;
    }
    let w = w.min(sw - x);
    let h = h.min(sh - y);
    if w == 0 || h == 0 {
        return None;
    }
    Some((x, y, w, h))
}

fn place(w: u32, h: u32) -> (u32, u32) {
    let Some(fb) = fbterm::info() else {
        return (ORIGIN_X, ORIGIN_Y);
    };
    let work = panel::work_h(fb.h);
    let mut ox = ORIGIN_X;
    let mut oy = ORIGIN_Y;
    let fh = DECO_H.saturating_add(h);
    if ox.saturating_add(w).saturating_add(SHADOW_PX) > fb.w {
        ox = fb.w.saturating_sub(w.saturating_add(SHADOW_PX));
    }
    if oy.saturating_add(fh).saturating_add(SHADOW_PX) > work {
        oy = work.saturating_sub(fh.saturating_add(SHADOW_PX));
    }
    (ox, oy)
}

fn log_create(id: u32) {
    serial::write_str("win: create id=");
    serial::write_dec_u32(id);
    serial::write_str("\n");
}

fn log_close(id: u32) {
    serial::write_str("win: close id=");
    serial::write_dec_u32(id);
    serial::write_str("\n");
}
