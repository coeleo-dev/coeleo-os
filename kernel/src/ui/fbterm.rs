//! Flanterm on a heap canvas. The compositor blits it into a decorated window.
//!
//! `init` only records the Limine FB (no heap yet). `attach_vt` runs after
//! `heap::init` and binds Flanterm once (bump instance, no resize).

use alloc::vec::Vec;
use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::{AtomicPtr, Ordering};

use limine::framebuffer::Framebuffer;
use spin::Mutex;

#[derive(Clone, Copy)]
pub struct FbInfo {
    pub addr: usize,
    pub w: u32,
    pub h: u32,
    pub pitch: u32,
    pub bpp: u8,
    pub split: u32,
}

#[derive(Clone, Copy)]
struct Masks {
    rsz: u8,
    rsh: u8,
    gsz: u8,
    gsh: u8,
    bsz: u8,
    bsh: u8,
}

struct Vt {
    buf: Vec<u32>,
    w: u32,
    h: u32,
}

#[repr(C)]
struct FlantermContext {
    _opaque: [u8; 0],
}

unsafe extern "C" {
    fn flanterm_fb_init(
        malloc: Option<unsafe extern "C" fn(usize) -> *mut c_void>,
        free: Option<unsafe extern "C" fn(*mut c_void, usize)>,
        framebuffer: *mut u32,
        width: usize,
        height: usize,
        pitch: usize,
        red_mask_size: u8,
        red_mask_shift: u8,
        green_mask_size: u8,
        green_mask_shift: u8,
        blue_mask_size: u8,
        blue_mask_shift: u8,
        canvas: *mut u32,
        ansi_colours: *mut u32,
        ansi_bright_colours: *mut u32,
        default_bg: *mut u32,
        default_fg: *mut u32,
        default_bg_bright: *mut u32,
        default_fg_bright: *mut u32,
        font: *mut c_void,
        font_width: usize,
        font_height: usize,
        font_spacing: usize,
        font_scale_x: usize,
        font_scale_y: usize,
        margin: usize,
        rotation: i32,
    ) -> *mut FlantermContext;

    fn flanterm_write(ctx: *mut FlantermContext, buf: *const u8, count: usize);
    fn flanterm_flush(ctx: *mut FlantermContext);
    fn flanterm_get_dimensions(ctx: *mut FlantermContext, cols: *mut usize, rows: *mut usize);
    fn flanterm_get_cursor_pos(ctx: *mut FlantermContext, x: *mut usize, y: *mut usize);
}

static CTX: AtomicPtr<FlantermContext> = AtomicPtr::new(ptr::null_mut());
static FB: Mutex<Option<FbInfo>> = Mutex::new(None);
static MASKS: Mutex<Option<Masks>> = Mutex::new(None);
static VT: Mutex<Option<Vt>> = Mutex::new(None);

static ANSI: [u32; 8] = [
    0x001C_212B, // 0: Black (Surface)
    0x00F8_5149, // 1: Red (Danger)
    0x003F_B950, // 2: Green (Success)
    0x00D2_9922, // 3: Yellow (Warning)
    0x0038_8BFD, // 4: Blue (Accent)
    0x00BC_8CFF, // 5: Magenta
    0x0039_C5CF, // 6: Cyan
    0x00F0_F4F8, // 7: White (Text)
];
static ANSI_BRIGHT: [u32; 8] = [
    0x006E_7681, // 0: Bright Black (Muted)
    0x00FF_7B72, // 1: Bright Red
    0x0056_D364, // 2: Bright Green
    0x00E3_B341, // 3: Bright Yellow
    0x0058_A6FF, // 4: Bright Blue
    0x00D2_A8FF, // 5: Bright Magenta
    0x0056_D4DD, // 6: Bright Cyan
    0x00FF_FFFF, // 7: Bright White
];
static DEFAULT_BG: u32 = 0x001C_212B;
static DEFAULT_FG: u32 = 0x00F0_F4F8;

/// Record the Limine framebuffer. Does not init Flanterm (heap is not up yet).
pub fn init(fb: &Framebuffer<'_>) -> bool {
    let w = fb.width() as u32;
    let h = fb.height() as u32;
    let pitch = fb.pitch() as u32;
    let bpp = fb.bpp() as u8;
    *MASKS.lock() = Some(Masks {
        rsz: fb.red_mask_size(),
        rsh: fb.red_mask_shift(),
        gsz: fb.green_mask_size(),
        gsh: fb.green_mask_shift(),
        bsz: fb.blue_mask_size(),
        bsh: fb.blue_mask_shift(),
    });
    *FB.lock() = Some(FbInfo {
        addr: fb.addr() as usize,
        w,
        h,
        pitch,
        bpp,
        split: 0,
    });
    true
}

/// Allocate the VT canvas and bind Flanterm. Call once, after `heap::init`.
pub fn attach_vt_default() -> bool {
    let Some(fb) = info() else {
        return false;
    };
    let work = fb.h.saturating_sub(coeleo_theme::PANEL_H);
    let sh = coeleo_theme::SHADOW_PX;
    let deco = coeleo_theme::DECO_H;
    let w = fb.w.saturating_sub(32 + sh).min(1920).max(80);
    let h = work.saturating_sub(deco + 8 + sh).min(1080).max(64);
    attach_vt(w, h)
}

