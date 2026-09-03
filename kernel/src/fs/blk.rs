//! Indexed block devices: virtio-blk, then AHCI, then one USB MSC stick.

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write;

use spin::Mutex;
use virtio_drivers::device::blk::VirtIOBlk;
use virtio_drivers::transport::DeviceType;
use virtio_drivers::transport::pci::PciTransport;

use crate::ahci::AhciDisk;
use crate::usb_msc::UsbMsc;
use crate::virtio_hal::VirtioHal;

pub const SECTOR_SIZE: usize = virtio_drivers::device::blk::SECTOR_SIZE;
pub const MAX_DISKS: usize = 8;
pub const KIND_VIRTIO: u32 = 0;
pub const KIND_AHCI: u32 = 1;
pub const KIND_USB: u32 = 2;
pub const FLAG_LIVE: u32 = 1;
pub const FLAG_SMALL: u32 = 2;
const MIN_SECTORS: u64 = 64 * 1024 * 1024 / SECTOR_SIZE as u64;
const DISK_HDR: usize = 8;
const DISK_ROW: usize = 24;

type VirtioBlk = VirtIOBlk<VirtioHal, PciTransport>;

enum BlockDev {
    Virtio(VirtioBlk),
    Ahci(AhciDisk),
    Usb(UsbMsc),
}

struct Table {
    disks: Vec<BlockDev>,
    live_root: Option<usize>,
}

static TABLE: Mutex<Option<Table>> = Mutex::new(None);

pub fn init() {
    let mut disks = Vec::new();
    if let Some(virtio) = probe_virtio() {
        disks.push(BlockDev::Virtio(virtio));
    }
    let mut ahci = Vec::new();
    AhciDisk::probe_all(&mut ahci, MAX_DISKS.saturating_sub(disks.len()));
    for d in ahci {
        if disks.len() >= MAX_DISKS {
            break;
        }
        disks.push(BlockDev::Ahci(d));
    }
    crate::xhci::init();
    crate::xhci::enumerate();
    if disks.len() < MAX_DISKS {
        if let Some(usb) = UsbMsc::probe() {
            disks.push(BlockDev::Usb(usb));
        }
    }
    *TABLE.lock() = Some(Table {
        disks,
        live_root: None,
    });
}

pub fn count() -> usize {
    TABLE.lock().as_ref().map(|t| t.disks.len()).unwrap_or(0)
}

pub fn live_root() -> Option<usize> {
    TABLE.lock().as_ref()?.live_root
}

pub fn set_live_root(i: usize) {
    let mut g = TABLE.lock();
    if let Some(t) = g.as_mut() {
        if i < t.disks.len() {
            t.live_root = Some(i);
        }
    }
}

pub fn clear_live_root() {
    if let Some(t) = TABLE.lock().as_mut() {
        t.live_root = None;
    }
}

pub fn kind_at(i: usize) -> Option<&'static str> {
    match TABLE.lock().as_ref()?.disks.get(i)? {
        BlockDev::Virtio(_) => Some("virtio-blk"),
        BlockDev::Ahci(_) => Some("ahci"),
        BlockDev::Usb(_) => Some("usb"),
    }
}

pub fn kind() -> Option<&'static str> {
    kind_at(live_root()?)
}

pub fn capacity_sectors_at(i: usize) -> Option<u64> {
    match TABLE.lock().as_ref()?.disks.get(i)? {
        BlockDev::Virtio(blk) => Some(blk.capacity()),
        BlockDev::Ahci(ahci) => Some(ahci.capacity_sectors()),
        BlockDev::Usb(usb) => Some(usb.capacity_sectors()),
    }
}

pub fn capacity_sectors() -> Option<u64> {
    capacity_sectors_at(live_root()?)
}

pub fn capacity_bytes_at(i: usize) -> Option<u64> {
    capacity_sectors_at(i).map(|s| s * SECTOR_SIZE as u64)
}

pub fn capacity_bytes() -> Option<u64> {
    capacity_bytes_at(live_root()?)
}

pub fn read_blocks_at(disk: usize, start_sector: u64, buf: &mut [u8]) -> Result<(), ()> {
    if buf.len() % SECTOR_SIZE != 0 {
        return Err(());
    }
    let mut g = TABLE.lock();
    let t = g.as_mut().ok_or(())?;
    match t.disks.get_mut(disk).ok_or(())? {
        BlockDev::Virtio(blk) => {
            let start = usize::try_from(start_sector).map_err(|_| ())?;
            blk.read_blocks(start, buf).map_err(|_| ())
        }
        BlockDev::Ahci(ahci) => ahci.read_blocks(start_sector, buf),
        BlockDev::Usb(usb) => usb.read_blocks(start_sector, buf),
    }
}

