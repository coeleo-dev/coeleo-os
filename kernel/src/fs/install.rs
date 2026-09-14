//! Headless install: GPT + FAT32 ESP + copy from the live FAT. Confirm is a second `install n`.

use alloc::string::String;

use fatfs::{FatType, FileSystem, FormatVolumeOptions, FsOptions, Write};
use spin::Mutex;

use crate::blk::{self, SECTOR_SIZE};
use crate::fat_disk::FatDisk;
use crate::fs::{self, FsError};
use crate::gpt::{self, ESP_START_LBA};

const MIN_SECTORS: u64 = 64 * 1024 * 1024 / SECTOR_SIZE as u64;
const LABEL: [u8; 11] = *b"COELEO     ";

const REQUIRED: &[(&str, &str)] = &[
    ("/boot/kernel", "boot/kernel"),
    ("/boot/limine/limine.conf", "boot/limine/limine.conf"),
    (
        "/boot/limine/limine-bios.sys",
        "boot/limine/limine-bios.sys",
    ),
    ("/EFI/BOOT/BOOTX64.EFI", "EFI/BOOT/BOOTX64.EFI"),
    ("/README.TXT", "README.TXT"),
    ("/docs/HELLO.TXT", "docs/HELLO.TXT"),
    ("/sh", "sh"),
];

const OPTIONAL: &[(&str, &str)] = &[("/EFI/BOOT/BOOTIA32.EFI", "EFI/BOOT/BOOTIA32.EFI")];

static PENDING: Mutex<Option<usize>> = Mutex::new(None);

const ERR: u64 = u64::MAX;

enum Step {
    Refuse,
    RefuseLive,
    Armed,
    Ok,
    Failed,
}

pub fn handle_cmd(arg: &str) -> &'static str {
    let Some(n) = parse_index(arg) else {
        return "install: refuse\n";
    };
    match step(n) {
        Step::Refuse => "install: refuse\n",
        Step::RefuseLive => "install: refuse live-root\n",
        Step::Armed => "install: confirm\n",
        Step::Ok => "install: ok\n",
        Step::Failed => "install: failed\n",
    }
}

/// First valid call arms; the second with the same index runs [`apply`].
pub fn sys_install(n: u64) -> u64 {
    let Ok(n) = usize::try_from(n) else {
        crate::serial::write_str("install: refuse\n");
        return ERR;
    };
    match step(n) {
        Step::Refuse => {
            crate::serial::write_str("install: refuse\n");
            ERR
        }
        Step::RefuseLive => {
            crate::serial::write_str("install: refuse live-root\n");
            ERR
        }
        Step::Armed => 1,
        Step::Ok => {
            crate::serial::write_str("install: ok\n");
            0
        }
        Step::Failed => {
            crate::serial::write_str("install: failed\n");
            ERR
        }
    }
}

fn step(n: usize) -> Step {
    if blk::live_root() == Some(n) {
        return Step::RefuseLive;
    }
    if !target_ok(n) {
        return Step::Refuse;
    }
    let mut pending = PENDING.lock();
    if *pending == Some(n) {
        *pending = None;
        drop(pending);
        return match apply(n) {
            Ok(()) => Step::Ok,
            Err(()) => Step::Failed,
        };
    }
    *pending = Some(n);
    Step::Armed
}

pub fn apply(n: usize) -> Result<(), ()> {
    if blk::live_root() == Some(n) {
        return Err(());
    }
    if !target_ok(n) {
        return Err(());
    }
    let sectors = blk::capacity_sectors_at(n).ok_or(())?;
    let last_usable = gpt::last_usable(sectors).ok_or(())?;
    gpt::write_esp(n)?;
    let part_bytes = (last_usable - ESP_START_LBA + 1) * SECTOR_SIZE as u64;
    let mut vol = FatDisk::volume(n, ESP_START_LBA, part_bytes);
    // Force FAT32 (a 64 MiB live image would otherwise become FAT16). Do not
    // pin cluster size to 512: that is correct for small test volumes (fatfs
    // picks it anyway below 260 MiB) but on a multi-gigabyte stick it makes
    // two FAT tables of hundreds of MiB, written 512 B at a time with IRQs
    // off for the whole SYS_INSTALL — the GUI looks frozen.
    let opts = FormatVolumeOptions::new()
        .fat_type(FatType::Fat32)
        .bytes_per_sector(512)
        .volume_label(LABEL);
    fatfs::format_volume(&mut vol, opts).map_err(|_| ())?;
    let dst = FatDisk::volume(n, ESP_START_LBA, part_bytes);
    let fs = FileSystem::new(dst, FsOptions::new().update_accessed_date(false)).map_err(|_| ())?;
    if fs.fat_type() != FatType::Fat32 {
        return Err(());
    }
    for &(_, dest) in REQUIRED.iter().chain(OPTIONAL.iter()) {
        if let Some(parent) = parent_of(dest) {
            mkdir_p(&fs, parent)?;
        }
    }
    for &(src, dest) in REQUIRED {
        copy_file(&fs, src, dest)?;
    }
    for &(src, dest) in OPTIONAL {
        if fs::exists(src) {
            copy_file(&fs, src, dest)?;
        }
    }
    let _ = fs.unmount();
    blk::flush_at(n)
}

fn target_ok(n: usize) -> bool {
    if n >= blk::count() {
        return false;
    }
    let Some(sectors) = blk::capacity_sectors_at(n) else {
        return false;
    };
    if sectors < MIN_SECTORS {
        return false;
    }
    gpt::last_usable(sectors).is_some_and(|u| u >= ESP_START_LBA) && payload_ok()
}

fn payload_ok() -> bool {
    REQUIRED.iter().all(|&(src, _)| fs::exists(src))
}

fn copy_file(dst_fs: &FileSystem<FatDisk>, src: &str, dest: &str) -> Result<(), ()> {
    let mut file = dst_fs.root_dir().create_file(dest).map_err(|_| ())?;
    file.truncate().map_err(|_| ())?;
    fs::read_chunks(src, |bytes| {
        file.write_all(bytes).map_err(|_| FsError::Io)?;
        Ok(())
    })
    .map_err(|_| ())?;
    file.flush().map_err(|_| ())
}

fn mkdir_p(fs: &FileSystem<FatDisk>, path: &str) -> Result<(), ()> {
    let root = fs.root_dir();
    let mut acc = String::new();
    for part in path.split('/') {
        if part.is_empty() {
            continue;
        }
        if !acc.is_empty() {
            acc.push('/');
        }
        acc.push_str(part);
        root.create_dir(&acc).map_err(|_| ())?;
    }
    Ok(())
}

fn parent_of(path: &str) -> Option<&str> {
    path.rfind('/')
        .map(|i| &path[..i])
        .filter(|p| !p.is_empty())
}

fn parse_index(s: &str) -> Option<usize> {
    let tok = s.split_whitespace().next()?;
    if tok.is_empty() || !tok.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    tok.parse().ok()
}
