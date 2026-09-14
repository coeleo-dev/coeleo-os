//! Per-process fds. The table lives on the current PCB.

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write;

use spin::Mutex;

use crate::fs::{self, DirEnt, FsError};

pub const OPEN_READ: u64 = 1;
pub const OPEN_WRITE: u64 = 2;
pub const OPEN_CREATE: u64 = 4;
pub const OPEN_TRUNC: u64 = 8;
pub const INFO_MEM: u64 = 0;
pub const INFO_DISK: u64 = 1;
pub const INFO_INSTALL: u64 = 2;

const FD_MAX: usize = 16;
const PATH_MAX: usize = 256;
const IO_MAX: usize = 4096;
const DIRENT_SIZE: usize = 64;
const ERR: u64 = u64::MAX;

enum OpenFd {
    Stdin,
    Stdout,
    Stderr,
    File {
        path: String,
        offset: u64,
        writable: bool,
    },
    Dir {
        index: usize,
        ents: Vec<DirEnt>,
    },
    Pipe {
        id: u8,
        write: bool,
    },
}

pub struct FdTable {
    slots: [Option<OpenFd>; FD_MAX],
}

impl FdTable {
    pub fn new_stdio() -> Self {
        let mut t = Self {
            slots: [const { None }; FD_MAX],
        };
        t.slots[0] = Some(OpenFd::Stdin);
        t.slots[1] = Some(OpenFd::Stdout);
        t.slots[2] = Some(OpenFd::Stderr);
        t
    }

    fn get_mut(&mut self, fd: u64) -> Option<&mut OpenFd> {
        let i = usize::try_from(fd).ok()?;
        self.slots.get_mut(i)?.as_mut()
    }

    fn take(&mut self, fd: u64) -> Option<OpenFd> {
        let i = usize::try_from(fd).ok()?;
        self.slots.get_mut(i)?.take()
    }

    fn alloc(&mut self, of: OpenFd) -> u64 {
        for (i, slot) in self.slots.iter_mut().enumerate() {
            if slot.is_none() {
                *slot = Some(of);
                return i as u64;
            }
        }
        ERR
    }

    fn replace(&mut self, fd: u64, of: OpenFd) {
        if let Some(old) = self.take(fd) {
            drop_open(old);
        }
        let Some(i) = usize::try_from(fd).ok() else {
            drop_open(of);
            return;
        };
        if i >= FD_MAX {
            drop_open(of);
            return;
        }
        self.slots[i] = Some(of);
    }

    fn clone_slot(&self, fd: u64) -> Option<OpenFd> {
        let i = usize::try_from(fd).ok()?;
        let of = self.slots.get(i)?.as_ref()?;
        match of {
            OpenFd::Stdin => Some(OpenFd::Stdin),
            OpenFd::Stdout => Some(OpenFd::Stdout),
            OpenFd::Stderr => Some(OpenFd::Stderr),
            OpenFd::File {
                path,
                offset,
                writable,
            } => Some(OpenFd::File {
                path: path.clone(),
                offset: *offset,
                writable: *writable,
            }),
            OpenFd::Dir { .. } => None,
            OpenFd::Pipe { id, write } => {
                crate::pipe::clone_end(*id, *write).ok()?;
                Some(OpenFd::Pipe {
                    id: *id,
                    write: *write,
                })
            }
        }
    }

    pub fn inherit_from(&self, stdin_fd: u64, stdout_fd: u64) -> Option<Self> {
        let mut t = Self::new_stdio();
        if stdin_fd != u64::MAX {
            let of = self.clone_slot(stdin_fd)?;
            t.replace(0, of);
        }
        if stdout_fd != u64::MAX {
            match self.clone_slot(stdout_fd) {
                Some(of) => t.replace(1, of),
                None => {
                    t.close_all();
                    return None;
                }
            }
        }
        Some(t)
    }

    pub fn close_all(&mut self) {
        for slot in &mut self.slots {
            if let Some(of) = slot.take() {
                drop_open(of);
            }
        }
    }
}

fn drop_open(of: OpenFd) {
    if let OpenFd::Pipe { id, write } = of {
        crate::pipe::drop_end(id, write);
    }
}

/// Shared with `sys_write`. A `[u8; IO_MAX]` on the 16 KiB syscall stack plus
/// fatfs (debug frames, nested `open_dir`) overwrites the saved `sysretq` RIP.
static IO_BUF: Mutex<[u8; IO_MAX]> = Mutex::new([0; IO_MAX]);

