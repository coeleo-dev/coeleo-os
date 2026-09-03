# Architecture

Map of the current code. Behaviour and phases: [spec v0.2](../requirements/coeleo-os-specification-v02.md). Where to put new code and the `crate::comp` / `crate::sched` contract: [`AGENTS.md`](../../AGENTS.md). After 1–15 (reference class **R15**): [roadmap](../roadmap-pos-fase-15.md). Phase 16 closed. Index: [implementation-plan-full.md](../requirements/implementation-plan-full.md). Next: [17.1 disk table](../requirements/demands/PHASE-17/phase-17.1-disk-table.md) ([installer product](../requirements/demands/PHASE-17/phase-17-installer.md)).

## Boot

Limine loads the kernel (ISO or HDD). Limine requests in [`kernel/src/main.rs`](../../kernel/src/main.rs): framebuffer, HHDM, memory map, executable address, RSDP. `kmain` initializes serial, framebuffer (Flanterm), memory, ACPI (silent), interrupts, disk, network, compositor, and the `init` process (ELF `sh` if it exists on the FAT; otherwise the in-kernel shell).

```mermaid
flowchart TB
  limine[Limine]
  kmain[kmain]
  subgraph kernelDom [kernel/src]
    boot[boot]
    mem[mem]
    fsDom[fs]
    netDom[net]
    ui[ui/comp]
    task[task/sched]
  end
  us[userspace ELFs]
  libs[userspace/libs]
  limine --> kmain
  kmain --> boot
  kmain --> ui
  kmain --> task
  us --> libs
  task --> us
```

## Kernel: one crate, folders by domain

The kernel is **one** crate. Folders are modules, not new crates. `main.rs` reexports the names the rest of the crate already uses (`crate::comp`, `crate::sched`, `crate::fs`, `crate::panel`, …). Do not mass-rewrite `crate::foo` to `crate::ui::foo`.

| Domain | Folder | Role |
| --- | --- | --- |
| Boot / CPU | `kernel/src/boot/` | GDT, IDT, LAPIC, console, serial, kernel shell, `init`, ACPI, RTC |
| Memory | `kernel/src/mem/` | PMM, VMM, heap |
| PCI | `kernel/src/bus/` | PCI config space |
| Disk / FAT | `kernel/src/fs/` | FAT (`fs/mod.rs`), AHCI, block, partitions |
| Network | `kernel/src/net/` | smoltcp (`net/mod.rs`), HTTP, virtio-net |
| Input | `kernel/src/input/` | keyboard, mouse, PS/2, UHCI |
| Desktop | `kernel/src/ui/` | compositor (`ui/comp/`), panel, KRunner, FM, VT, windows |
| Processes | `kernel/src/task/` | scheduler (`task/sched/`), syscalls, FDs, ELF |

`kernel/vendor/` is not reorganized.

**Compositor** (`kernel/src/ui/comp/`): public API in `mod.rs` (`init`, `poll`, `irq_*`, `on_client_*`, …). Paint (shadow, deco, blit) is separate from policy (focus, z-order, launcher). Dirty-rect, not full-frame. The VT (Flanterm) is one window; the file manager is another.

**Scheduler** (`kernel/src/task/sched/`): `mod.rs` is the facade. `unsafe`, `naked_asm`, and the `KSTACKS` / `KCONTS` / `FXSAVES` statics stay in `switch.rs`. Do not scatter the context switch.

**Syscalls:** table in [`kernel/src/task/syscall.rs`](../../kernel/src/task/syscall.rs) (`SYS_EXIT` = 1 … `SYS_POWEROFF` = 22). ELFs call through [libcoeleo](libs/libcoeleo.md).

The kernel depends on crates in `userspace/libs/`: `coeleo-theme`, `coeleo-draw`, `coeleo-image` (path in [`kernel/Cargo.toml`](../../kernel/Cargo.toml)).

## Userspace

Workspace: [`userspace/Cargo.toml`](../../userspace/Cargo.toml). Target `x86_64-unknown-none`, `no_std`, `panic = "abort"`.

- **Libs** — `userspace/libs/`. Sheets in [`libs/`](libs/libcoeleo.md).
- **Apps** — `userspace/apps/`: `sh`, `ls`, `cat`, `hello`, `fault`, `clock`, `spin`, `winprobe`, `widgets`. Each is an ELF; `_start` instead of `main`.
- **Host** — `tools/` is not in the image. Packer: [tools-coe-pack.md](tools-coe-pack.md).

Ring 3 GUI chain: `widgets` → `libcoeleoui` → `coeleo-draw` + `coeleo-theme` + `libcoeleo` (`win_create` / `win_damage` / `poll_input`). The kernel compositor paints chrome (panel, shadow, deco) with the same theme/draw. The client paints an **opaque** buffer.

`sh` is split into `cwd` / `fs` / `net` / `pkg` / `path` under `userspace/apps/sh/src/`. It does not need its own sheet in this directory.

## Disk

Seeds in `disk-seed/` (`README.TXT`, `docs/HELLO.TXT`). The makefile creates:

- **`disk.img`** — 64 MiB, FAT32 on the whole image, label `COELEO`. virtio-blk.
- **`disk-ahci.img`** — 64 MiB, GPT, one type-0700 partition from LBA 2048 (1 MiB). FAT32 **on that** partition. SATA/AHCI.

Directories on the FAT: `docs/`, `bin/` (`pkg install` destination), `pacotes/` (test `.coe`). ELFs at the root (`sh`, `hello`, …) for phase 10 spawn (`hello` is still found as `/hello`).

QEMU user-net: static IP **10.0.2.15/24**, gateway **10.0.2.2**. `ping 10.0.2.2` is the phase 11 acceptance; ICMP to the Internet on user-net is not.
