//! Intel 82574L (e1000e): poll Ethernet, smoltcp `phy::Device`.
//!
//! IRQs stay masked (PCI INTERRUPT_DISABLE + IMC). `net::init` runs before
//! `interrupts::enable`, so reset/link waits are bounded spins, not ticks.

use core::sync::atomic::{Ordering, compiler_fence, fence};

use smoltcp::phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken};
use smoltcp::time::Instant;
use virtio_drivers::transport::pci::bus::{BarInfo, Command, PciRoot};
use x86_64::structures::paging::{PhysFrame, Size4KiB};
use x86_64::{PhysAddr, VirtAddr};

use crate::pci::CamCf8;
use crate::{pmm, vmm};

const VENDOR_INTEL: u16 = 0x8086;
const DEV_82574L: u16 = 0x10D3;

const RING: usize = 32;
const BUF_LEN: usize = 2048;
const MTU: usize = 1514;
const DESC_BYTES: u32 = (RING * 16) as u32;
const SPIN_MAX: u32 = 50_000_000;

const REG_CTRL: u32 = 0x00000;
const REG_STATUS: u32 = 0x00008;
const REG_ICR: u32 = 0x000C0;
const REG_IMS: u32 = 0x000D0;
const REG_IMC: u32 = 0x000D8;
const REG_RCTL: u32 = 0x00100;
const REG_TCTL: u32 = 0x00400;
const REG_TIPG: u32 = 0x00410;
const REG_RDBAL: u32 = 0x02800;
const REG_RDBAH: u32 = 0x02804;
const REG_RDLEN: u32 = 0x02808;
const REG_RDH: u32 = 0x02810;
const REG_RDT: u32 = 0x02818;
const REG_RXDCTL: u32 = 0x02828;
const REG_TDBAL: u32 = 0x03800;
const REG_TDBAH: u32 = 0x03804;
const REG_TDLEN: u32 = 0x03808;
const REG_TDH: u32 = 0x03810;
const REG_TDT: u32 = 0x03818;
const REG_TXDCTL: u32 = 0x03828;
const REG_RFCTL: u32 = 0x05008;
const REG_RAL0: u32 = 0x05400;
const REG_RAH0: u32 = 0x05404;

const CTRL_FD: u32 = 1 << 0;
const CTRL_ASDE: u32 = 1 << 5;
const CTRL_SLU: u32 = 1 << 6;
const CTRL_RST: u32 = 1 << 26;
const CTRL_RFCE: u32 = 1 << 27;
const CTRL_TFCE: u32 = 1 << 28;

const STATUS_LU: u32 = 1 << 1;
const RAH_AV: u32 = 1 << 31;
const RFCTL_EXSTEN: u32 = 1 << 15;

const RCTL_EN: u32 = 1 << 1;
const RCTL_BAM: u32 = 1 << 15;
const RCTL_SECRC: u32 = 1 << 26;

const TCTL_EN: u32 = 1 << 1;
const TCTL_PSP: u32 = 1 << 3;
const TCTL_CT: u32 = 0x10 << 4;
const TCTL_COLD: u32 = 0x40 << 12;
const TIPG_COPPER: u32 = 0x0060_2008;

const DCTL_WTHRESH: u32 = 1 << 16;
const DCTL_GRAN: u32 = 1 << 24;
const DCTL_ENABLE: u32 = 1 << 25;

const RX_DD: u8 = 1 << 0;
const RX_EOP: u8 = 1 << 1;
const TX_DD: u8 = 1 << 0;
const TX_CMD_EOP: u8 = 1 << 0;
const TX_CMD_IFCS: u8 = 1 << 1;
const TX_CMD_RS: u8 = 1 << 3;

#[repr(C)]
struct RxDesc {
    addr: u64,
    length: u16,
    _csum: u16,
    status: u8,
    errors: u8,
    _special: u16,
}

#[repr(C)]
struct TxDesc {
    addr: u64,
    length: u16,
    _cso: u8,
    cmd: u8,
    status: u8,
    _css: u8,
    _special: u16,
}

pub struct E1000e {
    mmio: VirtAddr,
    mac: [u8; 6],
    rx_ring: VirtAddr,
    tx_ring: VirtAddr,
    rx_ring_phys: PhysAddr,
    tx_ring_phys: PhysAddr,
    rx_virt: [VirtAddr; RING],
    tx_phys: [PhysAddr; RING],
    tx_virt: [VirtAddr; RING],
    rx_i: usize,
    tx_i: usize,
}

