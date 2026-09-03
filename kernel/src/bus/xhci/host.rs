//! xHCI MMIO, rings, events, and host start. Internals stay in this module.

use core::sync::atomic::{Ordering, compiler_fence};

use virtio_drivers::transport::pci::bus::{BarInfo, Command, PciRoot};
use x86_64::{PhysAddr, VirtAddr};

use crate::clock;
use crate::pci::CamCf8;
use crate::{pmm, vmm};

pub(super) const CLASS_USB: u8 = 0x0C;
pub(super) const SUBCLASS_USB: u8 = 0x03;
pub(super) const PROG_XHCI: u8 = 0x30;

pub(super) const USBCMD: u32 = 0x00;
pub(super) const USBSTS: u32 = 0x04;
pub(super) const CRCR: u32 = 0x18;
pub(super) const DCBAAP: u32 = 0x30;
pub(super) const CONFIG: u32 = 0x38;
pub(super) const PORTSC0: u32 = 0x400;

pub(super) const CMD_RS: u32 = 1 << 0;
pub(super) const CMD_HCRST: u32 = 1 << 1;
pub(super) const STS_HCH: u32 = 1 << 0;
pub(super) const STS_CNR: u32 = 1 << 11;

pub(super) const PORT_CCS: u32 = 1 << 0;
pub(super) const PORT_PED: u32 = 1 << 1;
pub(super) const PORT_PR: u32 = 1 << 4;
pub(super) const PORT_PP: u32 = 1 << 9;
pub(super) const PORT_CSC: u32 = 1 << 17;
pub(super) const PORT_PRC: u32 = 1 << 21;
pub(super) const PORT_WPR: u32 = 1 << 31;
pub(super) const PORT_RW1C: u32 = 0x00FE_0000;

const IMAN: u32 = 0x20;
const ERSTSZ: u32 = 0x28;
const ERSTBA: u32 = 0x30;
const ERDP: u32 = 0x38;

pub(super) const TRB_NORMAL: u32 = 1;
pub(super) const TRB_SETUP: u32 = 2;
pub(super) const TRB_DATA: u32 = 3;
pub(super) const TRB_STATUS: u32 = 4;
pub(super) const TRB_LINK: u32 = 6;
pub(super) const TRB_ENABLE_SLOT: u32 = 9;
pub(super) const TRB_DISABLE_SLOT: u32 = 10;
pub(super) const TRB_ADDRESS: u32 = 11;
pub(super) const TRB_CONFIG_EP: u32 = 12;
pub(super) const TRB_EVAL: u32 = 13;
pub(super) const TRB_TRANSFER: u32 = 32;
pub(super) const TRB_CMD_COMP: u32 = 33;
pub(super) const TRB_PORT_STAT: u32 = 34;

pub(super) const CYCLE: u32 = 1;
pub(super) const IOC: u32 = 1 << 5;
pub(super) const IDT: u32 = 1 << 6;
pub(super) const CH: u32 = 1 << 4;
pub(super) const TC: u32 = 1 << 1;
pub(super) const TRT_IN: u32 = 3 << 16;
pub(super) const TRT_OUT: u32 = 2 << 16;
pub(super) const DIR_IN: u32 = 1 << 16;

pub(super) const CC_SUCCESS: u8 = 1;
pub(super) const CC_SHORT: u8 = 13;
pub(super) const CC_STALL: u8 = 6;

pub(super) const SPIN: u32 = 50_000_000;
pub(super) const PORT_SPIN: u32 = 8_000_000;
pub(super) const TIMEOUT_TICKS: u64 = 500;
pub(super) const RING_TRBS: usize = 256;
pub(super) const TRB_BYTES: usize = 16;
pub(super) const MAX_SLOTS: u8 = 8;
pub(super) const MAX_HC: usize = 2;
pub(super) const XFER_Q: usize = 16;

const HCC_CSZ: u32 = 1 << 2;
const HCC_PPC: u32 = 1 << 3;

pub(super) struct Ring {
    pub virt: VirtAddr,
    pub phys: PhysAddr,
    pub i: usize,
    pub pcs: u32,
}

pub(super) struct IntEp {
    pub dci: u8,
    pub ring: Ring,
    pub buf: VirtAddr,
    pub buf_phys: PhysAddr,
    pub maxpkt: u16,
    pub armed: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Unknown,
    Hub,
    Hid,
    Msc,
}

