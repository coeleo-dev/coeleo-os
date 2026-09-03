//! AHCI: every HBA and PI ATA port, poll I/O, DMA bounce. TCB MMIO + DMA.

use alloc::vec::Vec;
use core::sync::atomic::{Ordering, compiler_fence};

use virtio_drivers::transport::pci::bus::{BarInfo, Command, PciRoot};
use x86_64::{PhysAddr, VirtAddr};

use crate::clock;
use crate::pci::CamCf8;
use crate::{pmm, vmm};

const SECTOR: usize = 512;
const SPIN_MAX: u32 = 50_000_000;
const TIMEOUT_TICKS: u64 = 500;

const GHC: u32 = 0x04;
const PI: u32 = 0x0C;
const CAP2: u32 = 0x24;
const BOHC: u32 = 0x28;

const GHC_AE: u32 = 1 << 31;
const GHC_IE: u32 = 1 << 1;
const CAP2_BOH: u32 = 1 << 0;
const BOHC_BOS: u32 = 1 << 0;
const BOHC_OOS: u32 = 1 << 1;

const P_CLB: u32 = 0x00;
const P_CLBU: u32 = 0x04;
const P_FB: u32 = 0x08;
const P_FBU: u32 = 0x0C;
const P_IS: u32 = 0x10;
const P_IE: u32 = 0x14;
const P_CMD: u32 = 0x18;
const P_TFD: u32 = 0x20;
const P_SIG: u32 = 0x24;
const P_SSTS: u32 = 0x28;
const P_SERR: u32 = 0x30;
const P_CI: u32 = 0x38;

const CMD_ST: u32 = 1 << 0;
const CMD_SUD: u32 = 1 << 1;
const CMD_POD: u32 = 1 << 2;
const CMD_FRE: u32 = 1 << 4;
const CMD_FR: u32 = 1 << 14;
const CMD_CR: u32 = 1 << 15;

const TFD_ERR: u32 = 1 << 0;
const TFD_DRQ: u32 = 1 << 3;
const TFD_BSY: u32 = 1 << 7;
const IS_TFES: u32 = 1 << 30;

const DET_MASK: u32 = 0xF;
const DET_COMM: u32 = 3;
const SIG_ATA: u32 = 0x0000_0101;

const FIS_H2D: u8 = 0x27;
const ATA_IDENTIFY: u8 = 0xEC;
const ATA_READ_DMA_EXT: u8 = 0x25;
const ATA_WRITE_DMA_EXT: u8 = 0x35;
const ATA_FLUSH_EXT: u8 = 0xEA;

const CLASS_STORAGE: u8 = 0x01;
const SUBCLASS_SATA: u8 = 0x06;
const PROG_AHCI: u8 = 0x01;

pub struct AhciDisk {
    hba: VirtAddr,
    port: u8,
    capacity: u64,
    ct_phys: PhysAddr,
    bounce_phys: PhysAddr,
    bounce: VirtAddr,
}

impl AhciDisk {
    pub fn probe_all(out: &mut Vec<Self>, max: usize) {
        if max == 0 {
            return;
        }
        let mut root = PciRoot::new(CamCf8);
        let mut hbas = Vec::new();
        for bus in 0u8..=255 {
            for (df, info) in root.enumerate_bus(bus) {
                if info.class == CLASS_STORAGE
                    && info.subclass == SUBCLASS_SATA
                    && info.prog_if == PROG_AHCI
                {
                    hbas.push(df);
                }
            }
        }
        for df in hbas {
            if out.len() >= max {
                return;
            }
            let Some(hba) = map_hba(&mut root, df) else {
                continue;
            };
            bios_handoff(hba);
            enable_ahci(hba);
            let pi = read32(hba, PI);
            for n in 0u8..32 {
                if out.len() >= max {
                    return;
                }
                if pi & (1 << n) == 0 {
                    continue;
                }
                if let Some(disk) = init_port(hba, n) {
                    out.push(disk);
                }
            }
        }
    }

    pub fn capacity_sectors(&self) -> u64 {
        self.capacity
    }