pub struct RxTok {
    frame: [u8; BUF_LEN],
    len: usize,
}

pub struct TxTok<'a> {
    nic: &'a mut E1000e,
}

impl E1000e {
    pub fn probe() -> Option<Self> {
        let (mmio, _) = map_bar0()?;
        mask_irq(mmio);
        reset(mmio)?;
        set_link(mmio);
        if !spin_until(|| (read32(mmio, REG_STATUS) & STATUS_LU) != 0) {
            return None;
        }
        let mac = read_mac(mmio)?;
        let mut nic = alloc_rings(mmio, mac)?;
        program_rings(&mut nic);
        Some(nic)
    }

    pub fn mac(&self) -> [u8; 6] {
        self.mac
    }

    fn ack_irq(&self) {
        let _ = read32(self.mmio, REG_ICR);
    }

    fn rx_desc(&self, i: usize) -> *mut RxDesc {
        unsafe { self.rx_ring.as_mut_ptr::<RxDesc>().add(i) }
    }

    fn tx_desc(&self, i: usize) -> *mut TxDesc {
        unsafe { self.tx_ring.as_mut_ptr::<TxDesc>().add(i) }
    }

    fn recycle_rx(&mut self, i: usize) {
        let desc = self.rx_desc(i);
        unsafe {
            (*desc).status = 0;
            (*desc).errors = 0;
            (*desc).length = 0;
        }
        fence(Ordering::SeqCst);
        compiler_fence(Ordering::SeqCst);
        write32(self.mmio, REG_RDT, i as u32);
    }

    fn tx_ready(&mut self) -> bool {
        let i = self.tx_i;
        let desc = self.tx_desc(i);
        unsafe { (*desc).status & TX_DD != 0 }
    }

    fn send_frame(&mut self, data: &[u8]) -> bool {
        if data.is_empty() || data.len() > BUF_LEN {
            return false;
        }
        if !self.tx_ready() {
            return false;
        }
        let i = self.tx_i;
        let dst = self.tx_virt[i];
        unsafe {
            core::ptr::copy_nonoverlapping(data.as_ptr(), dst.as_mut_ptr::<u8>(), data.len());
        }
        let desc = self.tx_desc(i);
        unsafe {
            (*desc).addr = self.tx_phys[i].as_u64();
            (*desc).length = data.len() as u16;
            (*desc).status = 0;
            (*desc).cmd = TX_CMD_EOP | TX_CMD_IFCS | TX_CMD_RS;
        }
        fence(Ordering::SeqCst);
        compiler_fence(Ordering::SeqCst);
        let next = (i + 1) % RING;
        write32(self.mmio, REG_TDT, next as u32);
        if !spin_until(|| unsafe { (*desc).status & TX_DD != 0 }) {
            return false;
        }
        self.tx_i = next;
        true
    }
}

impl Device for E1000e {
    type RxToken<'a> = RxTok;
    type TxToken<'a> = TxTok<'a>;

    fn receive(&mut self, _timestamp: Instant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        self.ack_irq();
        let i = self.rx_i;
        let desc = self.rx_desc(i);
        let status = unsafe { (*desc).status };
        if status & RX_DD == 0 {
            return None;
        }
        let errors = unsafe { (*desc).errors };
        let len = unsafe { (*desc).length as usize };
        let bad = errors != 0 || status & RX_EOP == 0 || len == 0 || len > BUF_LEN;
        let tok = if bad {
            self.recycle_rx(i);
            self.rx_i = (i + 1) % RING;
            return None;
        } else {
            let mut frame = [0u8; BUF_LEN];
            let src = self.rx_virt[i];
            unsafe {
                core::ptr::copy_nonoverlapping(src.as_ptr::<u8>(), frame.as_mut_ptr(), len);
            }
            self.recycle_rx(i);
            self.rx_i = (i + 1) % RING;
            RxTok { frame, len }
        };
        Some((tok, TxTok { nic: self }))
    }

