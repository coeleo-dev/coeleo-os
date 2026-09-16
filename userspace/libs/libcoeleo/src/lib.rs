#![no_std]

pub const SYS_EXIT: u64 = 1;
pub const SYS_WRITE: u64 = 2;
pub const SYS_OPEN: u64 = 3;
pub const SYS_READ: u64 = 4;
pub const SYS_CLOSE: u64 = 5;
pub const SYS_READDIR: u64 = 6;
pub const SYS_SPAWN: u64 = 7;
pub const SYS_WAIT: u64 = 8;
pub const SYS_KILL: u64 = 9;
pub const SYS_CLOCK_MS: u64 = 10;
pub const SYS_PS: u64 = 11;
pub const SYS_UNLINK: u64 = 12;
pub const SYS_SYNC: u64 = 13;
pub const SYS_SYSINFO: u64 = 14;
pub const SYS_NET_PING: u64 = 15;
pub const SYS_HTTP_GET: u64 = 16;
pub const SYS_WIN_CREATE: u64 = 17;
pub const SYS_WIN_DAMAGE: u64 = 18;
pub const SYS_POLL_INPUT: u64 = 19;
pub const SYS_DATE: u64 = 20;
pub const SYS_REBOOT: u64 = 21;
pub const SYS_POWEROFF: u64 = 22;
pub const SYS_DISKS: u64 = 23;
pub const SYS_INSTALL: u64 = 24;
pub const SYS_PIPE: u64 = 25;
pub const SYS_MKDIR: u64 = 26;
pub const SYS_CLIPBOARD: u64 = 27;
pub const SYS_THREAD_CREATE: u64 = 28;

pub const OPEN_READ: u64 = 1;
pub const OPEN_WRITE: u64 = 2;
pub const OPEN_CREATE: u64 = 4;
pub const OPEN_TRUNC: u64 = 8;
pub const INFO_MEM: u64 = 0;
pub const INFO_DISK: u64 = 1;
pub const INFO_INSTALL: u64 = 2;
pub const DISK_KIND_VIRTIO: u32 = 0;
pub const DISK_KIND_AHCI: u32 = 1;
pub const DISK_KIND_USB: u32 = 2;
pub const DISK_FLAG_LIVE: u32 = 1;
pub const DISK_FLAG_SMALL: u32 = 2;
pub const DIRENT_SIZE: usize = 64;
pub const ERR: u64 = u64::MAX;
pub const ERR_NO_NET: u64 = u64::MAX - 1;
pub const ERR_TIMEOUT: u64 = u64::MAX - 2;
pub const ERR_HTTPS: u64 = u64::MAX - 3;
pub const ERR_BAD_URL: u64 = u64::MAX - 4;
pub const SPAWN_FD_DEFAULT: u64 = u64::MAX;

#[repr(C)]
struct HttpGetArgs {
    url_ptr: u64,
    url_len: u64,
    buf_ptr: u64,
    buf_len: u64,
}

#[repr(C)]
struct SpawnArgs {
    path_ptr: u64,
    path_len: u64,
    argv_ptr: u64,
    argv_len: u64,
    stdin_fd: u64,
    stdout_fd: u64,
}

#[repr(C)]
struct PingArgs {
    name_ptr: u64,
    name_len: u64,
    buf_ptr: u64,
    buf_len: u64,
}

#[repr(C)]
struct ThreadArgs {
    entry: u64,
    arg: u64,
    stack: u64,
    tls: u64,
}

pub fn write(fd: u64, buf: &[u8]) -> u64 {
    syscall3(SYS_WRITE, fd, buf.as_ptr() as u64, buf.len() as u64)
}

pub fn open(path: &str, flags: u64) -> u64 {
    syscall3(SYS_OPEN, path.as_ptr() as u64, path.len() as u64, flags)
}

pub fn read(fd: u64, buf: &mut [u8]) -> u64 {
    syscall3(SYS_READ, fd, buf.as_mut_ptr() as u64, buf.len() as u64)
}

pub fn close(fd: u64) -> u64 {
    syscall1(SYS_CLOSE, fd)
}

pub fn readdir(fd: u64, buf: &mut [u8; DIRENT_SIZE]) -> u64 {
    syscall3(SYS_READDIR, fd, buf.as_mut_ptr() as u64, DIRENT_SIZE as u64)
}

pub fn spawn(path: &str) -> u64 {
    spawn_ex(path, &[], SPAWN_FD_DEFAULT, SPAWN_FD_DEFAULT)
}

pub fn spawn_ex(path: &str, argv: &[u8], stdin_fd: u64, stdout_fd: u64) -> u64 {
    let args = SpawnArgs {
        path_ptr: path.as_ptr() as u64,
        path_len: path.len() as u64,
        argv_ptr: argv.as_ptr() as u64,
        argv_len: argv.len() as u64,
        stdin_fd,
        stdout_fd,
    };
    syscall1(SYS_SPAWN, &args as *const SpawnArgs as u64)
}

pub fn pipe(fds: &mut [u32; 2]) -> u64 {
    syscall1(SYS_PIPE, fds.as_mut_ptr() as u64)
}

pub fn wait() -> u64 {
    syscall0(SYS_WAIT)
}

pub fn kill(pid: u64) -> u64 {
    syscall1(SYS_KILL, pid)
}

pub fn clock_ms() -> u64 {
    syscall0(SYS_CLOCK_MS)
}

pub fn ps(buf: &mut [u8]) -> u64 {
    syscall2(SYS_PS, buf.as_mut_ptr() as u64, buf.len() as u64)
}

