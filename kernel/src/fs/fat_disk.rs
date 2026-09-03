//! Byte-level FAT volume over the block device.

use fatfs::{IoBase, Read, Seek, SeekFrom, Write};

use crate::blk::{self, SECTOR_SIZE};

pub struct FatDisk {
    pos: u64,
    disk: usize,
    start_lba: u64,
    size: u64,
    bounce: [u8; SECTOR_SIZE],
    cached_lba: Option<u64>,
}

impl FatDisk {
    pub fn volume(disk: usize, start_lba: u64, size: u64) -> Self {
        Self {
            pos: 0,
            disk,
            start_lba,
            size,
            bounce: [0; SECTOR_SIZE],
            cached_lba: None,
        }
    }
}

impl IoBase for FatDisk {
    type Error = ();
}

impl Read for FatDisk {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, ()> {
        if buf.is_empty() || self.pos >= self.size {
            return Ok(0);
        }
        let mut done = 0;
        while done < buf.len() && self.pos < self.size {
            let lba = self.pos / SECTOR_SIZE as u64;
            let off = (self.pos % SECTOR_SIZE as u64) as usize;
            if self.cached_lba != Some(lba) {
                blk::read_blocks_at(self.disk, self.start_lba + lba, &mut self.bounce)?;
                self.cached_lba = Some(lba);
            }
            let avail = (SECTOR_SIZE - off).min(buf.len() - done);
            let remain = (self.size - self.pos) as usize;
            let n = avail.min(remain);
            buf[done..done + n].copy_from_slice(&self.bounce[off..off + n]);
            self.pos += n as u64;
            done += n;
        }
        Ok(done)
    }
}

impl Write for FatDisk {
    fn write(&mut self, buf: &[u8]) -> Result<usize, ()> {
        if buf.is_empty() {
            return Ok(0);
        }
        if self.pos >= self.size {
            return Ok(0);
        }
        let mut done = 0;
        while done < buf.len() && self.pos < self.size {
            let lba = self.pos / SECTOR_SIZE as u64;
            let off = (self.pos % SECTOR_SIZE as u64) as usize;
            let remain_sector = SECTOR_SIZE - off;
            let remain_disk = (self.size - self.pos) as usize;
            let n = remain_sector.min(buf.len() - done).min(remain_disk);
            if off == 0 && n == SECTOR_SIZE {
                self.bounce.copy_from_slice(&buf[done..done + n]);
            } else {
                if self.cached_lba != Some(lba) {
                    blk::read_blocks_at(self.disk, self.start_lba + lba, &mut self.bounce)?;
                }
                self.bounce[off..off + n].copy_from_slice(&buf[done..done + n]);
            }
            blk::write_blocks_at(self.disk, self.start_lba + lba, &self.bounce)?;
            self.cached_lba = Some(lba);
            self.pos += n as u64;
            done += n;
        }
        Ok(done)
    }

    fn flush(&mut self) -> Result<(), ()> {
        Ok(())
    }
}

impl Seek for FatDisk {
    fn seek(&mut self, pos: SeekFrom) -> Result<u64, ()> {
        let new = match pos {
            SeekFrom::Start(n) => n,
            SeekFrom::Current(off) => {
                let p = self.pos as i128 + i128::from(off);
                if p < 0 {
                    return Err(());
                }
                p as u64
            }
            SeekFrom::End(off) => {
                let p = self.size as i128 + i128::from(off);
                if p < 0 {
                    return Err(());
                }
                p as u64
            }
        };
        self.pos = new.min(self.size);
        Ok(self.pos)
    }
}
