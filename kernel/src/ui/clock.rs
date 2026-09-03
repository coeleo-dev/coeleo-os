//! Monotonic uptime from the LAPIC 100 Hz tick.
//!
//! With the compositor, the mm:ss lives on the Plasma strut. CSI overlay
//! remains only when there is no compositor (non-GUI framebuffer).

use core::fmt::Write;
use core::sync::atomic::{AtomicU64, Ordering};

use crate::fbterm;

pub const HZ: u64 = 100;

static TICKS: AtomicU64 = AtomicU64::new(0);
static LAST_PAINTED_SEC: AtomicU64 = AtomicU64::new(u64::MAX);

struct SliceWriter<'a> {
    buf: &'a mut [u8],
    pos: usize,
}

impl Write for SliceWriter<'_> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        let rest = self.buf.get_mut(self.pos..).ok_or(core::fmt::Error)?;
        let n = s.len().min(rest.len());
        rest[..n].copy_from_slice(&s.as_bytes()[..n]);
        self.pos += n;
        Ok(())
    }
}

pub fn tick() {
    TICKS.fetch_add(1, Ordering::Relaxed);
}

pub fn ticks() -> u64 {
    TICKS.load(Ordering::Relaxed)
}

pub fn seconds() -> u64 {
    ticks() / HZ
}

pub fn millis() -> u64 {
    ticks().saturating_mul(1000 / HZ)
}

pub fn format_mmss(buf: &mut [u8; 16]) -> &str {
    let total = seconds();
    let secs = total % 60;
    let mins = total / 60;
    let n = {
        let mut w = SliceWriter { buf, pos: 0 };
        let _ = write!(w, "{mins:02}:{secs:02}");
        w.pos
    };
    core::str::from_utf8(&buf[..n]).unwrap_or("??:??")
}

pub fn format_panel(buf: &mut [u8; 16]) -> &str {
    if crate::rtc::format_hhmm(buf).is_some() {
        core::str::from_utf8(&buf[..5]).unwrap_or("??:??")
    } else {
        format_mmss(buf)
    }
}

pub fn paint_if_second_elapsed() {
    let sec = seconds();
    if LAST_PAINTED_SEC.load(Ordering::Relaxed) == sec {
        return;
    }
    LAST_PAINTED_SEC.store(sec, Ordering::Relaxed);

    if crate::comp::repaint_strut() {
        return;
    }

    let Some((cols, _)) = fbterm::dimensions() else {
        return;
    };
    let mut time = [0u8; 16];
    let text = format_mmss(&mut time);
    if cols < text.len() {
        return;
    }
    let col = cols - text.len() + 1;
    let mut seq = [0u8; 64];
    let n = {
        let mut w = SliceWriter {
            buf: &mut seq,
            pos: 0,
        };
        let _ = write!(w, "\x1b[s\x1b[1;{col}H\x1b[1;96m{text}\x1b[0m\x1b[u");
        w.pos
    };
    if let Ok(s) = core::str::from_utf8(&seq[..n]) {
        fbterm::write(s);
        fbterm::flush();
    }
}