    fn transmit(&mut self, _timestamp: Instant) -> Option<Self::TxToken<'_>> {
        self.ack_irq();
        if !self.tx_ready() {
            return None;
        }
        Some(TxTok { nic: self })
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut caps = DeviceCapabilities::default();
        caps.max_transmission_unit = MTU;
        caps.max_burst_size = Some(1);
        caps.medium = Medium::Ethernet;
        caps
    }
}

impl RxToken for RxTok {
    fn consume<R, F>(self, f: F) -> R
    where
        F: FnOnce(&[u8]) -> R,
    {
        f(&self.frame[..self.len])
    }
}

impl TxToken for TxTok<'_> {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        let mut buf = [0u8; BUF_LEN];
        let n = len.min(BUF_LEN);
        let result = f(&mut buf[..n]);
        let _ = self.nic.send_frame(&buf[..n]);
        result
    }
}

fn map_bar0() -> Option<(VirtAddr, u64)> {
    let mut root = PciRoot::new(CamCf8);
    let mut found = None;
    for bus in 0u8..=255 {
        for (df, info) in root.enumerate_bus(bus) {
            if info.vendor_id == VENDOR_INTEL && info.device_id == DEV_82574L {
                found = Some(df);
                break;
            }
        }
        if found.is_some() {
            break;
        }
    }
    let df = found?;
    let (_, mut cmd) = root.get_status_command(df);
    cmd.insert(Command::MEMORY_SPACE | Command::BUS_MASTER | Command::INTERRUPT_DISABLE);
    root.set_command(df, cmd);
    let (addr, size) = bar0(&mut root, df)?;
    if addr == 0 || size == 0 {
        return None;
    }
    Some((
        vmm::map_mmio_range(PhysAddr::new(addr), size as usize),
        size,
    ))
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

fn mask_irq(mmio: VirtAddr) {
    write32(mmio, REG_IMC, 0xFFFF_FFFF);
    write32(mmio, REG_IMS, 0);
    let _ = read32(mmio, REG_ICR);
}

fn reset(mmio: VirtAddr) -> Option<()> {
    write32(mmio, REG_CTRL, read32(mmio, REG_CTRL) | CTRL_RST);
    if !spin_until(|| (read32(mmio, REG_CTRL) & CTRL_RST) == 0) {
        return None;
    }
    mask_irq(mmio);
    let rfctl = read32(mmio, REG_RFCTL);
    write32(mmio, REG_RFCTL, rfctl & !RFCTL_EXSTEN);
    Some(())
}

fn set_link(mmio: VirtAddr) {
    let mut ctrl = read32(mmio, REG_CTRL);
    ctrl |= CTRL_SLU | CTRL_ASDE | CTRL_FD;
    ctrl &= !(CTRL_RFCE | CTRL_TFCE);
    write32(mmio, REG_CTRL, ctrl);
}

fn read_mac(mmio: VirtAddr) -> Option<[u8; 6]> {
    let ral = read32(mmio, REG_RAL0);
    let rah = read32(mmio, REG_RAH0);
    if rah & RAH_AV == 0 {
        return None;
    }
    Some([
        ral as u8,
        (ral >> 8) as u8,
        (ral >> 16) as u8,
        (ral >> 24) as u8,
        rah as u8,
        (rah >> 8) as u8,
    ])
}

fn alloc_rings(mmio: VirtAddr, mac: [u8; 6]) -> Option<E1000e> {
    let rx_ring = alloc_page()?;
    let tx_ring = match alloc_page() {
        Some(p) => p,
        None => {
            free_page(rx_ring);
            return None;
        }
    };
    let mut rx_virt = [VirtAddr::zero(); RING];
    let mut rx_phys = [PhysAddr::zero(); RING];
    let mut n_rx = 0usize;
    let mut tx_virt = [VirtAddr::zero(); RING];
    let mut tx_phys = [PhysAddr::zero(); RING];
    let mut n_tx = 0usize;
    let fail =
        |n_rx: usize, n_tx: usize, rx_phys: &[PhysAddr; RING], tx_phys: &[PhysAddr; RING]| {
            for i in 0..n_rx {
                free_phys(rx_phys[i]);
            }
            for i in 0..n_tx {
                free_phys(tx_phys[i]);
            }
            free_page(rx_ring);
            free_page(tx_ring);
        };
    for i in 0..RING {
        let Some(p) = alloc_page() else {
            fail(n_rx, n_tx, &rx_phys, &tx_phys);
            return None;
        };
        rx_virt[i] = p.1;
        rx_phys[i] = p.0;
        n_rx += 1;
    }
    for i in 0..RING {
        let Some(p) = alloc_page() else {
            fail(n_rx, n_tx, &rx_phys, &tx_phys);
            return None;
        };
        tx_virt[i] = p.1;
        tx_phys[i] = p.0;
        n_tx += 1;
    }
    for i in 0..RING {
        unsafe {
            let d = rx_ring.1.as_mut_ptr::<RxDesc>().add(i);
            *d = RxDesc {
                addr: rx_phys[i].as_u64(),
                length: 0,
                _csum: 0,
                status: 0,
                errors: 0,
                _special: 0,
            };
            let t = tx_ring.1.as_mut_ptr::<TxDesc>().add(i);
            *t = TxDesc {
                addr: tx_phys[i].as_u64(),
                length: 0,
                _cso: 0,
                cmd: 0,
                status: TX_DD,
                _css: 0,
                _special: 0,
            };
        }
    }
    Some(E1000e {
        mmio,
        mac,
        rx_ring: rx_ring.1,
        tx_ring: tx_ring.1,
        rx_ring_phys: rx_ring.0,
        tx_ring_phys: tx_ring.0,
        rx_virt,
        tx_phys,
        tx_virt,
        rx_i: 0,
        tx_i: 0,
    })
}

fn program_rings(nic: &mut E1000e) {
    write32(nic.mmio, REG_RCTL, 0);
    write32(nic.mmio, REG_TCTL, 0);

    write32(nic.mmio, REG_RDBAL, nic.rx_ring_phys.as_u64() as u32);
    write32(nic.mmio, REG_RDBAH, 0);
    write32(nic.mmio, REG_RDLEN, DESC_BYTES);
    write32(nic.mmio, REG_RDH, 0);
    write32(nic.mmio, REG_RDT, (RING - 1) as u32);

    write32(nic.mmio, REG_TDBAL, nic.tx_ring_phys.as_u64() as u32);
    write32(nic.mmio, REG_TDBAH, 0);
    write32(nic.mmio, REG_TDLEN, DESC_BYTES);
    write32(nic.mmio, REG_TDH, 0);
    write32(nic.mmio, REG_TDT, 0);

    write32(nic.mmio, REG_TXDCTL, DCTL_WTHRESH | DCTL_GRAN | DCTL_ENABLE);
    write32(nic.mmio, REG_TCTL, TCTL_EN | TCTL_PSP | TCTL_CT | TCTL_COLD);
    write32(nic.mmio, REG_TIPG, TIPG_COPPER);

    write32(nic.mmio, REG_RXDCTL, DCTL_WTHRESH | DCTL_GRAN | DCTL_ENABLE);
    write32(nic.mmio, REG_RCTL, RCTL_EN | RCTL_BAM | RCTL_SECRC);
    mask_irq(nic.mmio);
}

fn alloc_page() -> Option<(PhysAddr, VirtAddr)> {
    let frame = pmm::alloc()?;
    let phys = frame.start_address();
    if phys.as_u64() >> 32 != 0 {
        pmm::free(frame);
        return None;
    }
    let virt = vmm::phys_to_virt(phys);
    unsafe {
        core::ptr::write_bytes(virt.as_mut_ptr::<u8>(), 0, pmm::FRAME_SIZE as usize);
    }
    Some((phys, virt))
}

fn free_page(p: (PhysAddr, VirtAddr)) {
    free_phys(p.0);
    let _ = p.1;
}

fn free_phys(phys: PhysAddr) {
    if let Ok(f) = PhysFrame::<Size4KiB>::from_start_address(phys) {
        pmm::free(f);
    }
}

fn spin_until(mut pred: impl FnMut() -> bool) -> bool {
    for _ in 0..SPIN_MAX {
        if pred() {
            return true;
        }
        core::hint::spin_loop();
    }
    false
}

fn read32(base: VirtAddr, off: u32) -> u32 {
    unsafe {
        base.as_ptr::<u8>()
            .add(off as usize)
            .cast::<u32>()
            .read_volatile()
    }
}

fn write32(base: VirtAddr, off: u32, val: u32) {
    unsafe {
        base.as_mut_ptr::<u8>()
            .add(off as usize)
            .cast::<u32>()
            .write_volatile(val);
    }
}
