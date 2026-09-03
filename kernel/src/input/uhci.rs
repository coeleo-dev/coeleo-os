//! UHCI host + HID boot-protocol mouse. Poll TDs; no USB IRQ, no keyboard.

use core::sync::atomic::{Ordering, compiler_fence};

use spin::Mutex;
use virtio_drivers::transport::pci::bus::{BarInfo, Command, ConfigurationAccess, PciRoot};
use x86_64::instructions::port::Port;
use x86_64::{PhysAddr, VirtAddr};

use crate::mouse::Event;
use crate::pci::CamCf8;
use crate::{pmm, vmm};

const CLASS_USB: u8 = 0x0C;
const SUBCLASS_USB: u8 = 0x03;
const PROG_UHCI: u8 = 0x00;

const USBCMD: u16 = 0x00;
const USBSTS: u16 = 0x02;
const USBINTR: u16 = 0x04;
const FRNUM: u16 = 0x06;
const FLBASEADD: u16 = 0x08;
const PORTSC0: u16 = 0x10;

const CMD_RS: u16 = 1 << 0;
const CMD_HCRESET: u16 = 1 << 1;
const CMD_GRESET: u16 = 1 << 2;
const CMD_CF: u16 = 1 << 6;
const CMD_MAXP: u16 = 1 << 7;

const STS_HCH: u16 = 1 << 5;

const PORT_CCS: u16 = 1 << 0;
const PORT_CSC: u16 = 1 << 1;
const PORT_PE: u16 = 1 << 2;
const PORT_PEDC: u16 = 1 << 3;
const PORT_LS: u16 = 1 << 8;
const PORT_RESET: u16 = 1 << 9;

const PTR_TERM: u32 = 1;
const PTR_QH: u32 = 2;
const PTR_DEPTH: u32 = 4;

const TD_SETUP: usize = 0x10;
const TD_DATA: usize = 0x20;
const TD_STAT: usize = 0x30;
const TD_INT: usize = 0x40;
const QH_OFF: usize = 0x00;
const SETUP_BUF: usize = 0x100;
const DATA_BUF: usize = 0x108;
const REPORT_BUF: usize = 0x208;

const PID_SETUP: u32 = 0x2D;
const PID_IN: u32 = 0x69;
const PID_OUT: u32 = 0xE1;

const TD_ACTIVE: u32 = 1 << 23;
const TD_LS: u32 = 1 << 26;
const TD_SPD: u32 = 1 << 29;
const TD_ERR_MASK: u32 = 0x7C_0000;

const REQ_GET_DESC: u8 = 6;
const REQ_SET_ADDR: u8 = 5;
const REQ_SET_CFG: u8 = 9;
const REQ_SET_IDLE: u8 = 0x0A;
const REQ_SET_PROTO: u8 = 0x0B;

const SPIN: u32 = 2_000_000;

struct Uhci {
    io: u16,
    addr: u8,
    ep: u8,
    toggle: u8,
    ls: bool,
    maxpkt: u16,
    pool: VirtAddr,
    pool_phys: PhysAddr,
    prev_left: bool,
    prev_right: bool,
}

static DEV: Mutex<Option<Uhci>> = Mutex::new(None);

pub fn init() {
    let Some((frame_phys, pool_phys)) = dma_pages() else {
        return;
    };
    let frame = vmm::phys_to_virt(frame_phys);
    let pool = vmm::phys_to_virt(pool_phys);
    unsafe {
        core::ptr::write_bytes(frame.as_mut_ptr::<u8>(), 0, pmm::FRAME_SIZE as usize);
        core::ptr::write_bytes(pool.as_mut_ptr::<u8>(), 0, pmm::FRAME_SIZE as usize);
    }

    let mut root = PciRoot::new(CamCf8);
    let mut found = [None; 8];
    let mut n = 0usize;
    for bus in 0u8..=255 {
        for (df, info) in root.enumerate_bus(bus) {
            if info.class == CLASS_USB && info.subclass == SUBCLASS_USB && info.prog_if == PROG_UHCI
            {
                if n < found.len() {
                    found[n] = Some(df);
                    n += 1;
                }
            }
        }
    }

    for slot in found.iter().flatten() {
        if let Some(dev) = try_controller(&mut root, *slot, frame, frame_phys, pool, pool_phys) {
            *DEV.lock() = Some(dev);
            return;
        }
    }
}

pub fn present() -> bool {
    DEV.lock().is_some()
}

pub fn idle() -> bool {
    let g = DEV.lock();
    let Some(dev) = g.as_ref() else {
        return true;
    };
    td_active(dev.pool, TD_INT)
}