pub(super) struct Slot {
    pub kind: Kind,
    pub speed: u8,
    pub root_port: u8,
    pub route: u32,
    pub parent: u8,
    pub parent_port: u8,
    pub out_ctx: VirtAddr,
    pub out_ctx_phys: PhysAddr,
    pub ep0: Ring,
    pub bulk_out: Option<Ring>,
    pub bulk_in: Option<Ring>,
    pub dci_out: u8,
    pub dci_in: u8,
    pub int_eps: [Option<IntEp>; 2],
}

pub(super) struct Host {
    pub cap: VirtAddr,
    pub op: u32,
    pub rt: u32,
    pub db: u32,
    pub ctx_sz: usize,
    pub max_ports: u8,
    pub max_slots: u8,
    pub ppc: bool,
    pub cmd: Ring,
    pub evt: Ring,
    pub evt_ccs: u32,
    pub dcbaa: VirtAddr,
    pub in_ctx: VirtAddr,
    pub in_ctx_phys: PhysAddr,
    pub bounce: VirtAddr,
    pub bounce_phys: PhysAddr,
    pub cmd_ev: Option<Evt>,
    pub xfer: [Option<Evt>; XFER_Q],
    pub slots: [Option<Slot>; 9],
    pub tried: u32,
}

#[derive(Clone, Copy)]
pub(super) struct Evt {
    pub ty: u32,
    pub code: u8,
    pub slot: u8,
    pub dci: u8,
    pub remain: u32,
}

pub(super) struct Page {
    pub v: VirtAddr,
    pub p: PhysAddr,
}

pub(super) fn probe_all() -> [Option<Host>; MAX_HC] {
    let mut out: [Option<Host>; MAX_HC] = [None, None];
    let mut n = 0usize;
    let mut root = PciRoot::new(CamCf8);
    for bus in 0u8..=255 {
        for (df, info) in root.enumerate_bus(bus) {
            if info.class != CLASS_USB
                || info.subclass != SUBCLASS_USB
                || info.prog_if != PROG_XHCI
            {
                continue;
            }
            if let Some(hc) = start_pci(&mut root, df) {
                out[n] = Some(hc);
                n += 1;
                if n == MAX_HC {
                    return out;
                }
            }
        }
    }
    out
}

fn start_pci(
    root: &mut PciRoot<CamCf8>,
    df: virtio_drivers::transport::pci::bus::DeviceFunction,
) -> Option<Host> {
    let (_, mut cmd) = root.get_status_command(df);
    cmd.insert(Command::MEMORY_SPACE | Command::BUS_MASTER | Command::INTERRUPT_DISABLE);
    root.set_command(df, cmd);
    let (addr, size) = bar0(root, df)?;
    if addr == 0 || size == 0 {
        return None;
    }
    let cap = vmm::map_mmio_range(PhysAddr::new(addr), size as usize);
    start(cap)
}

fn bar0(
    root: &mut PciRoot<CamCf8>,
    df: virtio_drivers::transport::pci::bus::DeviceFunction,
) -> Option<(u64, u64)> {
    if let Ok(Some(BarInfo::Memory { address, size, .. })) = root.bar_info(df, 0) {
        if address != 0 && size != 0 {
            return Some((address, size));
        }
    }
    let bars = root.bars(df).ok()?;
    for bar in bars.iter().flatten() {
        if let Some((address, size)) = bar.memory_address_size() {
            if address != 0 && size != 0 {
                return Some((address, size));
            }
        }
    }
    None
}

