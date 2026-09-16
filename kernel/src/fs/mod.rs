//! One FAT32 mount at `/` and a cwd. No process fds.

pub mod ahci;
pub mod blk;
pub mod fat_disk;
pub mod gpt;
pub mod install;
pub mod part;
pub mod usb_msc;

use alloc::string::String;
use alloc::vec::Vec;

use fatfs::{Error, FatType, FileAttributes, FileSystem, FsOptions, Read, Seek, SeekFrom, Write};
use spin::Mutex;

use crate::fat_disk::FatDisk;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FsError {
    NoFs,
    NotFound,
    NotDir,
    IsDir,
    NotText,
    Io,
}

#[derive(Clone)]
pub struct DirEnt {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
    pub mtime: Option<(u16, u8, u8, u8, u8, u8)>,
}

static FS: Mutex<Option<FileSystem<FatDisk>>> = Mutex::new(None);
static CWD: Mutex<String> = Mutex::new(String::new());
static VOLUME: Mutex<Option<(usize, u64, u64)>> = Mutex::new(None);

pub fn init() {
    *CWD.lock() = String::from("/");
    let mut fallback: Option<(usize, u64, u64)> = None;
    for i in 0..crate::blk::count() {
        if looks_like_fat32(i, 0) {
            if let Some(cap) = crate::blk::capacity_bytes_at(i) {
                if consider_mount(i, 0, cap, &mut fallback) {
                    return;
                }
            }
        }
        for vol in crate::part::volumes(i) {
            if !looks_like_fat32(i, vol.start_lba) {
                continue;
            }
            let size = vol.sectors.saturating_mul(crate::blk::SECTOR_SIZE as u64);
            if consider_mount(i, vol.start_lba, size, &mut fallback) {
                return;
            }
        }
    }
    if let Some((disk, start, size)) = fallback {
        let _ = try_mount(disk, start, size);
    }
}

fn consider_mount(
    disk: usize,
    start_lba: u64,
    size: u64,
    fallback: &mut Option<(usize, u64, u64)>,
) -> bool {
    if !try_mount(disk, start_lba, size) {
        return false;
    }
    if exists("/boot/kernel") {
        return true;
    }
    if fallback.is_none() {
        *fallback = Some((disk, start_lba, size));
    }
    drop_mount();
    false
}

fn drop_mount() {
    let mut g = FS.lock();
    if let Some(fs) = g.take() {
        let _ = fs.unmount();
    }
    drop(g);
    *VOLUME.lock() = None;
    crate::blk::clear_live_root();
}

fn looks_like_fat32(disk: usize, start_lba: u64) -> bool {
    let mut buf = [0u8; crate::blk::SECTOR_SIZE];
    if crate::blk::read_blocks_at(disk, start_lba, &mut buf).is_err() {
        return false;
    }
    buf[510] == 0x55 && buf[511] == 0xAA && buf.get(0x52..0x57) == Some(&b"FAT32"[..])
}

fn try_mount(disk: usize, start_lba: u64, size: u64) -> bool {
    let vol = FatDisk::volume(disk, start_lba, size);
    let Ok(fs) = FileSystem::new(vol, FsOptions::new().update_accessed_date(false)) else {
        return false;
    };
    if fs.fat_type() != FatType::Fat32 {
        return false;
    }
    *VOLUME.lock() = Some((disk, start_lba, size));
    crate::blk::set_live_root(disk);
    *FS.lock() = Some(fs);
    true
}

pub fn list(path: &str) -> Result<Vec<DirEnt>, FsError> {
    let abs = resolve(&cwd(), path);
    let rel = rel(&abs);
    let fs = FS.lock();
    let fs = fs.as_ref().ok_or(FsError::NoFs)?;
    let dir = if rel.is_empty() {
        fs.root_dir()
    } else {
        fs.root_dir().open_dir(rel).map_err(map_dir_err)?
    };
    let mut out = Vec::new();
    for ent in dir.iter() {
        let ent = ent.map_err(|_| FsError::Io)?;
        if ent.attributes().contains(FileAttributes::VOLUME_ID) {
            continue;
        }
        let name = ent.file_name();
        if name == "." || name == ".." {
            continue;
        }
        let mtime = {
            let dt = ent.modified();
            Some((
                dt.date.year,
                dt.date.month as u8,
                dt.date.day as u8,
                dt.time.hour as u8,
                dt.time.min as u8,
                dt.time.sec as u8,
            ))
        };
        out.push(DirEnt {
            is_dir: ent.is_dir(),
            name,
            size: if ent.is_dir() { 0 } else { ent.len() },
            mtime,
        });
    }
    Ok(out)
}