fn with_fds<T>(f: impl FnOnce(&mut FdTable) -> T) -> Option<T> {
    crate::sched::with_current_fds(f)
}

pub fn sys_open(path_ptr: u64, path_len: u64, flags: u64) -> u64 {
    let Some(path) = copy_path(path_ptr, path_len) else {
        return ERR;
    };
    let want_read = flags & OPEN_READ != 0;
    let want_write = flags & OPEN_WRITE != 0;
    let want_create = flags & OPEN_CREATE != 0;
    let want_trunc = flags & OPEN_TRUNC != 0;
    if !want_read && !want_write {
        return ERR;
    }
    match fs::list(&path) {
        Ok(ents) => {
            if want_write {
                return ERR;
            }
            alloc(OpenFd::Dir { index: 0, ents })
        }
        Err(FsError::NotDir) => {
            if want_trunc && want_write && fs::write_file(&path, &[]).is_err() {
                return ERR;
            }
            open_file(path, want_write)
        }
        Err(FsError::NotFound) if want_create && want_write => {
            if fs::touch(&path).is_err() {
                return ERR;
            }
            open_file(path, true)
        }
        Err(_) => ERR,
    }
}

fn open_file(path: String, writable: bool) -> u64 {
    alloc(OpenFd::File {
        path,
        offset: 0,
        writable,
    })
}

enum ReadKind {
    Stdin,
    File,
    Pipe(u8),
}

pub fn sys_read(fd: u64, buf: u64, len: u64) -> u64 {
    if len > IO_MAX as u64 {
        return ERR;
    }
    let n = len as usize;
    let kind = match with_fds(|t| match t.get_mut(fd) {
        Some(OpenFd::Stdin) => Some(ReadKind::Stdin),
        Some(OpenFd::File { .. }) => Some(ReadKind::File),
        Some(OpenFd::Pipe { id, write: false }) => Some(ReadKind::Pipe(*id)),
        _ => None,
    }) {
        Some(Some(k)) => k,
        _ => return ERR,
    };
    match kind {
        ReadKind::Stdin => read_stdin(buf, n),
        ReadKind::File => read_file(fd, buf, n),
        ReadKind::Pipe(id) => read_pipe(id, buf, n),
    }
}

fn read_file(fd: u64, buf: u64, n: usize) -> u64 {
    if n > 0 && !crate::vmm::user_slice_ok(buf, n as u64) {
        return ERR;
    }
    let (path, offset) = {
        match with_fds(|t| match t.get_mut(fd) {
            Some(OpenFd::File { path, offset, .. }) => {
                if n == 0 {
                    Ok(None)
                } else {
                    Ok(Some((path.clone(), *offset)))
                }
            }
            _ => Err(()),
        }) {
            Some(Ok(None)) => return 0,
            Some(Ok(Some(v))) => v,
            _ => return ERR,
        }
    };
    let mut io = IO_BUF.lock();
    match fs::read_at(&path, offset, &mut io[..n]) {
        Ok(got) => {
            if copy_to_user(buf, &io[..got]).is_err() {
                return ERR;
            }
            drop(io);
            let _ = with_fds(|t| {
                if let Some(OpenFd::File { offset: o, .. }) = t.get_mut(fd) {
                    *o = offset + got as u64;
                }
            });
            got as u64
        }
        Err(_) => ERR,
    }
}

fn read_pipe(id: u8, buf: u64, n: usize) -> u64 {
    if n == 0 {
        return 0;
    }
    if !crate::vmm::user_slice_ok(buf, n as u64) {
        return ERR;
    }
    loop {
        let mut io = IO_BUF.lock();
        match crate::pipe::read(id, &mut io[..n]) {
            crate::pipe::Read::Data(got) => {
                if copy_to_user(buf, &io[..got]).is_err() {
                    return ERR;
                }
                drop(io);
                crate::sched::wake_pipe();
                return got as u64;
            }
            crate::pipe::Read::Eof => return 0,
            crate::pipe::Read::WouldBlock => {
                drop(io);
                crate::sched::wake_pipe();
                crate::sched::block_pipe_read();
            }
        }
    }
}

fn read_stdin(buf: u64, len: usize) -> u64 {
    if len == 0 {
        return 0;
    }
    if !crate::vmm::user_slice_ok(buf, 1) {
        return ERR;
    }
    loop {
        if let Some(b) = crate::kbd::pop_byte() {
            if b == 4 {
                return 0;
            }
            let byte = [b];
            if copy_to_user(buf, &byte).is_err() {
                return ERR;
            }
            return 1;
        }
        crate::comp::poll();
        if crate::sched::has_runnable_other() {
            crate::sched::block_stdin();
            continue;
        }
        crate::clock::paint_if_second_elapsed();
        crate::interrupts::wait();
        x86_64::instructions::interrupts::disable();
    }
}

