//! CMOS RTC (ports 0x70/0x71). Unsafe is only the port I/O.

use core::sync::atomic::{AtomicU8, AtomicU64, Ordering};

use x86_64::instructions::port::Port;

const IDX: u16 = 0x70;
const DATA: u16 = 0x71;
const NMI_OFF: u8 = 0x80;
const REG_SEC: u8 = 0x00;
const REG_MIN: u8 = 0x02;
const REG_HOUR: u8 = 0x04;
const REG_DAY: u8 = 0x07;
const REG_MONTH: u8 = 0x08;
const REG_YEAR: u8 = 0x09;
const REG_STAT_A: u8 = 0x0A;
const REG_STAT_B: u8 = 0x0B;
const REG_CENTURY_DEFAULT: u8 = 0x32;
const UIP: u8 = 1 << 7;
const STAT_B_24H: u8 = 1 << 1;
const STAT_B_BIN: u8 = 1 << 2;
const HOUR_PM: u8 = 1 << 7;
const UIP_SPINS: u32 = 100_000;
const SNAP_TRIES: u32 = 3;

static CENTURY_REG: AtomicU8 = AtomicU8::new(REG_CENTURY_DEFAULT);
static CACHE_SEC: AtomicU64 = AtomicU64::new(u64::MAX);
static PACKED: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Copy)]
struct Civil {
    year: u16,
    month: u8,
    day: u8,
    hour: u8,
    min: u8,
    sec: u8,
}

pub fn set_century_reg(reg: u8) {
    if reg != 0 {
        CENTURY_REG.store(reg, Ordering::Relaxed);
    }
}

pub fn format_hhmm(buf: &mut [u8; 16]) -> Option<&str> {
    let c = cached()?;
    if buf.len() < 5 {
        return None;
    }
    put2(buf, 0, c.hour);
    buf[2] = b':';
    put2(buf, 3, c.min);
    core::str::from_utf8(&buf[..5]).ok()
}

pub fn format_ymd(buf: &mut [u8; 16]) -> Option<&str> {
    let c = cached()?;
    if buf.len() < 10 {
        return None;
    }
    put4(buf, 0, c.year);
    buf[4] = b'-';
    put2(buf, 5, c.month);
    buf[7] = b'-';
    put2(buf, 8, c.day);
    core::str::from_utf8(&buf[..10]).ok()
}

pub fn format_iso(buf: &mut [u8]) -> Option<&str> {
    let c = read_fresh()?;
    if buf.len() < 20 {
        return None;
    }
    let n = write_iso_slice(buf, c);
    core::str::from_utf8(&buf[..n]).ok()
}

pub fn sys_date(buf: u64, len: u64) -> u64 {
    if len == 0 || !crate::vmm::user_slice_ok(buf, len) {
        return u64::MAX;
    }
    let Some(c) = read_fresh() else {
        return u64::MAX;
    };
    let mut tmp = [0u8; 20];
    let n = write_iso(&mut tmp, c);
    let n = n.min(len as usize);
    if crate::fd::copy_to_user(buf, &tmp[..n]).is_err() {
        return u64::MAX;
    }
    n as u64
}

fn cached() -> Option<Civil> {
    let sec = crate::clock::seconds();
    if CACHE_SEC.load(Ordering::Relaxed) != sec {
        let _ = refresh();
    }
    unpack(PACKED.load(Ordering::Relaxed))
}

fn read_fresh() -> Option<Civil> {
    refresh()
}

fn refresh() -> Option<Civil> {
    let c = read_cmos()?;
    PACKED.store(pack(c), Ordering::Relaxed);
    CACHE_SEC.store(crate::clock::seconds(), Ordering::Relaxed);
    Some(c)
}

fn pack(c: Civil) -> u64 {
    (1u64 << 63)
        | (u64::from(c.year) << 32)
        | (u64::from(c.month) << 24)
        | (u64::from(c.day) << 16)
        | (u64::from(c.hour) << 8)
        | u64::from(c.min)
        | (u64::from(c.sec) << 48)
}

fn unpack(v: u64) -> Option<Civil> {
    if v & (1u64 << 63) == 0 {
        return None;
    }
    Some(Civil {
        year: ((v >> 32) & 0xFFFF) as u16,
        month: ((v >> 24) & 0xFF) as u8,
        day: ((v >> 16) & 0xFF) as u8,
        hour: ((v >> 8) & 0xFF) as u8,
        min: (v & 0xFF) as u8,
        sec: ((v >> 48) & 0xFF) as u8,
    })
}

