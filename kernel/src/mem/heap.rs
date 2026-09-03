//! Kernel heap: PMM frames claimed by Talc through the HHDM.
//!
//! 48 MiB: VT canvas and scene backbuffer (~8 MiB each at 1080p) plus a
//! framebuffer-sized wallpaper and a JPEG decode (stb + packed RGBA).
//! C malloc/realloc/free wrap the same heap for vendored stb_image.

use core::alloc::Layout;
use core::ffi::c_void;
use core::ptr;

use talc::{ErrOnOom, Talc, Talck};

use crate::{pmm, vmm};

/// Bytes before a C allocation: payload length, 16-byte aligned.
const C_HDR: usize = 16;

const HEAP_FRAMES: u64 = 12288;

#[global_allocator]
static ALLOCATOR: Talck<spin::Mutex<()>, ErrOnOom> = Talc::new(ErrOnOom).lock();

pub fn init() {
    let start = pmm::alloc_contiguous(HEAP_FRAMES).expect("heap frames");
    let virt = vmm::phys_to_virt(start.start_address());
    let ptr = virt.as_mut_ptr::<u8>();
    let len = (HEAP_FRAMES * pmm::FRAME_SIZE) as usize;
    // SAFETY: these frames were just allocated and are mapped in the HHDM.
    let span = unsafe { core::slice::from_raw_parts_mut(ptr, len) };
    unsafe {
        ALLOCATOR.lock().claim(span.into()).expect("talc claim");
    }
}

fn c_layout(total: usize) -> Option<Layout> {
    Layout::from_size_align(total, C_HDR).ok()
}

/// C heap for stb_image. Size lives in the header so `realloc` need not take it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn coeleo_malloc(size: usize) -> *mut c_void {
    if size == 0 {
        return ptr::null_mut();
    }
    let total = size.saturating_add(C_HDR);
    if total < size {
        return ptr::null_mut();
    }
    let Some(layout) = c_layout(total) else {
        return ptr::null_mut();
    };
    let raw = unsafe { alloc::alloc::alloc(layout) };
    if raw.is_null() {
        return ptr::null_mut();
    }
    unsafe {
        raw.cast::<usize>().write(size);
        raw.add(C_HDR).cast()
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn coeleo_free(p: *mut c_void) {
    if p.is_null() {
        return;
    }
    unsafe {
        let raw = (p as *mut u8).sub(C_HDR);
        let size = raw.cast::<usize>().read();
        let total = size.saturating_add(C_HDR);
        if let Some(layout) = c_layout(total) {
            alloc::alloc::dealloc(raw, layout);
        }
    }
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn coeleo_realloc(p: *mut c_void, newsz: usize) -> *mut c_void {
    unsafe {
        if p.is_null() {
            return coeleo_malloc(newsz);
        }
        if newsz == 0 {
            coeleo_free(p);
            return ptr::null_mut();
        }
        let q = coeleo_malloc(newsz);
        if q.is_null() {
            return ptr::null_mut();
        }
        let old = (p as *mut u8).sub(C_HDR).cast::<usize>().read();
        ptr::copy_nonoverlapping(p as *const u8, q as *mut u8, old.min(newsz));
        coeleo_free(p);
        q
    }
}