enum WriteKind {
    Console,
    File,
    Pipe(u8),
}

pub fn sys_write(fd: u64, buf: u64, len: u64) -> u64 {
    if len > IO_MAX as u64 {
        return ERR;
    }
    if !crate::vmm::user_slice_ok(buf, len) {
        return ERR;
    }
    let n = len as usize;
    let kind = match with_fds(|t| match t.get_mut(fd) {
        Some(OpenFd::Stdout | OpenFd::Stderr) => Some(WriteKind::Console),
        Some(OpenFd::File { writable: true, .. }) => Some(WriteKind::File),
        Some(OpenFd::Pipe { id, write: true }) => Some(WriteKind::Pipe(*id)),
        _ => None,
    }) {
        Some(Some(k)) => k,
        _ => return ERR,
    };
    match kind {
        WriteKind::Console => write_console(buf, n),
        WriteKind::File => write_file(fd, buf, n),
        WriteKind::Pipe(id) => write_pipe(id, buf, n),
    }
}

fn write_console(buf: u64, n: usize) -> u64 {
    let mut io = IO_BUF.lock();
    if copy_from_user(buf, n, &mut io[..]).is_err() {
        return ERR;
    }
    match core::str::from_utf8(&io[..n]) {
        Ok(s) => {
            crate::console::write(s);
            n as u64
        }
        Err(_) => ERR,
    }
}

fn write_file(fd: u64, buf: u64, n: usize) -> u64 {
    let mut io = IO_BUF.lock();
    if copy_from_user(buf, n, &mut io[..]).is_err() {
        return ERR;
    }
    let (path, offset) = {
        match with_fds(|t| match t.get_mut(fd) {
            Some(OpenFd::File {
                path,
                offset,
                writable: true,
            }) => Some((path.clone(), *offset)),
            _ => None,
        }) {
            Some(Some(v)) => v,
            _ => return ERR,
        }
    };
    match fs::write_at(&path, offset, &io[..n]) {
        Ok(got) => {
            drop(io);
            let _ = with_fds(|t| {
                if let Some(OpenFd::File { offset: o, .. }) = t.get_mut(fd) {
                    *o = offset + got as u64;
                }
            });
            got as u64
        }
        Err(_) => ERR,
    }
}

fn write_pipe(id: u8, buf: u64, n: usize) -> u64 {
    if n == 0 {
        return 0;
    }
    loop {
        let mut io = IO_BUF.lock();
        if copy_from_user(buf, n, &mut io[..]).is_err() {
            return ERR;
        }
        match crate::pipe::write(id, &io[..n]) {
            crate::pipe::Write::Data(got) => {
                drop(io);
                crate::sched::wake_pipe();
                return got as u64;
            }
            crate::pipe::Write::Broken => return ERR,
            crate::pipe::Write::WouldBlock => {
                drop(io);
                crate::sched::wake_pipe();
                crate::sched::block_pipe_write();
            }
        }
    }
}

pub fn sys_close(fd: u64) -> u64 {
    if fd <= 2 {
        return ERR;
    }
    match with_fds(|t| t.take(fd)) {
        Some(Some(of)) => {
            drop_open(of);
            crate::sched::wake_pipe();
            0
        }
        _ => ERR,
    }
}

pub fn sys_pipe(buf: u64) -> u64 {
    if !crate::vmm::user_slice_ok(buf, 8) {
        return ERR;
    }
    let Some(id) = crate::pipe::alloc() else {
        return ERR;
    };
    let pair = with_fds(|t| {
        let r = t.alloc(OpenFd::Pipe { id, write: false });
        if r == ERR {
            return None;
        }
        let w = t.alloc(OpenFd::Pipe { id, write: true });
        if w == ERR {
            let _ = t.take(r);
            return None;
        }
        Some((r as u32, w as u32))
    });
    match pair {
        Some(Some((r, w))) => {
            let mut raw = [0u8; 8];
            raw[..4].copy_from_slice(&r.to_le_bytes());
            raw[4..].copy_from_slice(&w.to_le_bytes());
            if copy_to_user(buf, &raw).is_err() {
                let _ = with_fds(|t| {
                    if let Some(of) = t.take(r as u64) {
                        drop_open(of);
                    }
                    if let Some(of) = t.take(w as u64) {
                        drop_open(of);
                    }
                });
                return ERR;
            }
            0
        }
        _ => {
            crate::pipe::drop_end(id, false);
            crate::pipe::drop_end(id, true);
            ERR
        }
    }
}

