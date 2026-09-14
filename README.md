# Coeleo OS

A **from-scratch** operating system in Rust, for one person sitting at an **x86_64** machine. It is not Linux, Windows, Redox, or an RTOS: kernel, ABI, and userspace are Coeleo. It reuses *components* (Limine, Flanterm, smoltcp, virtio, fatfs), not whole operating systems.

The target is a weak PC — one core, ~512 MiB, software framebuffer, no 3D GPU. The UI mixes a terminal and windows. A piece lands when the user *sees* it on screen, not when an invisible layer is “done”.

```
  Ring 3                         Ring 0
  --------                       ------
  sh, ls, cat, widgets,          syscall
  install, …                        ├── FAT32 (VFS, fd)
         |                          ├── smoltcp / virtio-net
         +---- libcoeleo ---------->├── static ELF, scheduler
                                    └── 2D compositor, disk, input
```

How to write code: [AGENTS.md](AGENTS.md). Map and lib sheets: [docs/contrib/](docs/contrib/).

## What it does

**Desktop.** Kernel compositor: opaque windows with deco, shadow, move/resize, min/max. Panel (floating or full-width), launcher, KRunner (Alt+Space), file manager, VT (Flanterm), JPEG wallpaper, Settings (`/desk.cfg`). PS/2 and USB HID mouse (UHCI); PS/2 keyboard.

**Processes.** Static Ring 3 ELFs, preemptive (LAPIC). `spawn` / `wait` / `kill`, `ps`. One thread per process. Disk `sh` is `init`; without it the kernel falls back to the in-kernel prompt.

**Disk.** FAT32 that survives reboot. virtio-blk (`make run`) and SATA/AHCI (`make run-ahci`, GPT). Seeds in `disk-seed/`.

**Network.** Static IPv4 `10.0.2.15/24` on QEMU user-net. `ping` and plaintext HTTP `GET`, no TLS.

**Packages.** `.coe` format (ELF + Ed25519). `pkg install` / `remove` in `sh`.

**Power.** ACPI reboot/shutdown and RTC (civil date).

Out of scope: Linux ABI, POSIX, SMP, Wi-Fi, 3D GPU, TLS, a dynamic linker, compositor in Ring 3.

## Architecture

One kernel crate. Folders are domain modules, not extra crates.

| Domain | Folder | Role |
| --- | --- | --- |
| Boot / CPU | `kernel/src/boot/` | GDT, IDT, LAPIC, serial, in-kernel shell, `init`, ACPI, RTC |
| Memory | `kernel/src/mem/` | PMM, VMM, heap |
| PCI | `kernel/src/bus/` | config space, xHCI |
| Disk / FAT | `kernel/src/fs/` | FAT, AHCI, block, partitions, USB MSC |
| Network | `kernel/src/net/` | smoltcp, HTTP, virtio-net |
| Input | `kernel/src/input/` | keyboard, mouse, PS/2, UHCI |
| Desktop | `kernel/src/ui/` | compositor (`ui/comp/`), panel, KRunner, Files, VT |
| Processes | `kernel/src/task/` | scheduler, syscalls, FDs, ELF |
| Entry | `kernel/src/main.rs` | Limine + `kmain` |

Limine loads the kernel (ISO or HDD). The compositor paints dirty rects; policy (focus, z-order) stays separate from paint. Context switch (`naked` asm) lives in `task/sched/switch.rs`.

Userspace: `no_std` workspace in [`userspace/Cargo.toml`](userspace/Cargo.toml), target `x86_64-unknown-none`. Apps in `userspace/apps/`. Host tools in `tools/` — not in the image.

Ring 3 GUI chain: `widgets` → `libcoeleoui` → `coeleo-draw` + `coeleo-theme` + `libcoeleo` (`win_create` / `win_damage`). The kernel paints chrome with the same tokens.

Detail: [docs/contrib/architecture.md](docs/contrib/architecture.md).

## Libraries

`no_std`, under `userspace/libs/`. Contracts in [docs/contrib/libs/](docs/contrib/libs/).

| Crate | Role | Userspace workspace | Consumers |
| --- | --- | --- | --- |
| [libcoeleo](docs/contrib/libs/libcoeleo.md) | Syscall wrappers (no `alloc`) | yes | every ELF |
| [libcoeleoui](docs/contrib/libs/libcoeleoui.md) | Immediate-mode widgets | yes | `widgets`, `install` |
| [coeleo-theme](docs/contrib/libs/coeleo-theme.md) | Palette and metrics (`PAD`, `ACCENT`, `DECO_H`, …) | yes | kernel, draw, ui |
| [coeleo-draw](docs/contrib/libs/coeleo-draw.md) | Primitives (round fill, proportional text, icons) | yes | kernel, libcoeleoui |
| [coeleo-image](docs/contrib/libs/coeleo-image.md) | JPEG/PNG (wallpaper, preview) | **no** | kernel only |
| [pkg](docs/contrib/libs/pkg.md) | Parse/verify `.coe` | yes | `sh`; `coe-pack` on the host |

`coeleo-image` is not a userspace workspace member: `build-std` + zune duplicate `core`. The kernel depends on it by path.

On the host, [`tools/coe-pack`](docs/contrib/tools-coe-pack.md) signs packages (`make -C tools/coe-pack`). Do not build the packer with the userspace manifest.

## Programs

Static ELFs in `userspace/apps/`. `_start` instead of `main`.

| App | Role |
| --- | --- |
| `sh` | Shell: fs, `mkdir`/`cp`/`mv`, `echo`/`pwd`, `ps`, `ping`, `get`, `pkg`, `install` |
| `ls`, `cat`, `echo` | List / read / print args |
| `hello`, `fault` | Spawn and fault accept |
| `clock`, `spin` | Background and CPU; Ctrl+C |
| `winprobe` | Ring 3 surface (`win_create`) |
| `widgets` | Toolkit (`libcoeleoui`) |
| `install` | Installer (GUI) |
| `edit` | TUI editor on the VT (`^S` save, `^Q` quit) |

## Run

Linux (Debian/Ubuntu). Rust nightly via [`kernel/rust-toolchain.toml`](kernel/rust-toolchain.toml).

```sh
sudo apt install make build-essential xorriso qemu-system-x86 python3 mtools gdisk
make all
make run
```

`make run` is QEMU UEFI + virtio-blk + virtio-net + USB mouse. `make run-ahci` uses SATA instead of virtio-blk. Environment, FAT disks, tests, and adding an app: [docs/contrib/develop.md](docs/contrib/develop.md).