fn read_cmos() -> Option<Civil> {
    let mut last: Option<[u8; 8]> = None;
    for _ in 0..SNAP_TRIES {
        wait_uip()?;
        let snap = snapshot();
        restore_nmi();
        if last == Some(snap) {
            return decode(snap);
        }
        last = Some(snap);
    }
    last.and_then(decode)
}

fn wait_uip() -> Option<()> {
    for _ in 0..UIP_SPINS {
        if cmos_read(REG_STAT_A) & UIP == 0 {
            return Some(());
        }
        core::hint::spin_loop();
    }
    restore_nmi();
    None
}

fn snapshot() -> [u8; 8] {
    [
        cmos_read(REG_SEC),
        cmos_read(REG_MIN),
        cmos_read(REG_HOUR),
        cmos_read(REG_DAY),
        cmos_read(REG_MONTH),
        cmos_read(REG_YEAR),
        cmos_read(REG_STAT_B),
        cmos_read(CENTURY_REG.load(Ordering::Relaxed)),
    ]
}

fn decode(raw: [u8; 8]) -> Option<Civil> {
    let stat_b = raw[6];
    let bin = stat_b & STAT_B_BIN != 0;
    let sec = decode_u8(raw[0], bin)?;
    let min = decode_u8(raw[1], bin)?;
    let mut hour = raw[2];
    let day = decode_u8(raw[3], bin)?;
    let month = decode_u8(raw[4], bin)?;
    let year = decode_u8(raw[5], bin)?;
    if !bin {
        let pm = hour & HOUR_PM != 0;
        hour &= !HOUR_PM;
        hour = bcd(hour)?;
        if stat_b & STAT_B_24H == 0 {
            hour = hour12(hour, pm)?;
        }
    } else if stat_b & STAT_B_24H == 0 {
        let pm = hour & HOUR_PM != 0;
        hour = hour12(hour & !HOUR_PM, pm)?;
    }
    if sec > 59 || min > 59 || hour > 23 || day < 1 || day > 31 || month < 1 || month > 12 {
        return None;
    }
    let mut y = u16::from(year);
    let century = decode_u8(raw[7], bin).filter(|&c| (19..=21).contains(&c));
    if let Some(c) = century {
        y = u16::from(c) * 100 + y;
    } else if y < 100 {
        y += 2000;
    }
    if !(2000..=2099).contains(&y) {
        return None;
    }
    Some(Civil {
        year: y,
        month,
        day,
        hour,
        min,
        sec,
    })
}

fn hour12(h: u8, pm: bool) -> Option<u8> {
    if h < 1 || h > 12 {
        return None;
    }
    Some(match (h, pm) {
        (12, false) => 0,
        (12, true) => 12,
        (n, true) => n + 12,
        (n, false) => n,
    })
}

fn decode_u8(v: u8, bin: bool) -> Option<u8> {
    if bin {
        Some(v)
    } else {
        bcd(v)
    }
}

fn bcd(v: u8) -> Option<u8> {
    let hi = v >> 4;
    let lo = v & 0x0F;
    if hi > 9 || lo > 9 {
        return None;
    }
    Some(hi * 10 + lo)
}

fn cmos_read(reg: u8) -> u8 {
    unsafe {
        Port::<u8>::new(IDX).write(NMI_OFF | (reg & 0x7F));
        Port::<u8>::new(DATA).read()
    }
}

fn restore_nmi() {
    unsafe {
        Port::<u8>::new(IDX).write(0x0D);
    }
}

fn write_iso(buf: &mut [u8; 20], c: Civil) -> usize {
    write_iso_slice(buf, c)
}

fn write_iso_slice(buf: &mut [u8], c: Civil) -> usize {
    put4(buf, 0, c.year);
    buf[4] = b'-';
    put2(buf, 5, c.month);
    buf[7] = b'-';
    put2(buf, 8, c.day);
    buf[10] = b' ';
    put2(buf, 11, c.hour);
    buf[13] = b':';
    put2(buf, 14, c.min);
    buf[16] = b':';
    put2(buf, 17, c.sec);
    buf[19] = b'\n';
    20
}

fn put2(buf: &mut [u8], i: usize, n: u8) {
    buf[i] = b'0' + (n / 10);
    buf[i + 1] = b'0' + (n % 10);
}

fn put4(buf: &mut [u8], i: usize, n: u16) {
    buf[i] = b'0' + ((n / 1000) % 10) as u8;
    buf[i + 1] = b'0' + ((n / 100) % 10) as u8;
    buf[i + 2] = b'0' + ((n / 10) % 10) as u8;
    buf[i + 3] = b'0' + (n % 10) as u8;
}