pub fn chdir(path: &str) -> Result<(), FsError> {
    let abs = resolve(&cwd(), path);
    let rel = rel(&abs);
    {
        let fs = FS.lock();
        let fs = fs.as_ref().ok_or(FsError::NoFs)?;
        if !rel.is_empty() {
            fs.root_dir().open_dir(rel).map_err(map_dir_err)?;
        }
    }
    *CWD.lock() = abs;
    Ok(())
}

pub fn read_chunks(
    path: &str,
    mut f: impl FnMut(&[u8]) -> Result<(), FsError>,
) -> Result<(), FsError> {
    let abs = resolve(&cwd(), path);
    let rel = rel(&abs);
    if rel.is_empty() {
        return Err(FsError::IsDir);
    }
    let fs = FS.lock();
    let fs = fs.as_ref().ok_or(FsError::NoFs)?;
    match fs.root_dir().open_dir(rel) {
        Ok(_) => return Err(FsError::IsDir),
        Err(Error::NotFound) => return Err(FsError::NotFound),
        Err(Error::InvalidInput) => {}
        Err(_) => return Err(FsError::Io),
    }
    let mut file = fs.root_dir().open_file(rel).map_err(map_file_err)?;
    let mut buf = [0u8; 4096];
    loop {
        let n = file.read(&mut buf).map_err(|_| FsError::Io)?;
        if n == 0 {
            break;
        }
        f(&buf[..n])?;
    }
    Ok(())
}

pub fn read_at(path: &str, offset: u64, buf: &mut [u8]) -> Result<usize, FsError> {
    let abs = resolve(&cwd(), path);
    let rel = rel(&abs);
    if rel.is_empty() {
        return Err(FsError::IsDir);
    }
    let fs = FS.lock();
    let fs = fs.as_ref().ok_or(FsError::NoFs)?;
    match fs.root_dir().open_dir(rel) {
        Ok(_) => return Err(FsError::IsDir),
        Err(Error::NotFound) => return Err(FsError::NotFound),
        Err(Error::InvalidInput) => {}
        Err(_) => return Err(FsError::Io),
    }
    let mut file = fs.root_dir().open_file(rel).map_err(map_file_err)?;
    file.seek(SeekFrom::Start(offset))
        .map_err(|_| FsError::Io)?;
    file.read(buf).map_err(|_| FsError::Io)
}

pub fn write_at(path: &str, offset: u64, buf: &[u8]) -> Result<usize, FsError> {
    let abs = resolve(&cwd(), path);
    let rel = rel(&abs);
    if rel.is_empty() {
        return Err(FsError::IsDir);
    }
    let fs = FS.lock();
    let fs = fs.as_ref().ok_or(FsError::NoFs)?;
    match fs.root_dir().open_dir(rel) {
        Ok(_) => return Err(FsError::IsDir),
        Err(Error::NotFound) => return Err(FsError::NotFound),
        Err(Error::InvalidInput) => {}
        Err(_) => return Err(FsError::Io),
    }
    let mut file = fs.root_dir().open_file(rel).map_err(map_file_err)?;
    file.seek(SeekFrom::Start(offset))
        .map_err(|_| FsError::Io)?;
    let n = file.write(buf).map_err(|_| FsError::Io)?;
    file.flush().map_err(|_| FsError::Io)?;
    Ok(n)
}

const READ_FILE_MAX: usize = 1536 * 1024;

pub fn read_file(path: &str) -> Result<Vec<u8>, FsError> {
    let mut out = Vec::new();
    read_chunks(path, |bytes| {
        if out.len().saturating_add(bytes.len()) > READ_FILE_MAX {
            return Err(FsError::Io);
        }
        out.extend_from_slice(bytes);
        Ok(())
    })?;
    Ok(out)
}

pub fn exists(path: &str) -> bool {
    match read_at(path, 0, &mut [0u8; 1]) {
        Ok(_) => true,
        Err(FsError::NotFound | FsError::IsDir | FsError::NoFs) => false,
        Err(_) => false,
    }
}