pub fn sys_readdir(fd: u64, buf: u64, len: u64) -> u64 {
    if len < DIRENT_SIZE as u64 {
        return ERR;
    }
    if !crate::vmm::user_slice_ok(buf, DIRENT_SIZE as u64) {
        return ERR;
    }
    let packed = match with_fds(|t| -> Result<Option<[u8; DIRENT_SIZE]>, ()> {
        let slot = t.get_mut(fd).ok_or(())?;
        let OpenFd::Dir { index, ents } = slot else {
            return Err(());
        };
        if *index >= ents.len() {
            return Ok(None);
        }
        let ent = &ents[*index];
        *index += 1;
        let mut packed = [0u8; DIRENT_SIZE];
        packed[0] = u8::from(ent.is_dir);
        let name = ent.name.as_bytes();
        let n = name.len().min(DIRENT_SIZE - 2);
        packed[1..1 + n].copy_from_slice(&name[..n]);
        Ok(Some(packed))
    }) {
        Some(Ok(None)) => return 0,
        Some(Ok(Some(p))) => p,
        _ => return ERR,
    };
    if copy_to_user(buf, &packed).is_err() {
        return ERR;
    }
    1
}

pub fn sys_unlink(path_ptr: u64, path_len: u64) -> u64 {
    let Some(path) = copy_path(path_ptr, path_len) else {
        return ERR;
    };
    match fs::remove(&path) {
        Ok(()) => 0,
        Err(_) => ERR,
    }
}

pub fn sys_mkdir(path_ptr: u64, path_len: u64) -> u64 {
    let Some(path) = copy_path(path_ptr, path_len) else {
        return ERR;
    };
    match fs::mkdir(&path) {
        Ok(()) => 0,
        Err(_) => ERR,
    }
}

pub fn sys_sync() -> u64 {
    match fs::sync() {
        Ok(()) => 0,
        Err(_) => ERR,
    }
}

pub fn sys_sysinfo(kind: u64, buf: u64, len: u64) -> u64 {
    if len == 0 || !crate::vmm::user_slice_ok(buf, len) {
        return ERR;
    }
    let mut out = String::new();
    match kind {
        INFO_MEM => {
            let s = crate::pmm::stats();
            let _ = write!(
                out,
                "mem: {} / {} frames free ({} / {} KiB)\n",
                s.free,
                s.total,
                s.free * 4,
                s.total * 4
            );
        }
        INFO_DISK => crate::blk::format_table(&mut out),
        INFO_INSTALL => {
            let n = (len as usize).min(32);
            let mut tmp = [0u8; 32];
            if copy_from_user(buf, n, &mut tmp).is_err() {
                return ERR;
            }
            let arg = core::str::from_utf8(&tmp[..n])
                .unwrap_or("")
                .split('\0')
                .next()
                .unwrap_or("")
                .trim();
            out.push_str(crate::install::handle_cmd(arg));
        }
        _ => return ERR,
    }
    let n = out.len().min(len as usize);
    if copy_to_user(buf, &out.as_bytes()[..n]).is_err() {
        return ERR;
    }
    n as u64
}

fn alloc(of: OpenFd) -> u64 {
    match with_fds(|t| t.alloc(of)) {
        Some(fd) => fd,
        None => ERR,
    }
}

pub(crate) fn copy_path(ptr: u64, len: u64) -> Option<String> {
    if len == 0 || len > PATH_MAX as u64 {
        return None;
    }
    if !crate::vmm::user_slice_ok(ptr, len) {
        return None;
    }
    let n = len as usize;
    let mut tmp = [0u8; PATH_MAX];
    copy_from_user(ptr, n, &mut tmp).ok()?;
    core::str::from_utf8(&tmp[..n]).ok().map(String::from)
}

pub(crate) fn copy_from_user(ptr: u64, len: usize, dst: &mut [u8]) -> Result<(), ()> {
    if len > dst.len() {
        return Err(());
    }
    unsafe {
        core::ptr::copy_nonoverlapping(ptr as *const u8, dst.as_mut_ptr(), len);
    }
    Ok(())
}

pub(crate) fn copy_to_user(ptr: u64, src: &[u8]) -> Result<(), ()> {
    unsafe {
        core::ptr::copy_nonoverlapping(src.as_ptr(), ptr as *mut u8, src.len());
    }
    Ok(())
}
