//! USB MSC BOT/BBB on the xHCI slot. One stick, 512-byte sectors.

use crate::xhci::{self, Setup};

const SECTOR: usize = 512;
const CBW_SIG: u32 = 0x4342_5355;
const CSW_SIG: u32 = 0x5342_5355;
const CBW_LEN: usize = 31;
const CSW_LEN: usize = 13;

pub struct UsbMsc {
    sectors: u64,
    tag: u32,
    dev: xhci::Dev,
}

impl UsbMsc {
    pub fn probe() -> Option<Self> {
        if !xhci::present() {
            return None;
        }
        let mut found = [xhci::Dev { hc: 0, slot: 0 }; 4];
        let n = xhci::msc_devs(&mut found);
        for i in 0..n {
            if let Some(disk) = configure_slot(found[i]) {
                return Some(disk);
            }
            xhci::release(found[i]);
        }
        None
    }

    pub fn capacity_sectors(&self) -> u64 {
        self.sectors
    }

    pub fn read_blocks(&mut self, start: u64, buf: &mut [u8]) -> Result<(), ()> {
        if buf.len() % SECTOR != 0 {
            return Err(());
        }
        let mut done = 0;
        let mut lba = start;
        while done < buf.len() {
            let n = (buf.len() - done).min(8 * SECTOR);
            let blocks = (n / SECTOR) as u16;
            self.scsi_in(cdb_read10(lba, blocks), 10, &mut buf[done..done + n])?;
            done += n;
            lba += u64::from(blocks);
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
            let n = (buf.len() - done).min(8 * SECTOR);
            let blocks = (n / SECTOR) as u16;
            self.scsi_out(cdb_write10(lba, blocks), &buf[done..done + n])?;
            done += n;
            lba += u64::from(blocks);
        }
        Ok(())
    }

    pub fn flush(&mut self) -> Result<(), ()> {
        let mut cdb = [0u8; 16];
        cdb[0] = 0x35;
        let _ = self.scsi_nodata(&cdb, 10);
        Ok(())
    }

    fn test_ready(&mut self) -> Result<(), ()> {
        self.scsi_nodata(&[0u8; 16], 6)
    }

    fn request_sense(&mut self) -> Result<(), ()> {
        let mut cdb = [0u8; 16];
        cdb[0] = 0x03;
        cdb[4] = 18;
        let mut sense = [0u8; 18];
        self.scsi_in(cdb, 6, &mut sense)
    }

    fn inquiry(&mut self) -> Result<(), ()> {
        let mut cdb = [0u8; 16];
        cdb[0] = 0x12;
        cdb[4] = 36;
        let mut inq = [0u8; 36];
        self.scsi_in(cdb, 6, &mut inq)
    }

    fn read_capacity(&mut self) -> Option<u64> {
        let mut cdb = [0u8; 16];
        cdb[0] = 0x25;
        let mut cap = [0u8; 8];
        self.scsi_in(cdb, 10, &mut cap).ok()?;
        let last = u32::from_be_bytes(cap[0..4].try_into().ok()?);
        let size = u32::from_be_bytes(cap[4..8].try_into().ok()?);
        if size != SECTOR as u32 {
            return None;
        }
        Some(u64::from(last) + 1)
    }

    fn scsi_nodata(&mut self, cdb: &[u8; 16], cblen: u8) -> Result<(), ()> {
        self.cbw(cdb, 0, false, cblen)?;
        self.csw()
    }

    fn scsi_in(&mut self, cdb: [u8; 16], cblen: u8, data: &mut [u8]) -> Result<(), ()> {
        self.cbw(&cdb, data.len() as u32, true, cblen)?;
        xhci::bulk_in(self.dev, data)?;
        self.csw()
    }

    fn scsi_out(&mut self, cdb: [u8; 16], data: &[u8]) -> Result<(), ()> {
        self.cbw(&cdb, data.len() as u32, false, 10)?;
        xhci::bulk_out(self.dev, data)?;
        self.csw()
    }

    fn cbw(&mut self, cdb: &[u8; 16], len: u32, din: bool, cblen: u8) -> Result<(), ()> {
        let mut p = [0u8; CBW_LEN];
        p[0..4].copy_from_slice(&CBW_SIG.to_le_bytes());
        p[4..8].copy_from_slice(&self.tag.to_le_bytes());
        p[8..12].copy_from_slice(&len.to_le_bytes());
        p[12] = if din { 0x80 } else { 0 };
        p[13] = 0;
        p[14] = cblen;
        p[15..31].copy_from_slice(cdb);
        self.tag = self.tag.wrapping_add(1);
        xhci::bulk_out(self.dev, &p)
    }

    fn csw(&mut self) -> Result<(), ()> {
        let mut p = [0u8; CSW_LEN];
        xhci::bulk_in(self.dev, &mut p)?;
        let sig = u32::from_le_bytes(p[0..4].try_into().map_err(|_| ())?);
        if sig != CSW_SIG {
            return Err(());
        }
        if p[12] != 0 {
            return Err(());
        }
        Ok(())
    }
}