pub fn poll() -> Option<Event> {
    let mut g = DEV.lock();
    let dev = g.as_mut()?;
    if td_active(dev.pool, TD_INT) {
        return None;
    }
    let cs = td_cs(dev.pool, TD_INT);
    if cs & TD_ERR_MASK != 0 {
        arm_int(dev);
        return None;
    }
    let mut report = [0u8; 4];
    unsafe {
        let src = (dev.pool.as_u64() as usize + REPORT_BUF) as *const u8;
        core::ptr::copy_nonoverlapping(src, report.as_mut_ptr(), 4);
    }
    out16(dev.io, USBSTS, 0x3F);
    arm_int(dev);

    let dx = report[1] as i8 as i16;
    let dy = report[2] as i8 as i16;
    let left = report[0] & 1 != 0;
    let right = report[0] & 2 != 0;
    let was = dev.prev_left;
    let was_r = dev.prev_right;
    dev.prev_left = left;
    dev.prev_right = right;
    if dx == 0 && dy == 0 && left == was && right == was_r {
        return None;
    }
    Some(Event {
        dx,
        dy,
        left_down: left && !was,
        left_up: !left && was,
        right_down: right && !was_r,
        right_up: !right && was_r,
    })
}

fn dma_pages() -> Option<(PhysAddr, PhysAddr)> {
    let f = pmm::alloc_contiguous(1)?;
    let p = pmm::alloc_contiguous(1)?;
    Some((f.start_address(), p.start_address()))
}

fn try_controller(
    root: &mut PciRoot<CamCf8>,
    df: virtio_drivers::transport::pci::bus::DeviceFunction,
    frame: VirtAddr,
    frame_phys: PhysAddr,
    pool: VirtAddr,
    pool_phys: PhysAddr,
) -> Option<Uhci> {
    let mut cam = CamCf8;
    cam.write_word(df, 0xC0, 0x8F00);

    let (_, mut cmd) = root.get_status_command(df);
    cmd.insert(Command::IO_SPACE | Command::BUS_MASTER | Command::INTERRUPT_DISABLE);
    root.set_command(df, cmd);

    let io = io_base(root, df)?;
    reset_hc(io)?;

    unsafe {
        core::ptr::write_bytes(pool.as_mut_ptr::<u8>(), 0, pmm::FRAME_SIZE as usize);
    }
    qh_write(pool, 0, PTR_TERM);
    qh_write(pool, 4, PTR_TERM);
    let qh_ptr = (pool_phys.as_u64() as u32) | PTR_QH;
    for i in 0..1024u32 {
        unsafe {
            core::ptr::write_volatile(
                (frame.as_u64() as usize as *mut u32).add(i as usize),
                qh_ptr,
            );
        }
    }

    out16(io, USBINTR, 0);
    out16(io, USBSTS, 0x3F);
    out16(io, FRNUM, 0);
    out32(io, FLBASEADD, frame_phys.as_u64() as u32);
    out16(io, USBCMD, CMD_RS | CMD_CF | CMD_MAXP);
    delay(4_000);
    if in16(io, USBSTS) & STS_HCH != 0 {
        out16(io, USBCMD, 0);
        return None;
    }

    for port in 0..2u16 {
        if let Some(dev) = try_port(io, port, pool, pool_phys) {
            return Some(dev);
        }
    }
    out16(io, USBCMD, 0);
    None
}

fn try_port(io: u16, port: u16, pool: VirtAddr, pool_phys: PhysAddr) -> Option<Uhci> {
    let off = PORTSC0 + port * 2;
    let sc = in16(io, off);
    if sc == 0xFFFF || sc & PORT_CCS == 0 {
        return None;
    }
    let ls = sc & PORT_LS != 0;
    out16(io, off, PORT_RESET);
    delay(50_000);
    out16(io, off, 0);
    delay(10_000);
    out16(io, off, PORT_PE | PORT_CSC | PORT_PEDC);
    delay(10_000);
    if in16(io, off) & PORT_PE == 0 {
        return None;
    }

    let mut setup = [0u8; 8];
    // Device descriptor, 8 bytes (EP0 MPS for low-speed).
    setup_pkt(&mut setup, 0x80, REQ_GET_DESC, 0x0100, 0, 8);
    let mut desc8 = [0u8; 8];
    let _ = control(io, pool, pool_phys, 0, ls, &setup, Some(&mut desc8), true);

    setup_pkt(&mut setup, 0x00, REQ_SET_ADDR, 1, 0, 0);
    control(io, pool, pool_phys, 0, ls, &setup, None, false)?;
    delay(2_000);
    let addr = 1u8;

    let mut cfg9 = [0u8; 9];
    setup_pkt(&mut setup, 0x80, REQ_GET_DESC, 0x0200, 0, 9);
    control(io, pool, pool_phys, addr, ls, &setup, Some(&mut cfg9), true)?;
    let total = u16::from_le_bytes([cfg9[2], cfg9[3]]).min(256);
    if total < 9 {
        return None;
    }
    let mut cfg = [0u8; 256];
    setup_pkt(&mut setup, 0x80, REQ_GET_DESC, 0x0200, 0, total);
    control(
        io,
        pool,
        pool_phys,
        addr,
        ls,
        &setup,
        Some(&mut cfg[..total as usize]),
        true,
    )?;

    let (iface, ep, maxpkt) = parse_mouse(&cfg[..total as usize])?;

    setup_pkt(
        &mut setup,
        0x00,
        REQ_SET_CFG,
        u16::from(cfg9[5].max(1)),
        0,
        0,
    );
    control(io, pool, pool_phys, addr, ls, &setup, None, false)?;

    setup_pkt(&mut setup, 0x21, REQ_SET_PROTO, 0, u16::from(iface), 0);
    let _ = control(io, pool, pool_phys, addr, ls, &setup, None, false);

    setup_pkt(&mut setup, 0x21, REQ_SET_IDLE, 0, u16::from(iface), 0);
    let _ = control(io, pool, pool_phys, addr, ls, &setup, None, false);

    let mut dev = Uhci {
        io,
        addr,
        ep,
        toggle: 0,
        ls,
        maxpkt,
        pool,
        pool_phys,
        prev_left: false,
        prev_right: false,
    };
    arm_int(&mut dev);
    Some(dev)
}