fn start(cap: VirtAddr) -> Option<Host> {
    let caplength = read8(cap, 0);
    let hcs1 = read32(cap, 0x04);
    let hcs2 = read32(cap, 0x08);
    let hcc = read32(cap, 0x10);
    let dboff = read32(cap, 0x14);
    let rtsoff = read32(cap, 0x18);
    let op = u32::from(caplength);
    let max_slots = (hcs1 & 0xFF).min(u32::from(MAX_SLOTS)) as u8;
    if max_slots == 0 {
        return None;
    }
    let max_ports = ((hcs1 >> 24) & 0xFF) as u8;
    let ctx_sz = if hcc & HCC_CSZ != 0 { 64 } else { 32 };
    let ppc = hcc & HCC_PPC != 0;
    let scratch = scratchpad_count(hcs2);

    bios_handoff(cap, hcc);
    halt(cap, op)?;
    write32(cap, op + USBCMD, CMD_HCRST);
    wait_until(|| read32(cap, op + USBCMD) & CMD_HCRST == 0)?;
    wait_until(|| read32(cap, op + USBSTS) & STS_CNR == 0)?;

    let dcbaa_page = page()?;
    let in_page = page()?;
    let cmd_page = page()?;
    let evt_page = page()?;
    let erst_page = page()?;
    let bounce_page = page()?;

    if scratch > 0 {
        let array = page()?;
        for i in 0..scratch {
            let buf = page()?;
            unsafe {
                array
                    .v
                    .as_mut_ptr::<u8>()
                    .add(i * 8)
                    .cast::<u64>()
                    .write_volatile(buf.p.as_u64());
            }
        }
        unsafe {
            dcbaa_page
                .v
                .as_mut_ptr::<u64>()
                .write_volatile(array.p.as_u64());
        }
    }

    let cmd = ring_init(cmd_page.v, cmd_page.p);
    let evt = ring_init_evt(evt_page.v, evt_page.p);

    unsafe {
        erst_page
            .v
            .as_mut_ptr::<u64>()
            .write_volatile(evt_page.p.as_u64());
        erst_page
            .v
            .as_mut_ptr::<u8>()
            .add(8)
            .cast::<u32>()
            .write_volatile(RING_TRBS as u32);
    }

    write32(cap, op + CONFIG, u32::from(max_slots));
    write64(cap, op + DCBAAP, dcbaa_page.p.as_u64());
    write64(cap, op + CRCR, cmd_page.p.as_u64() | 1);
    write32(cap, rtsoff + ERSTSZ, 1);
    write64(cap, rtsoff + ERSTBA, erst_page.p.as_u64());
    write64(cap, rtsoff + ERDP, evt_page.p.as_u64());
    write32(cap, rtsoff + IMAN, 0);

    write32(cap, op + USBCMD, CMD_RS);
    wait_until(|| read32(cap, op + USBSTS) & STS_HCH == 0)?;

    let hc = Host {
        cap,
        op,
        rt: rtsoff,
        db: dboff,
        ctx_sz,
        max_ports,
        max_slots,
        ppc,
        cmd,
        evt,
        evt_ccs: 1,
        dcbaa: dcbaa_page.v,
        in_ctx: in_page.v,
        in_ctx_phys: in_page.p,
        bounce: bounce_page.v,
        bounce_phys: bounce_page.p,
        cmd_ev: None,
        xfer: [None; XFER_Q],
        slots: [None, None, None, None, None, None, None, None, None],
        tried: 0,
    };
    if hc.ppc {
        for p in 1..=hc.max_ports {
            let sc = read32(hc.cap, hc.op + PORTSC0 + u32::from(p - 1) * 0x10);
            if sc == 0xFFFF_FFFF {
                continue;
            }
            write32(
                hc.cap,
                hc.op + PORTSC0 + u32::from(p - 1) * 0x10,
                (sc & !PORT_RW1C) | PORT_PP,
            );
        }
    }
    spin_a_bit();
    Some(hc)
}

pub(super) fn pump(hc: &mut Host) {
    loop {
        let Some(ev) = pop_evt(hc) else {
            break;
        };
        match ev.ty {
            TRB_CMD_COMP => hc.cmd_ev = Some(ev),
            TRB_TRANSFER => stash_xfer(hc, ev),
            _ => {}
        }
    }
}

fn stash_xfer(hc: &mut Host, ev: Evt) {
    for slot in hc.xfer.iter_mut() {
        if slot
            .as_ref()
            .is_some_and(|x| x.slot == ev.slot && x.dci == ev.dci)
        {
            *slot = Some(ev);
            return;
        }
    }
    for slot in hc.xfer.iter_mut() {
        if slot.is_none() {
            *slot = Some(ev);
            return;
        }
    }
}

pub(super) fn take_xfer(hc: &mut Host, slot: u8, dci: u8) -> Option<Evt> {
    for e in hc.xfer.iter_mut() {
        if e.as_ref().is_some_and(|x| x.slot == slot && x.dci == dci) {
            return e.take();
        }
    }
    None
}

