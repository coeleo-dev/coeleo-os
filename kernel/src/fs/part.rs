//! MBR and GPT partition scan. No unsafe.

use alloc::vec::Vec;

use crate::blk::{self, SECTOR_SIZE};

pub struct Volume {
    pub start_lba: u64,
    pub sectors: u64,
}

/// EFI System Partition type GUID as stored on disk (mixed-endian).
pub(crate) const GUID_ESP: [u8; 16] = [
    0x28, 0x73, 0x2A, 0xC1, 0x1F, 0xF8, 0xD2, 0x11, 0xBA, 0x4B, 0x00, 0xA0, 0xC9, 0x3E, 0xC9, 0x3B,
];
/// Microsoft Basic Data type GUID as stored on disk (mixed-endian).
const GUID_BASIC: [u8; 16] = [
    0xA2, 0xA0, 0xD0, 0xEB, 0xE5, 0xB9, 0x33, 0x44, 0x87, 0xC0, 0x68, 0xB6, 0xB7, 0x26, 0x99, 0xC7,
];

pub fn volumes(disk: usize) -> Vec<Volume> {
    let mut sector = [0u8; SECTOR_SIZE];
    if blk::read_blocks_at(disk, 0, &mut sector).is_err() {
        return Vec::new();
    }
    let mut gpt_hdr = [0u8; SECTOR_SIZE];
    if blk::read_blocks_at(disk, 1, &mut gpt_hdr).is_ok() && gpt_hdr.starts_with(b"EFI PART") {
        return gpt_volumes(disk, &gpt_hdr);
    }
    mbr_volumes(&sector)
}

fn gpt_volumes(disk: usize, header: &[u8; SECTOR_SIZE]) -> Vec<Volume> {
    let part_lba = u64::from_le_bytes(header[72..80].try_into().unwrap_or([0; 8]));
    let num = u32::from_le_bytes(header[80..84].try_into().unwrap_or([0; 4]));
    let ent_size = u32::from_le_bytes(header[84..88].try_into().unwrap_or([0; 4]));
    if part_lba == 0 || num == 0 || ent_size < 128 {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut sector = [0u8; SECTOR_SIZE];
    let max = num.min(128);
    for i in 0..max {
        let off = u64::from(i) * u64::from(ent_size);
        let lba = part_lba + off / SECTOR_SIZE as u64;
        let within = (off % SECTOR_SIZE as u64) as usize;
        if blk::read_blocks_at(disk, lba, &mut sector).is_err() {
            break;
        }
        if within + 128 > SECTOR_SIZE {
            continue;
        }
        let ent = &sector[within..within + 128];
        let ty = &ent[0..16];
        if ty == [0u8; 16] {
            continue;
        }
        if ty != GUID_ESP && ty != GUID_BASIC {
            continue;
        }
        let first = u64::from_le_bytes(ent[32..40].try_into().unwrap_or([0; 8]));
        let last = u64::from_le_bytes(ent[40..48].try_into().unwrap_or([0; 8]));
        if last < first {
            continue;
        }
        out.push(Volume {
            start_lba: first,
            sectors: last - first + 1,
        });
    }
    out
}

fn mbr_volumes(sector: &[u8; SECTOR_SIZE]) -> Vec<Volume> {
    if sector[510] != 0x55 || sector[511] != 0xAA {
        return Vec::new();
    }
    let mut out = Vec::new();
    for i in 0..4 {
        let e = &sector[446 + i * 16..446 + (i + 1) * 16];
        let ty = e[4];
        if ty != 0x0B && ty != 0x0C && ty != 0xEF {
            continue;
        }
        let start = u32::from_le_bytes(e[8..12].try_into().unwrap_or([0; 4]));
        let count = u32::from_le_bytes(e[12..16].try_into().unwrap_or([0; 4]));
        if start == 0 || count == 0 {
            continue;
        }
        out.push(Volume {
            start_lba: u64::from(start),
            sectors: u64::from(count),
        });
    }
    out
}
