# libcoeleo

`no_std` syscall wrappers. No `alloc`. No Cargo dependencies.

**Folder:** `userspace/libs/libcoeleo`  
**Userspace workspace:** yes  
**Consumers:** all ELFs in `userspace/apps/` (`hello`, `sh`, `widgets`, …).

The ABI matches [`kernel/src/task/syscall.rs`](../../../kernel/src/task/syscall.rs). New numbers only when the spec requires them; the wrapper and the kernel table change in the same step. Behaviour detail (errors, flags): spec §5.

## Constants

Syscalls (`u64`):

| Name | Value |
| --- | --- |
| `SYS_EXIT` | 1 |
| `SYS_WRITE` | 2 |
| `SYS_OPEN` | 3 |
| `SYS_READ` | 4 |
| `SYS_CLOSE` | 5 |
| `SYS_READDIR` | 6 |
| `SYS_SPAWN` | 7 |
| `SYS_WAIT` | 8 |
| `SYS_KILL` | 9 |
| `SYS_CLOCK_MS` | 10 |
| `SYS_PS` | 11 |
| `SYS_UNLINK` | 12 |
| `SYS_SYNC` | 13 |
| `SYS_SYSINFO` | 14 |
| `SYS_NET_PING` | 15 |
| `SYS_HTTP_GET` | 16 |
| `SYS_WIN_CREATE` | 17 |
| `SYS_WIN_DAMAGE` | 18 |
| `SYS_POLL_INPUT` | 19 |
| `SYS_DATE` | 20 |
| `SYS_REBOOT` | 21 |
| `SYS_POWEROFF` | 22 |
| `SYS_DISKS` | 23 |
| `SYS_INSTALL` | 24 |

Flags / codes:

- `OPEN_READ` = 1, `OPEN_WRITE` = 2, `OPEN_CREATE` = 4, `OPEN_TRUNC` = 8 (combinable).
- `INFO_MEM` = 0, `INFO_DISK` = 1, `INFO_INSTALL` = 2 (`sysinfo`).
- `DISK_KIND_VIRTIO` = 0, `DISK_KIND_AHCI` = 1, `DISK_KIND_USB` = 2. `DISK_FLAG_LIVE` = 1, `DISK_FLAG_SMALL` = 2.
- `DIRENT_SIZE` = 64.
- `ERR` = `u64::MAX`; `ERR_NO_NET`, `ERR_TIMEOUT`, `ERR_HTTPS`, `ERR_BAD_URL` = `MAX-1` … `MAX-4`.

## Functions

All return `u64` (bytes, fd, pid, or `ERR*`), except `exit` (`!`) and the dirent helpers.

```text
write(fd, buf) -> u64
open(path, flags) -> u64
read(fd, buf) -> u64
close(fd) -> u64
readdir(fd, buf: &mut [u8; DIRENT_SIZE]) -> u64
spawn(path) -> u64
wait() -> u64
kill(pid) -> u64
clock_ms() -> u64
ps(buf) -> u64
unlink(path) -> u64
sync() -> u64
sysinfo(kind, buf) -> u64
date(buf) -> u64
reboot() -> u64
poweroff() -> u64
disks(buf) -> u64
install(index) -> u64
net_ping(octets: [u8; 4], buf) -> u64
http_get(url, buf) -> u64
win_create(w, h, pixels: &[u32]) -> u64
win_damage(id, x, y, w, h) -> u64
poll_input(buf) -> u64
dirent_is_dir(buf) -> bool
dirent_name(buf) -> &str
exit(code) -> !
```

`syscall` in `rax` / `rdi` / `rsi` / `rdx`. `http_get` passes a private `HttpGetArgs` struct in `rdi`.

`disks` copies into the caller buffer (use `[u8; 256]`): LE `u32` count, `u32` pad, then `count` rows of 24 bytes (`index`, `kind`, `flags`, reserved, `sectors` as `u64`). `kind`: 0 virtio, 1 AHCI. `flags` bit0 live-root, bit1 too small. Returns byte count or `ERR`. `install(n)`: first valid call returns `1` (armed); the second with the same `n` runs the kernel engine and returns `0` or `ERR`.

## Do not

- Do not call `syscall` by hand in the app if the wrapper already exists.
- Do not change numbers without the spec and the kernel.
- Do not add `std` or `alloc` to this crate.
