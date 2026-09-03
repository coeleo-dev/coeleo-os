//! Protective MBR + GPT with one ESP. Poll writes through `blk`.

use crate::blk::{self, SECTOR_SIZE};
use crate::clock;
use crate::part::GUID_ESP;

pub const ESP_START_LBA: u64 = 2048;
const ENTRIES: u32 = 128;
const ENTRY_SIZE: u32 = 128;
const ARRAY_SECTORS: u64 = 32;
const HEADER_SIZE: u32 = 92;
const MIN_SECTORS: u64 = 64 * 1024 * 1024 / SECTOR_SIZE as u64;

pub fn last_usable(sectors: u64) -> Option<u64> {
    if sectors < MIN_SECTORS {
        return None;
    }
    let last = sectors - 1;
    last.checked_sub(ARRAY_SECTORS + 1)
}

pub fn write_esp(disk: usize) -> Result<(), ()> {
    let sectors = blk::capacity_sectors_at(disk).ok_or(())?;
    let last_usable = last_usable(sectors).ok_or(())?;
    if last_usable < ESP_START_LBA {
        return Err(());
    }
    let last = sectors - 1;
    let backup_array = last - ARRAY_SECTORS;
    let disk_guid = make_guid(disk, 0xD15C);
    let part_guid = make_guid(disk, 0xE5F);
    let entry = partition_entry(part_guid, ESP_START_LBA, last_usable);
    let array_crc = array_crc32(&entry);

    let mut primary = [0u8; SECTOR_SIZE];
    fill_header(
        &mut primary,
        1,
        last,
        2,
        last_usable,
        disk_guid,
        array_crc,
    );
    let mut backup = [0u8; SECTOR_SIZE];
    fill_header(
        &mut backup,
        last,
        1,
        backup_array,
        last_usable,
        disk_guid,
        array_crc,
    );

    write_protective_mbr(disk, sectors)?;
    blk::write_blocks_at(disk, 1, &primary)?;
    write_array(disk, 2, &entry)?;
    write_array(disk, backup_array, &entry)?;
    blk::write_blocks_at(disk, last, &backup)?;
    Ok(())
}

fn write_protective_mbr(disk: usize, sectors: u64) -> Result<(), ()> {
    let mut mbr = [0u8; SECTOR_SIZE];
    mbr[446] = 0x00;
    mbr[447] = 0x00;
    mbr[448] = 0x02;
    mbr[449] = 0x00;
    mbr[450] = 0xEE;
    mbr[451] = 0xFF;
    mbr[452] = 0xFF;
    mbr[453] = 0xFF;
    mbr[454..458].copy_from_slice(&1u32.to_le_bytes());
    let size = sectors.saturating_sub(1).min(u64::from(u32::MAX)) as u32;
    mbr[458..462].copy_from_slice(&size.to_le_bytes());
    mbr[510] = 0x55;
    mbr[511] = 0xAA;
    blk::write_blocks_at(disk, 0, &mbr)
}

fn fill_header(
    buf: &mut [u8; SECTOR_SIZE],
    my_lba: u64,
    alt_lba: u64,
    part_lba: u64,
    last_usable: u64,
    disk_guid: [u8; 16],
    array_crc: u32,
) {
    buf.fill(0);
    buf[0..8].copy_from_slice(b"EFI PART");
    buf[8..12].copy_from_slice(&0x0001_0000u32.to_le_bytes());
    buf[12..16].copy_from_slice(&HEADER_SIZE.to_le_bytes());
    buf[24..32].copy_from_slice(&my_lba.to_le_bytes());
    buf[32..40].copy_from_slice(&alt_lba.to_le_bytes());
    buf[40..48].copy_from_slice(&34u64.to_le_bytes());
    buf[48..56].copy_from_slice(&last_usable.to_le_bytes());
    buf[56..72].copy_from_slice(&disk_guid);
    buf[72..80].copy_from_slice(&part_lba.to_le_bytes());
    buf[80..84].copy_from_slice(&ENTRIES.to_le_bytes());
    buf[84..88].copy_from_slice(&ENTRY_SIZE.to_le_bytes());
    buf[88..92].copy_from_slice(&array_crc.to_le_bytes());
    let crc = efi_crc32(&buf[..HEADER_SIZE as usize]);
    buf[16..20].copy_from_slice(&crc.to_le_bytes());
}

fn partition_entry(unique: [u8; 16], first: u64, last: u64) -> [u8; 128] {
    let mut e = [0u8; 128];
    e[0..16].copy_from_slice(&GUID_ESP);
    e[16..32].copy_from_slice(&unique);
    e[32..40].copy_from_slice(&first.to_le_bytes());
    e[40..48].copy_from_slice(&last.to_le_bytes());
    let name: [u16; 6] = [
        u16::from(b'C'),
        u16::from(b'O'),
        u16::from(b'E'),
        u16::from(b'L'),
        u16::from(b'E'),
        u16::from(b'O'),
    ];
    for (i, c) in name.iter().enumerate() {
        let o = 56 + i * 2;
        e[o..o + 2].copy_from_slice(&c.to_le_bytes());
    }
    e
}

fn write_array(disk: usize, start: u64, entry: &[u8; 128]) -> Result<(), ()> {
    for i in 0..ARRAY_SECTORS {
        let mut sec = [0u8; SECTOR_SIZE];
        if i == 0 {
            sec[..128].copy_from_slice(entry);
        }
        blk::write_blocks_at(disk, start + i, &sec)?;
    }
    Ok(())
}

fn array_crc32(entry: &[u8; 128]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    let mut sec = [0u8; SECTOR_SIZE];
    sec[..128].copy_from_slice(entry);
    crc = crc32_update(crc, &sec);
    let zero = [0u8; SECTOR_SIZE];
    for _ in 1..ARRAY_SECTORS {
        crc = crc32_update(crc, &zero);
    }
    !crc
}

fn make_guid(disk: usize, tag: u64) -> [u8; 16] {
    let t = clock::ticks();
    let mix = tag
        .wrapping_add(disk as u64)
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add(t);
    let mut g = [0u8; 16];
    g[0..8].copy_from_slice(&t.to_le_bytes());
    g[8..16].copy_from_slice(&mix.to_le_bytes());
    g[6] = (g[6] & 0x0F) | 0x40;
    g[8] = (g[8] & 0x3F) | 0x80;
    if g.iter().all(|&b| b == 0) {
        g[0] = 1;
    }
    g
}

fn efi_crc32(data: &[u8]) -> u32 {
    !crc32_update(0xFFFF_FFFF, data)
}

fn crc32_update(mut crc: u32, data: &[u8]) -> u32 {
    for &b in data {
        crc ^= u32::from(b);
        for _ in 0..8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ 0xEDB8_8320
            } else {
                crc >> 1
            };
        }
    }
    crc
}