fn parse_mouse(cfg: &[u8]) -> Option<(u8, u8, u16)> {
    let mut i = 0usize;
    let mut iface = 0u8;
    let mut is_mouse = false;
    let mut ep = None;
    let mut maxpkt = 4u16;
    while i + 2 <= cfg.len() {
        let len = cfg[i] as usize;
        if len < 2 || i + len > cfg.len() {
            break;
        }
        match cfg[i + 1] {
            4 if len >= 9 => {
                iface = cfg[i + 2];
                is_mouse = cfg[i + 5] == 3 && cfg[i + 6] == 1 && cfg[i + 7] == 2;
            }
            5 if len >= 7 && is_mouse => {
                let addr = cfg[i + 2];
                let attr = cfg[i + 3];
                if addr & 0x80 != 0 && attr & 3 == 3 {
                    ep = Some(addr & 0x0F);
                    maxpkt = u16::from_le_bytes([cfg[i + 4], cfg[i + 5]]).max(3).min(8);
                }
            }
            _ => {}
        }
        i += len;
    }
    Some((iface, ep?, maxpkt))
}

fn arm_int(dev: &mut Uhci) {
    let td = TD_INT;
    let buf_phys = (dev.pool_phys.as_u64() as u32) + REPORT_BUF as u32;
    write_td(
        dev.pool,
        td,
        PTR_TERM,
        td_cs_bits(dev.ls),
        token(PID_IN, dev.addr, dev.ep, dev.toggle, dev.maxpkt),
        buf_phys,
    );
    compiler_fence(Ordering::SeqCst);
    qh_write(dev.pool, 4, (dev.pool_phys.as_u64() as u32) + TD_INT as u32);
    dev.toggle ^= 1;
}

fn control(
    io: u16,
    pool: VirtAddr,
    pool_phys: PhysAddr,
    addr: u8,
    ls: bool,
    setup: &[u8; 8],
    data: Option<&mut [u8]>,
    data_in: bool,
) -> Option<()> {
    unsafe {
        let dst = (pool.as_u64() as usize + SETUP_BUF) as *mut u8;
        core::ptr::copy_nonoverlapping(setup.as_ptr(), dst, 8);
        if let Some(buf) = data.as_ref() {
            if !data_in {
                core::ptr::copy_nonoverlapping(
                    buf.as_ptr(),
                    (pool.as_u64() as usize + DATA_BUF) as *mut u8,
                    buf.len(),
                );
            }
        }
    }

    let setup_phys = pool_phys.as_u64() as u32 + SETUP_BUF as u32;
    let data_phys = pool_phys.as_u64() as u32 + DATA_BUF as u32;
    let cs = td_cs_bits(ls);

    let data_len = data.as_ref().map(|d| d.len() as u16).unwrap_or(0);
    let has_data = data_len != 0;
    let status_pid = match (has_data, data_in) {
        (false, _) => PID_IN,
        (true, true) => PID_OUT,
        (true, false) => PID_IN,
    };

    let setup_next = if has_data {
        (pool_phys.as_u64() as u32 + TD_DATA as u32) | PTR_DEPTH
    } else {
        (pool_phys.as_u64() as u32 + TD_STAT as u32) | PTR_DEPTH
    };
    write_td(
        pool,
        TD_SETUP,
        setup_next,
        cs,
        token(PID_SETUP, addr, 0, 0, 8),
        setup_phys,
    );
    if has_data {
        let pid = if data_in { PID_IN } else { PID_OUT };
        write_td(
            pool,
            TD_DATA,
            (pool_phys.as_u64() as u32 + TD_STAT as u32) | PTR_DEPTH,
            cs,
            token(pid, addr, 0, 1, data_len),
            data_phys,
        );
    }
    write_td(
        pool,
        TD_STAT,
        PTR_TERM,
        cs,
        token(status_pid, addr, 0, 1, 0),
        0,
    );
    compiler_fence(Ordering::SeqCst);
    qh_write(pool, 4, pool_phys.as_u64() as u32 + TD_SETUP as u32);
    wait_td(pool, TD_STAT)?;
    out16(io, USBSTS, 0x3F);
    qh_write(pool, 4, PTR_TERM);
    if data_in {
        if let Some(buf) = data {
            unsafe {
                core::ptr::copy_nonoverlapping(
                    (pool.as_u64() as usize + DATA_BUF) as *const u8,
                    buf.as_mut_ptr(),
                    buf.len(),
                );
            }
        }
    }
    Some(())
}