    pub fn read_blocks(&mut self, start: u64, buf: &mut [u8]) -> Result<(), ()> {
        if buf.len() % SECTOR != 0 {
            return Err(());
        }
        let mut done = 0;
        let mut lba = start;
        while done < buf.len() {
            let n = (buf.len() - done).min(pmm::FRAME_SIZE as usize);
            let count = (n / SECTOR) as u16;
            issue(self, ATA_READ_DMA_EXT, lba, count, false, Some(n))?;
            // SAFETY: bounce is our DMA frame; `buf` is a valid destination.
            unsafe {
                core::ptr::copy_nonoverlapping(
                    self.bounce.as_ptr::<u8>(),
                    buf[done..].as_mut_ptr(),
                    n,
                );
            }
            done += n;
            lba += u64::from(count);
        }
        Ok(())
    }

    pub fn write_blocks(&mut self, start: u64, buf: &[u8]) -> Result<(), ()> {
        if buf.is_empty() || buf.len() % SECTOR != 0 {
            return Err(());
        }
        let mut done = 0;
        let mut lba = start;
        while done < buf.len() {
            let n = (buf.len() - done).min(pmm::FRAME_SIZE as usize);
            let count = (n / SECTOR) as u16;
            unsafe {
                core::ptr::copy_nonoverlapping(
                    buf[done..].as_ptr(),
                    self.bounce.as_mut_ptr::<u8>(),
                    n,
                );
            }
            issue(self, ATA_WRITE_DMA_EXT, lba, count, true, Some(n))?;
            done += n;
            lba += u64::from(count);
        }
        Ok(())
    }

    pub fn flush(&mut self) -> Result<(), ()> {
        issue(self, ATA_FLUSH_EXT, 0, 0, false, None)
    }
}

fn map_hba(
    root: &mut PciRoot<CamCf8>,
    df: virtio_drivers::transport::pci::bus::DeviceFunction,
) -> Option<VirtAddr> {
    let (_, mut cmd) = root.get_status_command(df);
    cmd.insert(Command::MEMORY_SPACE | Command::BUS_MASTER | Command::INTERRUPT_DISABLE);
    root.set_command(df, cmd);

    let (addr, size) = abar(root, df)?;
    if addr == 0 || size == 0 {
        return None;
    }
    Some(vmm::map_mmio_range(PhysAddr::new(addr), size as usize))
}