pub(super) fn wait_cmd(hc: &mut Host) -> Option<Evt> {
    for _ in 0..SPIN {
        pump(hc);
        if let Some(ev) = hc.cmd_ev.take() {
            return Some(ev);
        }
        core::hint::spin_loop();
    }
    None
}

pub(super) fn wait_xfer(hc: &mut Host, slot: u8, dci: u8) -> Option<Evt> {
    for _ in 0..SPIN {
        pump(hc);
        if let Some(ev) = take_xfer(hc, slot, dci) {
            return Some(ev);
        }
        core::hint::spin_loop();
    }
    None
}

pub(super) fn drain(hc: &mut Host) {
    pump(hc);
}

fn pop_evt(hc: &mut Host) -> Option<Evt> {
    let i = hc.evt.i;
    let dw2 = read_trb(hc.evt.virt, i, 2);
    let dw3 = read_trb(hc.evt.virt, i, 3);
    if dw3 & 1 != hc.evt_ccs {
        return None;
    }
    let ty = (dw3 >> 10) & 0x3F;
    let code = (dw2 >> 24) as u8;
    let slot = (dw3 >> 24) as u8;
    let dci = ((dw3 >> 16) & 0x1F) as u8;
    let remain = dw2 & 0x00FF_FFFF;
    hc.evt.i += 1;
    if hc.evt.i == RING_TRBS {
        hc.evt.i = 0;
        hc.evt_ccs ^= 1;
    }
    let last = hc.evt.phys.as_u64() + (i * TRB_BYTES) as u64;
    write64(hc.cap, hc.rt + ERDP, last | (1 << 3));
    Some(Evt {
        ty,
        code,
        slot,
        dci,
        remain,
    })
}

pub(super) fn enqueue_cmd(hc: &mut Host, p: u64, st: u32, extra: u32, mut ctrl: u32) {
    ctrl = (ctrl & !CYCLE) | hc.cmd.pcs;
    let _ = extra;
    put_trb(hc.cmd.virt, hc.cmd.i, p, st, ctrl);
    bump(&mut hc.cmd);
}

pub(super) fn enqueue_ep(ring: &mut Ring, p: u64, len: u32, mut ctrl: u32) {
    ctrl = (ctrl & !CYCLE) | ring.pcs;
    put_trb(ring.virt, ring.i, p, len, ctrl);
    bump(ring);
}

fn bump(ring: &mut Ring) {
    ring.i += 1;
    if ring.i + 1 >= RING_TRBS {
        let mut link = trb(ring.pcs, TRB_LINK, TC);
        link |= ring.pcs;
        put_trb(ring.virt, RING_TRBS - 1, ring.phys.as_u64(), 0, link);
        ring.i = 0;
        ring.pcs ^= 1;
    }
}

pub(super) fn ring_init(v: VirtAddr, p: PhysAddr) -> Ring {
    zero(v, pmm::FRAME_SIZE as usize);
    put_trb(v, RING_TRBS - 1, p.as_u64(), 0, trb(0, TRB_LINK, TC));
    Ring {
        virt: v,
        phys: p,
        i: 0,
        pcs: 1,
    }
}

fn ring_init_evt(v: VirtAddr, p: PhysAddr) -> Ring {
    zero(v, pmm::FRAME_SIZE as usize);
    Ring {
        virt: v,
        phys: p,
        i: 0,
        pcs: 1,
    }
}

fn bios_handoff(cap: VirtAddr, hcc: u32) {
    let xecp = ((hcc >> 16) & 0xFFFF) * 4;
    if xecp == 0 {
        return;
    }
    let mut off = xecp;
    for _ in 0..32 {
        let v = read32(cap, off);
        if v == 0xFFFF_FFFF {
            return;
        }
        if v & 0xFF == 1 {
            write8(cap, off + 3, read8(cap, off + 3) | 1);
            let _ = wait_until(|| read32(cap, off) & (1 << 16) == 0);
            let mut ctl = read32(cap, off + 4);
            ctl &= !((0x7 << 1) | (0xFF << 5) | (0x7 << 17));
            ctl |= 0x7 << 29;
            write32(cap, off + 4, ctl);
            return;
        }
        let next = (v >> 8) & 0xFF;
        if next == 0 {
            return;
        }
        off += next * 4;
    }
}

fn halt(cap: VirtAddr, op: u32) -> Option<()> {
    let mut cmd = read32(cap, op + USBCMD);
    cmd &= !CMD_RS;
    write32(cap, op + USBCMD, cmd);
    wait_until(|| read32(cap, op + USBSTS) & STS_HCH != 0)
}