pub fn unlink(path: &str) -> u64 {
    syscall2(SYS_UNLINK, path.as_ptr() as u64, path.len() as u64)
}

pub fn mkdir(path: &str) -> u64 {
    syscall2(SYS_MKDIR, path.as_ptr() as u64, path.len() as u64)
}

pub fn sync() -> u64 {
    syscall0(SYS_SYNC)
}

pub fn sysinfo(kind: u64, buf: &mut [u8]) -> u64 {
    syscall3(SYS_SYSINFO, kind, buf.as_mut_ptr() as u64, buf.len() as u64)
}

pub fn date(buf: &mut [u8]) -> u64 {
    syscall2(SYS_DATE, buf.as_mut_ptr() as u64, buf.len() as u64)
}

pub fn reboot() -> u64 {
    syscall0(SYS_REBOOT)
}

pub fn poweroff() -> u64 {
    syscall0(SYS_POWEROFF)
}

pub fn disks(buf: &mut [u8]) -> u64 {
    syscall2(SYS_DISKS, buf.as_mut_ptr() as u64, buf.len() as u64)
}

pub fn install(index: u64) -> u64 {
    syscall1(SYS_INSTALL, index)
}

pub fn net_ping(name: &str, buf: &mut [u8]) -> u64 {
    let args = PingArgs {
        name_ptr: name.as_ptr() as u64,
        name_len: name.len() as u64,
        buf_ptr: buf.as_mut_ptr() as u64,
        buf_len: buf.len() as u64,
    };
    syscall1(SYS_NET_PING, &args as *const PingArgs as u64)
}

pub fn http_get(url: &str, buf: &mut [u8]) -> u64 {
    let args = HttpGetArgs {
        url_ptr: url.as_ptr() as u64,
        url_len: url.len() as u64,
        buf_ptr: buf.as_mut_ptr() as u64,
        buf_len: buf.len() as u64,
    };
    syscall1(SYS_HTTP_GET, &args as *const HttpGetArgs as u64)
}

/// Create a thread in this process: `entry(arg)` on a fresh user stack (`stack`
/// is the top), with `tls` as its `FsBase`. Returns the tid or `ERR`.
pub fn thread_create(entry: u64, arg: u64, stack: u64, tls: u64) -> u64 {
    let args = ThreadArgs {
        entry,
        arg,
        stack,
        tls,
    };
    syscall1(SYS_THREAD_CREATE, &args as *const ThreadArgs as u64)
}

pub fn win_create(w: u32, h: u32, pixels: &[u32]) -> u64 {
    syscall3(SYS_WIN_CREATE, w as u64, h as u64, pixels.as_ptr() as u64)
}

pub fn win_damage(id: u64, x: u32, y: u32, w: u32, h: u32) -> u64 {
    let xy = u64::from(x) | (u64::from(y) << 32);
    let wh = u64::from(w) | (u64::from(h) << 32);
    syscall3(SYS_WIN_DAMAGE, id, xy, wh)
}

pub fn poll_input(buf: &mut [u8]) -> u64 {
    syscall2(SYS_POLL_INPUT, buf.as_mut_ptr() as u64, buf.len() as u64)
}

pub fn dirent_is_dir(buf: &[u8; DIRENT_SIZE]) -> bool {
    buf[0] != 0
}

pub fn dirent_name(buf: &[u8; DIRENT_SIZE]) -> &str {
    let name = &buf[1..];
    let n = name.iter().position(|&b| b == 0).unwrap_or(name.len());
    core::str::from_utf8(&name[..n]).unwrap_or("")
}

pub fn exit(code: u64) -> ! {
    unsafe {
        core::arch::asm!(
            "syscall",
            in("rax") SYS_EXIT,
            in("rdi") code,
            options(noreturn),
        );
    }
}

pub fn clipboard_get(buf: &mut [u8]) -> Option<usize> {
    let r = unsafe {
        syscall3(
            SYS_CLIPBOARD,
            0,
            buf.as_mut_ptr() as u64,
            buf.len() as u64,
        )
    };
    if r == ERR {
        None
    } else {
        Some(r as usize)
    }
}

pub fn clipboard_set(data: &[u8]) {
    unsafe {
        syscall3(
            SYS_CLIPBOARD,
            1,
            data.as_ptr() as u64,
            data.len() as u64,
        );
    }
}

fn syscall3(num: u64, a0: u64, a1: u64, a2: u64) -> u64 {
    let mut ret: u64;
    unsafe {
        core::arch::asm!(
            "syscall",
            inlateout("rax") num => ret,
            in("rdi") a0,
            in("rsi") a1,
            in("rdx") a2,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack, preserves_flags),
        );
    }
    ret
}

fn syscall2(num: u64, a0: u64, a1: u64) -> u64 {
    let mut ret: u64;
    unsafe {
        core::arch::asm!(
            "syscall",
            inlateout("rax") num => ret,
            in("rdi") a0,
            in("rsi") a1,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack, preserves_flags),
        );
    }
    ret
}

fn syscall0(num: u64) -> u64 {
    let mut ret: u64;
    unsafe {
        core::arch::asm!(
            "syscall",
            inlateout("rax") num => ret,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack, preserves_flags),
        );
    }
    ret
}

fn syscall1(num: u64, a0: u64) -> u64 {
    let mut ret: u64;
    unsafe {
        core::arch::asm!(
            "syscall",
            inlateout("rax") num => ret,
            in("rdi") a0,
            lateout("rcx") _,
            lateout("r11") _,
            options(nostack, preserves_flags),
        );
    }
    ret
}