fn wait_td(pool: VirtAddr, off: usize) -> Option<()> {
    for _ in 0..SPIN {
        let cs = td_cs(pool, off);
        if cs & TD_ACTIVE == 0 {
            if cs & TD_ERR_MASK != 0 {
                return None;
            }
            return Some(());
        }
    }
    None
}

fn td_cs_bits(ls: bool) -> u32 {
    let mut cs = TD_ACTIVE | (3 << 27) | TD_SPD;
    if ls {
        cs |= TD_LS;
    }
    cs
}

fn token(pid: u32, addr: u8, ep: u8, toggle: u8, len: u16) -> u32 {
    let max = if len == 0 { 0x7FF } else { u32::from(len - 1) };
    pid | (u32::from(addr) << 8) | (u32::from(ep) << 15) | (u32::from(toggle) << 19) | (max << 21)
}

fn write_td(pool: VirtAddr, off: usize, link: u32, cs: u32, token: u32, buf: u32) {
    let p = (pool.as_u64() as usize + off) as *mut u32;
    unsafe {
        core::ptr::write_volatile(p, link);
        core::ptr::write_volatile(p.add(1), cs);
        core::ptr::write_volatile(p.add(2), token);
        core::ptr::write_volatile(p.add(3), buf);
    }
}

fn td_cs(pool: VirtAddr, off: usize) -> u32 {
    let p = (pool.as_u64() as usize + off) as *const u32;
    unsafe { core::ptr::read_volatile(p.add(1)) }
}

fn td_active(pool: VirtAddr, off: usize) -> bool {
    td_cs(pool, off) & TD_ACTIVE != 0
}

fn qh_write(pool: VirtAddr, off: usize, val: u32) {
    let p = (pool.as_u64() as usize + QH_OFF + off) as *mut u32;
    unsafe {
        core::ptr::write_volatile(p, val);
    }
}

fn setup_pkt(buf: &mut [u8; 8], ty: u8, req: u8, value: u16, index: u16, len: u16) {
    buf[0] = ty;
    buf[1] = req;
    buf[2] = value as u8;
    buf[3] = (value >> 8) as u8;
    buf[4] = index as u8;
    buf[5] = (index >> 8) as u8;
    buf[6] = len as u8;
    buf[7] = (len >> 8) as u8;
}

fn io_base(
    root: &mut PciRoot<CamCf8>,
    df: virtio_drivers::transport::pci::bus::DeviceFunction,
) -> Option<u16> {
    for bar in [4u8, 0, 1, 2, 3, 5] {
        if let Ok(Some(BarInfo::IO { address, size })) = root.bar_info(df, bar) {
            if address != 0 && size >= 0x14 {
                return Some(address as u16);
            }
        }
    }
    None
}

fn reset_hc(io: u16) -> Option<()> {
    out16(io, USBCMD, CMD_GRESET);
    delay(20_000);
    out16(io, USBCMD, 0);
    delay(4_000);
    out16(io, USBCMD, CMD_HCRESET);
    for _ in 0..100_000 {
        if in16(io, USBCMD) & CMD_HCRESET == 0 {
            return Some(());
        }
        delay(1);
    }
    None
}

fn delay(n: u32) {
    for _ in 0..n {
        unsafe {
            Port::<u8>::new(0x80).write(0);
        }
    }
}

fn in16(io: u16, off: u16) -> u16 {
    unsafe { Port::<u16>::new(io + off).read() }
}

fn out16(io: u16, off: u16, val: u16) {
    unsafe {
        Port::<u16>::new(io + off).write(val);
    }
}

fn out32(io: u16, off: u16, val: u32) {
    unsafe {
        Port::<u32>::new(io + off).write(val);
    }
}