pub fn mkdir(path: &str) -> Result<(), FsError> {
    let rel = path_rel(path)?;
    let fs = FS.lock();
    let fs = fs.as_ref().ok_or(FsError::NoFs)?;
    match fs.root_dir().open_dir(&rel) {
        Ok(_) => return Err(FsError::IsDir),
        Err(Error::NotFound) | Err(Error::InvalidInput) => {}
        Err(_) => return Err(FsError::Io),
    }
    fs.root_dir().create_dir(&rel).map_err(map_dir_err)?;
    Ok(())
}

pub fn touch(path: &str) -> Result<(), FsError> {
    let rel = path_rel(path)?;
    let fs = FS.lock();
    let fs = fs.as_ref().ok_or(FsError::NoFs)?;
    reject_dir(fs, &rel)?;
    let _file = fs.root_dir().create_file(&rel).map_err(map_file_err)?;
    Ok(())
}

pub fn write_file(path: &str, data: &[u8]) -> Result<(), FsError> {
    let rel = path_rel(path)?;
    let fs = FS.lock();
    let fs = fs.as_ref().ok_or(FsError::NoFs)?;
    reject_dir(fs, &rel)?;
    let mut file = fs.root_dir().create_file(&rel).map_err(map_file_err)?;
    file.truncate().map_err(|_| FsError::Io)?;
    file.write_all(data).map_err(|_| FsError::Io)?;
    file.flush().map_err(|_| FsError::Io)?;
    Ok(())
}

pub fn remove(path: &str) -> Result<(), FsError> {
    let rel = path_rel(path)?;
    let fs = FS.lock();
    let fs = fs.as_ref().ok_or(FsError::NoFs)?;
    reject_dir(fs, &rel)?;
    fs.root_dir().remove(&rel).map_err(map_file_err)
}

pub fn sync() -> Result<(), FsError> {
    let (disk, start, size) = VOLUME.lock().ok_or(FsError::NoFs)?;
    let mut g = FS.lock();
    let fs = g.take().ok_or(FsError::NoFs)?;
    let _ = fs.unmount();
    drop(g);
    // flush may fail (e.g. QEMU block-0 restriction); always try to remount.
    let flush_ok = crate::blk::flush_at(disk).is_ok();
    if try_mount(disk, start, size) {
        if flush_ok { Ok(()) } else { Err(FsError::Io) }
    } else {
        Err(FsError::Io)
    }
}

pub fn volume_label() -> Option<String> {
    let fs = FS.lock();
    let fs = fs.as_ref()?;
    let s = fs.volume_label();
    let t = s.trim();
    if t.is_empty() {
        None
    } else {
        Some(String::from(t))
    }
}

fn path_rel(path: &str) -> Result<String, FsError> {
    let abs = resolve(&cwd(), path);
    let rel = rel(&abs);
    if rel.is_empty() {
        return Err(FsError::IsDir);
    }
    Ok(String::from(rel))
}

fn reject_dir(fs: &FileSystem<FatDisk>, rel: &str) -> Result<(), FsError> {
    match fs.root_dir().open_dir(rel) {
        Ok(_) => Err(FsError::IsDir),
        Err(Error::NotFound) => Ok(()),
        Err(Error::InvalidInput) => Ok(()),
        Err(_) => Err(FsError::Io),
    }
}

fn cwd() -> String {
    let c = CWD.lock();
    if c.is_empty() {
        String::from("/")
    } else {
        c.clone()
    }
}

fn rel(abs: &str) -> &str {
    abs.trim_start_matches('/')
}

fn resolve(cwd: &str, arg: &str) -> String {
    let mut parts: Vec<&str> = if arg.starts_with('/') || cwd == "/" {
        Vec::new()
    } else {
        cwd.trim_start_matches('/')
            .split('/')
            .filter(|s| !s.is_empty())
            .collect()
    };
    if !arg.is_empty() {
        for c in arg.split('/') {
            match c {
                "" | "." => {}
                ".." => {
                    let _ = parts.pop();
                }
                other => parts.push(other),
            }
        }
    }
    if parts.is_empty() {
        String::from("/")
    } else {
        let mut s = String::from("/");
        s.push_str(&parts.join("/"));
        s
    }
}

fn map_dir_err(e: Error<()>) -> FsError {
    match e {
        Error::NotFound => FsError::NotFound,
        Error::InvalidInput => FsError::NotDir,
        Error::AlreadyExists => FsError::IsDir,
        _ => FsError::Io,
    }
}

fn map_file_err(e: Error<()>) -> FsError {
    match e {
        Error::NotFound => FsError::NotFound,
        Error::InvalidInput => FsError::IsDir,
        _ => FsError::Io,
    }
}