fn attach_vt(w: u32, mut h: u32) -> bool {
    if !CTX.load(Ordering::Relaxed).is_null() {
        return true;
    }
    let Some(masks) = *MASKS.lock() else {
        return false;
    };
    loop {
        let n = (w as usize).saturating_mul(h as usize);
        if n == 0 {
            return false;
        }
        let mut buf = Vec::new();
        if buf.try_reserve(n).is_ok() {
            buf.resize(n, DEFAULT_BG);
            let ptr = buf.as_mut_ptr();
            let ctx = unsafe {
                flanterm_fb_init(
                    None,
                    None,
                    ptr,
                    w as usize,
                    h as usize,
                    (w as usize).saturating_mul(4),
                    masks.rsz,
                    masks.rsh,
                    masks.gsz,
                    masks.gsh,
                    masks.bsz,
                    masks.bsh,
                    ptr::null_mut(),
                    ANSI.as_ptr().cast_mut(),
                    ANSI_BRIGHT.as_ptr().cast_mut(),
                    (&raw const DEFAULT_BG).cast_mut(),
                    (&raw const DEFAULT_FG).cast_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    ptr::null_mut(),
                    0,
                    0,
                    1,
                    0,
                    0,
                    2,
                    0,
                )
            };
            if ctx.is_null() {
                return false;
            }
            CTX.store(ctx, Ordering::Relaxed);
            *VT.lock() = Some(Vt { buf, w, h });
            return true;
        }
        if h <= 64 {
            return false;
        }
        h = h.saturating_sub(16);
    }
}

pub fn info() -> Option<FbInfo> {
    *FB.lock()
}

pub fn is_ready() -> bool {
    FB.lock().is_some()
}

pub fn vt_size() -> Option<(u32, u32)> {
    VT.lock().as_ref().map(|v| (v.w, v.h))
}

pub fn with_vt_pixels<R>(f: impl FnOnce(&[u32], u32, u32) -> R) -> Option<R> {
    let g = VT.lock();
    let vt = g.as_ref()?;
    Some(f(&vt.buf, vt.w, vt.h))
}

pub fn write(s: &str) {
    let ctx = CTX.load(Ordering::Relaxed);
    if ctx.is_null() {
        return;
    }
    let old = cursor_pos(ctx);
    unsafe {
        flanterm_write(ctx, s.as_ptr(), s.len());
    }
    let new = cursor_pos(ctx);
    crate::comp::damage_vt(dirty_canvas(s, old, new));
}

pub fn flush() {
    let ctx = CTX.load(Ordering::Relaxed);
    if ctx.is_null() {
        return;
    }
    unsafe {
        flanterm_flush(ctx);
    }
    crate::comp::damage_vt(None);
}

fn cursor_pos(ctx: *mut FlantermContext) -> Option<(usize, usize)> {
    if ctx.is_null() {
        return None;
    }
    let mut x = 0usize;
    let mut y = 0usize;
    unsafe {
        flanterm_get_cursor_pos(ctx, &mut x, &mut y);
    }
    Some((x, y))
}

/// Canvas dirty rect, or `None` for the whole client (scroll / CSI / unknown).
fn dirty_canvas(
    s: &str,
    old: Option<(usize, usize)>,
    new: Option<(usize, usize)>,
) -> Option<(u32, u32, u32, u32)> {
    if s.bytes().any(|b| b == b'\n' || b == 0x1b || b == 0x0c) {
        return None;
    }
    let (ocol, orow) = old?;
    let (ncol, nrow) = new?;
    let (cols, rows) = dimensions()?;
    let (vw, vh) = vt_size()?;
    if cols == 0 || rows == 0 {
        return None;
    }
    if orow + 1 >= rows && ocol + s.chars().count() >= cols {
        return None;
    }
    if nrow.abs_diff(orow) > 3 {
        return None;
    }
    let cw = (vw / cols as u32).max(1);
    let ch = (vh / rows as u32).max(1);
    let (x0, y0) = cell_padded(ocol.min(ncol), orow.min(nrow), cw, ch, vw, vh, true);
    let (x1, y1) = cell_padded(ocol.max(ncol), orow.max(nrow), cw, ch, vw, vh, false);
    if y1.saturating_sub(y0) > ch.saturating_mul(3) {
        return None;
    }
    Some((x0, y0, x1.saturating_sub(x0), y1.saturating_sub(y0)))
}

fn cell_padded(
    col: usize,
    row: usize,
    cw: u32,
    ch: u32,
    vw: u32,
    vh: u32,
    origin: bool,
) -> (u32, u32) {
    if origin {
        let c = col.saturating_sub(1) as u32;
        let r = row.saturating_sub(1) as u32;
        (c.saturating_mul(cw).min(vw), r.saturating_mul(ch).min(vh))
    } else {
        let c = col.saturating_add(2) as u32;
        let r = row.saturating_add(2) as u32;
        (c.saturating_mul(cw).min(vw), r.saturating_mul(ch).min(vh))
    }
}

pub fn dimensions() -> Option<(usize, usize)> {
    let ctx = CTX.load(Ordering::Relaxed);
    if ctx.is_null() {
        return None;
    }
    let mut cols = 0usize;
    let mut rows = 0usize;
    unsafe {
        flanterm_get_dimensions(ctx, &mut cols, &mut rows);
    }
    Some((cols, rows))
}