fn configure_slot(dev: xhci::Dev) -> Option<UsbMsc> {
    let mut desc = [0u8; 18];
    ctrl(
        dev,
        Setup {
            ty: 0x80,
            req: 6,
            value: 0x0100,
            index: 0,
            len: 18,
        },
        &mut desc,
    )
    .ok()?;
    let cfg = read_config(dev)?;
    let (iface, value, ep_out, ep_in, max_out, max_in) = parse_msc(&cfg)?;
    xhci::configure_bulk(dev, ep_out, ep_in, max_out, max_in).ok()?;
    set_config(dev, value).ok()?;
    let _ = max_lun(dev, iface);
    let mut disk = UsbMsc {
        sectors: 0,
        tag: 1,
        dev,
    };
    for _ in 0..8 {
        if disk.test_ready().is_ok() {
            break;
        }
        let _ = disk.request_sense();
    }
    let _ = disk.inquiry();
    disk.sectors = disk.read_capacity()?;
    if disk.sectors == 0 {
        return None;
    }
    Some(disk)
}

fn read_config(dev: xhci::Dev) -> Option<[u8; 256]> {
    let mut hdr = [0u8; 9];
    ctrl(
        dev,
        Setup {
            ty: 0x80,
            req: 6,
            value: 0x0200,
            index: 0,
            len: 9,
        },
        &mut hdr,
    )
    .ok()?;
    let total = u16::from_le_bytes([hdr[2], hdr[3]]).min(256) as usize;
    if total < 9 {
        return None;
    }
    let mut cfg = [0u8; 256];
    ctrl(
        dev,
        Setup {
            ty: 0x80,
            req: 6,
            value: 0x0200,
            index: 0,
            len: total as u16,
        },
        &mut cfg[..total],
    )
    .ok()?;
    Some(cfg)
}

fn parse_msc(cfg: &[u8; 256]) -> Option<(u16, u16, u8, u8, u16, u16)> {
    let total = u16::from_le_bytes([cfg[2], cfg[3]]).min(256) as usize;
    let value = u16::from(cfg[5]);
    let mut i = 9usize;
    let mut iface = 0u16;
    let mut want = false;
    let mut ep_out = 0u8;
    let mut ep_in = 0u8;
    let mut max_out = 64u16;
    let mut max_in = 64u16;
    while i + 2 <= total {
        let len = cfg[i] as usize;
        if len < 2 || i + len > total {
            break;
        }
        match cfg[i + 1] {
            4 if len >= 9 => {
                want = cfg[i + 5] == 8 && cfg[i + 6] == 6 && cfg[i + 7] == 0x50;
                iface = u16::from(cfg[i + 2]);
            }
            5 if want && len >= 7 => {
                let addr = cfg[i + 2];
                let attr = cfg[i + 3] & 3;
                let max = u16::from_le_bytes([cfg[i + 4], cfg[i + 5]]);
                if attr == 2 {
                    if addr & 0x80 != 0 {
                        ep_in = addr;
                        max_in = max;
                    } else {
                        ep_out = addr;
                        max_out = max;
                    }
                }
            }
            _ => {}
        }
        i += len;
    }
    if ep_out == 0 || ep_in == 0 {
        return None;
    }
    Some((iface, value, ep_out, ep_in, max_out, max_in))
}

fn set_config(dev: xhci::Dev, value: u16) -> Result<(), ()> {
    ctrl(
        dev,
        Setup {
            ty: 0x00,
            req: 9,
            value,
            index: 0,
            len: 0,
        },
        &mut [],
    )
    .map(|_| ())
}

fn max_lun(dev: xhci::Dev, iface: u16) -> u8 {
    let mut b = [0u8; 1];
    match ctrl(
        dev,
        Setup {
            ty: 0xA1,
            req: 0xFE,
            value: 0,
            index: iface,
            len: 1,
        },
        &mut b,
    ) {
        Ok(_) => b[0],
        Err(()) => 0,
    }
}

fn ctrl(dev: xhci::Dev, setup: Setup, data: &mut [u8]) -> Result<usize, ()> {
    xhci::control(dev, setup, data)
}

fn cdb_read10(lba: u64, blocks: u16) -> [u8; 16] {
    let mut c = [0u8; 16];
    c[0] = 0x28;
    let l = lba as u32;
    c[2] = (l >> 24) as u8;
    c[3] = (l >> 16) as u8;
    c[4] = (l >> 8) as u8;
    c[5] = l as u8;
    c[7] = (blocks >> 8) as u8;
    c[8] = blocks as u8;
    c
}

fn cdb_write10(lba: u64, blocks: u16) -> [u8; 16] {
    let mut c = cdb_read10(lba, blocks);
    c[0] = 0x2A;
    c
}