fn abar(
    root: &mut PciRoot<CamCf8>,
    df: virtio_drivers::transport::pci::bus::DeviceFunction,
) -> Option<(u64, u64)> {
    if let Ok(Some(BarInfo::Memory { address, size, .. })) = root.bar_info(df, 5) {
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

fn bios_handoff(hba: VirtAddr) {
    if read32(hba, CAP2) & CAP2_BOH == 0 {
        return;
    }
    let mut bohc = read32(hba, BOHC);
    bohc |= BOHC_OOS;
    write32(hba, BOHC, bohc);
    let _ = wait_until(|| read32(hba, BOHC) & BOHC_BOS == 0);
}

fn enable_ahci(hba: VirtAddr) {
    let mut ghc = read32(hba, GHC);
    ghc |= GHC_AE;
    ghc &= !GHC_IE;
    write32(hba, GHC, ghc);
}

fn init_port(hba: VirtAddr, port: u8) -> Option<AhciDisk> {
    let ssts = pread(hba, port, P_SSTS);
    if ssts & DET_MASK != DET_COMM {
        return None;
    }
    if pread(hba, port, P_SIG) != SIG_ATA {
        return None;
    }
    stop_port(hba, port)?;

    let cl_frame = pmm::alloc_contiguous(1)?;
    let fis_frame = pmm::alloc_contiguous(1)?;
    let bounce_frame = pmm::alloc_contiguous(1)?;
    let cl_phys = cl_frame.start_address();
    let fis_phys = fis_frame.start_address();
    let bounce_phys = bounce_frame.start_address();
    let cl_virt = vmm::phys_to_virt(cl_phys);
    let fis_virt = vmm::phys_to_virt(fis_phys);
    let bounce = vmm::phys_to_virt(bounce_phys);
    // SAFETY: freshly allocated PMM frames, mapped in the HHDM.
    unsafe {
        core::ptr::write_bytes(cl_virt.as_mut_ptr::<u8>(), 0, pmm::FRAME_SIZE as usize);
        core::ptr::write_bytes(fis_virt.as_mut_ptr::<u8>(), 0, pmm::FRAME_SIZE as usize);
        core::ptr::write_bytes(bounce.as_mut_ptr::<u8>(), 0, pmm::FRAME_SIZE as usize);
    }

    // Command table lives in the FIS page at offset 256 (128-byte aligned, after RFIS).
    let ct_phys = PhysAddr::new(fis_phys.as_u64() + 256);

    pwrite(hba, port, P_CLB, cl_phys.as_u64() as u32);
    pwrite(hba, port, P_CLBU, (cl_phys.as_u64() >> 32) as u32);
    pwrite(hba, port, P_FB, fis_phys.as_u64() as u32);
    pwrite(hba, port, P_FBU, (fis_phys.as_u64() >> 32) as u32);

    pwrite(hba, port, P_IE, 0);
    pwrite(hba, port, P_SERR, u32::MAX);
    pwrite(hba, port, P_IS, u32::MAX);

    let mut cmd = pread(hba, port, P_CMD);
    cmd |= CMD_SUD | CMD_POD;
    pwrite(hba, port, P_CMD, cmd);
    cmd |= CMD_FRE;
    pwrite(hba, port, P_CMD, cmd);
    cmd |= CMD_ST;
    pwrite(hba, port, P_CMD, cmd);

    wait_until(|| pread(hba, port, P_TFD) & (TFD_BSY | TFD_DRQ) == 0)?;

    let mut disk = AhciDisk {
        hba,
        port,
        capacity: 0,
        ct_phys,
        bounce_phys,
        bounce,
    };
    let mut ident = [0u8; SECTOR];
    issue(&mut disk, ATA_IDENTIFY, 0, 1, false, Some(SECTOR)).ok()?;
    unsafe {
        core::ptr::copy_nonoverlapping(disk.bounce.as_ptr::<u8>(), ident.as_mut_ptr(), SECTOR);
    }
    if !logical_sector_512(&ident) {
        return None;
    }
    disk.capacity = identify_sectors(&ident)?;
    if disk.capacity == 0 {
        return None;
    }
    Some(disk)
}

fn stop_port(hba: VirtAddr, port: u8) -> Option<()> {
    let mut cmd = pread(hba, port, P_CMD);
    if cmd & CMD_ST != 0 {
        cmd &= !CMD_ST;
        pwrite(hba, port, P_CMD, cmd);
        wait_until(|| pread(hba, port, P_CMD) & CMD_CR == 0)?;
    }
    cmd = pread(hba, port, P_CMD);
    if cmd & CMD_FRE != 0 {
        cmd &= !CMD_FRE;
        pwrite(hba, port, P_CMD, cmd);
        wait_until(|| pread(hba, port, P_CMD) & CMD_FR == 0)?;
    }
    Some(())
}

fn issue(
    disk: &mut AhciDisk,
    ata_cmd: u8,
    lba: u64,
    count: u16,
    write: bool,
    data_len: Option<usize>,
) -> Result<(), ()> {
    wait_until(|| pread(disk.hba, disk.port, P_TFD) & (TFD_BSY | TFD_DRQ) == 0).ok_or(())?;

    let ct = vmm::phys_to_virt(disk.ct_phys);
    // SAFETY: command table is our DMA frame, 256 bytes into the FIS page.
    unsafe {
        core::ptr::write_bytes(ct.as_mut_ptr::<u8>(), 0, 256);
    }

    let mut fis = [0u8; 20];
    fis[0] = FIS_H2D;
    fis[1] = 1 << 7;
    fis[2] = ata_cmd;
    fis[4] = lba as u8;
    fis[5] = (lba >> 8) as u8;
    fis[6] = (lba >> 16) as u8;
    fis[7] = 0x40;
    fis[8] = (lba >> 24) as u8;
    fis[9] = (lba >> 32) as u8;
    fis[10] = (lba >> 40) as u8;
    fis[12] = count as u8;
    fis[13] = (count >> 8) as u8;
    unsafe {
        core::ptr::copy_nonoverlapping(fis.as_ptr(), ct.as_mut_ptr::<u8>(), 20);
    }

    let prdtl: u16 = if data_len.is_some() { 1 } else { 0 };
    if let Some(len) = data_len {
        let prd = ct.as_u64() + 0x80;
        let prd_ptr = VirtAddr::new(prd);
        let dbc = (n_minus_one(len)? as u32) | 0;
        unsafe {
            let p = prd_ptr.as_mut_ptr::<u32>();
            p.write_volatile(disk.bounce_phys.as_u64() as u32);
            p.add(1)
                .write_volatile((disk.bounce_phys.as_u64() >> 32) as u32);
            p.add(2).write_volatile(0);
            p.add(3).write_volatile(dbc);
        }
    }

    let cl = vmm::phys_to_virt(PhysAddr::new(
        // command list is the page that contains slot 0; we stored it in PxCLB.
        pread(disk.hba, disk.port, P_CLB) as u64
            | (u64::from(pread(disk.hba, disk.port, P_CLBU)) << 32),
    ));
    let mut dw0: u32 = 5; // CFL = 5 dwords
    if write {
        dw0 |= 1 << 6;
    }
    dw0 |= u32::from(prdtl) << 16;
    let ctba = disk.ct_phys.as_u64();
    unsafe {
        let hdr = cl.as_mut_ptr::<u32>();
        hdr.write_volatile(dw0);
        hdr.add(1).write_volatile(0);
        hdr.add(2).write_volatile(ctba as u32);
        hdr.add(3).write_volatile((ctba >> 32) as u32);
        hdr.add(4).write_volatile(0);
        hdr.add(5).write_volatile(0);
        hdr.add(6).write_volatile(0);
        hdr.add(7).write_volatile(0);
    }

    pwrite(disk.hba, disk.port, P_IS, u32::MAX);
    compiler_fence(Ordering::SeqCst);
    pwrite(disk.hba, disk.port, P_CI, 1);

    wait_until(|| pread(disk.hba, disk.port, P_CI) & 1 == 0).ok_or(())?;

    let tfd = pread(disk.hba, disk.port, P_TFD);
    let is = pread(disk.hba, disk.port, P_IS);
    if tfd & TFD_ERR != 0 || is & IS_TFES != 0 {
        pwrite(disk.hba, disk.port, P_IS, u32::MAX);
        return Err(());
    }
    Ok(())
}

fn n_minus_one(len: usize) -> Result<u32, ()> {
    if len == 0 {
        return Err(());
    }
    u32::try_from(len - 1).map_err(|_| ())
}

fn identify_sectors(ident: &[u8; SECTOR]) -> Option<u64> {
    let w = |i: usize| u16::from_le_bytes([ident[i * 2], ident[i * 2 + 1]]);
    let lba48 = u64::from(w(100))
        | (u64::from(w(101)) << 16)
        | (u64::from(w(102)) << 32)
        | (u64::from(w(103)) << 48);
    let lba28 = u64::from(u32::from(w(60)) | (u32::from(w(61)) << 16));
    let sectors = if lba48 != 0 { lba48 } else { lba28 };
    (sectors != 0).then_some(sectors)
}

/// ATA ACS: word 106 is valid when bit 14 is set and bit 15 is clear.
/// Bit 12 means words 117–118 hold the logical sector size in 16-bit words.
fn logical_sector_512(ident: &[u8; SECTOR]) -> bool {
    let w = |i: usize| u16::from_le_bytes([ident[i * 2], ident[i * 2 + 1]]);
    let w106 = w(106);
    if w106 & 0xC000 != 0x4000 {
        return true;
    }
    if w106 & (1 << 12) == 0 {
        return true;
    }
    let words = u32::from(w(117)) | (u32::from(w(118)) << 16);
    words.saturating_mul(2) == 512
}

fn wait_until(mut pred: impl FnMut() -> bool) -> Option<()> {
    let start = clock::ticks();
    for _ in 0..SPIN_MAX {
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

fn port_off(port: u8, reg: u32) -> u32 {
    0x100 + u32::from(port) * 0x80 + reg
}

fn pread(hba: VirtAddr, port: u8, reg: u32) -> u32 {
    read32(hba, port_off(port, reg))
}

fn pwrite(hba: VirtAddr, port: u8, reg: u32, val: u32) {
    write32(hba, port_off(port, reg), val);
}

fn read32(base: VirtAddr, off: u32) -> u32 {
    // SAFETY: `base` is the mapped AHCI ABAR; `off` is a 32-bit register.
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