pub(super) fn doorbell(hc: &Host, slot: u8, dci: u8) {
    compiler_fence(Ordering::SeqCst);
    write32(hc.cap, hc.db + u32::from(slot) * 4, u32::from(dci));
}

pub(super) fn dci(ep: u8) -> u8 {
    let n = ep & 0x0F;
    if n == 0 {
        return 1;
    }
    n * 2 + u8::from(ep & 0x80 != 0)
}

pub(super) fn max_packet0(speed: u8) -> u16 {
    match speed {
        1 | 2 => 8,
        3 => 64,
        _ => 512,
    }
}

fn scratchpad_count(hcs2: u32) -> usize {
    let lo = (hcs2 >> 27) & 0x1F;
    let hi = (hcs2 >> 21) & 0x1F;
    (lo | (hi << 5)) as usize
}

pub(super) fn trb(cycle: u32, ty: u32, extra: u32) -> u32 {
    (cycle & 1) | extra | (ty << 10)
}

pub(super) fn put_trb(base: VirtAddr, i: usize, param: u64, st: u32, ctrl: u32) {
    unsafe {
        let p = base.as_mut_ptr::<u8>().add(i * TRB_BYTES).cast::<u32>();
        p.write_volatile(param as u32);
        p.add(1).write_volatile((param >> 32) as u32);
        p.add(2).write_volatile(st);
        compiler_fence(Ordering::SeqCst);
        p.add(3).write_volatile(ctrl);
    }
}

fn read_trb(base: VirtAddr, i: usize, dw: usize) -> u32 {
    unsafe {
        base.as_ptr::<u8>()
            .add(i * TRB_BYTES + dw * 4)
            .cast::<u32>()
            .read_volatile()
    }
}

pub(super) fn write_ctx(base: VirtAddr, sz: usize, idx: u8, dw: usize, val: u32) {
    let off = idx as usize * sz + dw * 4;
    unsafe {
        base.as_mut_ptr::<u8>()
            .add(off)
            .cast::<u32>()
            .write_volatile(val);
    }
}

pub(super) fn read_ctx(base: VirtAddr, sz: usize, idx: u8, dw: usize) -> u32 {
    let off = idx as usize * sz + dw * 4;
    unsafe {
        base.as_ptr::<u8>()
            .add(off)
            .cast::<u32>()
            .read_volatile()
    }
}

pub(super) fn page() -> Option<Page> {
    let f = pmm::alloc()?;
    let p = f.start_address();
    let v = vmm::phys_to_virt(p);
    zero(v, pmm::FRAME_SIZE as usize);
    Some(Page { v, p })
}

pub(super) fn zero(v: VirtAddr, n: usize) {
    unsafe {
        core::ptr::write_bytes(v.as_mut_ptr::<u8>(), 0, n);
    }
}

pub(super) fn spin_a_bit() {
    for _ in 0..200_000 {
        core::hint::spin_loop();
    }
}

fn wait_until(mut pred: impl FnMut() -> bool) -> Option<()> {
    let start = clock::ticks();
    for _ in 0..SPIN {
        if pred() {
            return Some(());
        }
        if clock::ticks().saturating_sub(start) >= TIMEOUT_TICKS {
            return None;
        }
        core::hint::spin_loop();
    }
    None
}

fn read8(base: VirtAddr, off: u32) -> u8 {
    unsafe { base.as_ptr::<u8>().add(off as usize).read_volatile() }
}

fn write8(base: VirtAddr, off: u32, val: u8) {
    unsafe {
        base.as_mut_ptr::<u8>().add(off as usize).write_volatile(val);
    }
}

pub(super) fn read32(base: VirtAddr, off: u32) -> u32 {
    unsafe {
        base.as_ptr::<u8>()
            .add(off as usize)
            .cast::<u32>()
            .read_volatile()
    }
}

pub(super) fn write32(base: VirtAddr, off: u32, val: u32) {
    unsafe {
        base.as_mut_ptr::<u8>()
            .add(off as usize)
            .cast::<u32>()
            .write_volatile(val);
    }
}

fn write64(base: VirtAddr, off: u32, val: u64) {
    write32(base, off, val as u32);
    write32(base, off + 4, (val >> 32) as u32);
}