pub fn write_blocks_at(disk: usize, start_sector: u64, buf: &[u8]) -> Result<(), ()> {
    if buf.is_empty() || buf.len() % SECTOR_SIZE != 0 {
        return Err(());
    }
    let mut g = TABLE.lock();
    let t = g.as_mut().ok_or(())?;
    match t.disks.get_mut(disk).ok_or(())? {
        BlockDev::Virtio(blk) => {
            let start = usize::try_from(start_sector).map_err(|_| ())?;
            blk.write_blocks(start, buf).map_err(|_| ())
        }
        BlockDev::Ahci(ahci) => ahci.write_blocks(start_sector, buf),
        BlockDev::Usb(usb) => usb.write_blocks(start_sector, buf),
    }
}

pub fn flush_at(disk: usize) -> Result<(), ()> {
    let mut g = TABLE.lock();
    let t = g.as_mut().ok_or(())?;
    match t.disks.get_mut(disk).ok_or(())? {
        BlockDev::Virtio(_) => Ok(()),
        BlockDev::Ahci(ahci) => ahci.flush(),
        BlockDev::Usb(usb) => usb.flush(),
    }
}

pub fn read_blocks(start_sector: u64, buf: &mut [u8]) -> Result<(), ()> {
    read_blocks_at(live_root().ok_or(())?, start_sector, buf)
}

pub fn write_blocks(start_sector: u64, buf: &[u8]) -> Result<(), ()> {
    write_blocks_at(live_root().ok_or(())?, start_sector, buf)
}

pub fn flush() -> Result<(), ()> {
    flush_at(live_root().ok_or(())?)
}

pub fn format_table(out: &mut String) {
    let n = count();
    if n == 0 {
        out.push_str("disk: none\n");
        return;
    }
    for i in 0..n {
        let Some(kind) = kind_at(i) else {
            continue;
        };
        let Some(sectors) = capacity_sectors_at(i) else {
            continue;
        };
        let mut probe = [0u8; SECTOR_SIZE];
        if read_blocks_at(i, 0, &mut probe).is_err() {
            let _ = write!(out, "disk: {i} {kind} read failed\n");
            continue;
        }
        let mib = sectors * 512 / (1024 * 1024);
        let _ = write!(out, "disk: {i} {kind} {mib} MiB ({sectors} sectors)\n");
    }
}

/// Packed table: `u32 count`, `u32 pad`, then `count` rows of 24 bytes.
pub fn sys_disks(buf: u64, len: u64) -> u64 {
    const ERR: u64 = u64::MAX;
    if len < DISK_HDR as u64 || !crate::vmm::user_slice_ok(buf, len) {
        return ERR;
    }
    let n = count().min(MAX_DISKS);
    let need = DISK_HDR + n * DISK_ROW;
    if (len as usize) < need {
        return ERR;
    }
    let mut out = [0u8; DISK_HDR + MAX_DISKS * DISK_ROW];
    out[0..4].copy_from_slice(&(n as u32).to_le_bytes());
    let live = live_root();
    for i in 0..n {
        let kind = match kind_at(i) {
            Some("virtio-blk") => KIND_VIRTIO,
            Some("ahci") => KIND_AHCI,
            Some("usb") => KIND_USB,
            _ => continue,
        };
        let sectors = capacity_sectors_at(i).unwrap_or(0);
        let mut flags = 0u32;
        if live == Some(i) {
            flags |= FLAG_LIVE;
        }
        if sectors < MIN_SECTORS {
            flags |= FLAG_SMALL;
        }
        let o = DISK_HDR + i * DISK_ROW;
        out[o..o + 4].copy_from_slice(&(i as u32).to_le_bytes());
        out[o + 4..o + 8].copy_from_slice(&kind.to_le_bytes());
        out[o + 8..o + 12].copy_from_slice(&flags.to_le_bytes());
        out[o + 16..o + 24].copy_from_slice(&sectors.to_le_bytes());
    }
    if crate::fd::copy_to_user(buf, &out[..need]).is_err() {
        return ERR;
    }
    need as u64
}

fn probe_virtio() -> Option<VirtioBlk> {
    let transport = crate::virtio_pci::open(DeviceType::Block)?;
    let mut blk = VirtIOBlk::<VirtioHal, _>::new(transport).ok()?;
    blk.disable_interrupts();
    Some(blk)
}
