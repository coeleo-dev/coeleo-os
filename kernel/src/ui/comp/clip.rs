//! Shared clipboard buffer.

use alloc::vec::Vec;
use spin::Mutex;

const MAX_CLIP: usize = 4096;
static CLIPBOARD: Mutex<Vec<u8>> = Mutex::new(Vec::new());

pub fn sys_clipboard(op: u64, ptr: u64, len: u64) -> u64 {
    if op == 0 {
        let clip = CLIPBOARD.lock();
        if len < clip.len() as u64 {
            return clip.len() as u64;
        }
        if !clip.is_empty() {
            let buf = unsafe { core::slice::from_raw_parts_mut(ptr as *mut u8, clip.len()) };
            buf.copy_from_slice(&clip);
        }
        clip.len() as u64
    } else if op == 1 {
        let mut clip = CLIPBOARD.lock();
        clip.clear();
        let copy_len = len.min(MAX_CLIP as u64) as usize;
        if copy_len > 0 {
            let buf = unsafe { core::slice::from_raw_parts(ptr as *const u8, copy_len) };
            clip.extend_from_slice(buf);
        }
        0
    } else {
        u64::MAX
    }
}
