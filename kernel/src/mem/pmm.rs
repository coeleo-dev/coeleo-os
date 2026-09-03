//! Physical frame bitmap. Tracks only Limine USABLE RAM up to 4 GiB.

use core::sync::atomic::{AtomicU64, Ordering};

use limine::memory_map::{Entry, EntryType};
use spin::Mutex;
use x86_64::PhysAddr;
use x86_64::structures::paging::{PhysFrame, Size4KiB};

pub const FRAME_SIZE: u64 = 4096;
const MAX_FRAMES: u64 = 1_048_576;
const BITMAP_BYTES: usize = (MAX_FRAMES / 8) as usize;

static BITMAP: Mutex<[u8; BITMAP_BYTES]> = Mutex::new([0xFF; BITMAP_BYTES]);
static USABLE_TOTAL: AtomicU64 = AtomicU64::new(0);
static FREE: AtomicU64 = AtomicU64::new(0);

pub struct Stats {
    pub free: u64,
    pub total: u64,
}

pub fn init(entries: &[&Entry]) {
    let mut bitmap = BITMAP.lock();
    *bitmap = [0xFF; BITMAP_BYTES];
    let mut usable = 0u64;

    for entry in entries {
        if entry.entry_type != EntryType::USABLE {
            continue;
        }
        let start = (entry.base + FRAME_SIZE - 1) / FRAME_SIZE;
        let end = (entry.base + entry.length) / FRAME_SIZE;
        for frame in start..end {
            if frame >= MAX_FRAMES {
                break;
            }
            if is_used(&bitmap, frame) {
                mark_free(&mut bitmap, frame);
                usable += 1;
            }
        }
    }

    USABLE_TOTAL.store(usable, Ordering::Relaxed);
    FREE.store(usable, Ordering::Relaxed);
}

pub fn alloc() -> Option<PhysFrame<Size4KiB>> {
    alloc_contiguous(1)
}

pub fn alloc_contiguous(n: u64) -> Option<PhysFrame<Size4KiB>> {
    if n == 0 {
        return None;
    }
    let mut bitmap = BITMAP.lock();
    let mut run = 0u64;
    let mut run_start = 0u64;
    for frame in 0..MAX_FRAMES {
        if is_used(&bitmap, frame) {
            run = 0;
            continue;
        }
        if run == 0 {
            run_start = frame;
        }
        run += 1;
        if run == n {
            for f in run_start..run_start + n {
                mark_used(&mut bitmap, f);
            }
            FREE.fetch_sub(n, Ordering::Relaxed);
            return PhysFrame::from_start_address(PhysAddr::new(run_start * FRAME_SIZE)).ok();
        }
    }
    None
}

pub fn free(frame: PhysFrame<Size4KiB>) {
    free_contiguous(frame, 1);
}

pub fn free_contiguous(start: PhysFrame<Size4KiB>, n: u64) {
    if n == 0 {
        return;
    }
    let start_n = start.start_address().as_u64() / FRAME_SIZE;
    let mut bitmap = BITMAP.lock();
    let mut freed = 0u64;
    for i in 0..n {
        let f = start_n + i;
        if f >= MAX_FRAMES {
            break;
        }
        if is_used(&bitmap, f) {
            mark_free(&mut bitmap, f);
            freed += 1;
        }
    }
    FREE.fetch_add(freed, Ordering::Relaxed);
}

pub fn stats() -> Stats {
    Stats {
        free: FREE.load(Ordering::Relaxed),
        total: USABLE_TOTAL.load(Ordering::Relaxed),
    }
}

fn is_used(bitmap: &[u8; BITMAP_BYTES], frame: u64) -> bool {
    let (byte, mask) = bit(frame);
    bitmap[byte] & mask != 0
}

fn mark_used(bitmap: &mut [u8; BITMAP_BYTES], frame: u64) {
    let (byte, mask) = bit(frame);
    bitmap[byte] |= mask;
}

fn mark_free(bitmap: &mut [u8; BITMAP_BYTES], frame: u64) {
    let (byte, mask) = bit(frame);
    bitmap[byte] &= !mask;
}

fn bit(frame: u64) -> (usize, u8) {
    let i = frame as usize;
    (i / 8, 1 << (i % 8))
}
