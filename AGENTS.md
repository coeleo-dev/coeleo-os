# AGENTS.md

## This repository

Coeleo OS. The normative spec is [`docs/requirements/coeleo-os-specification-v02.md`](docs/requirements/coeleo-os-specification-v02.md). Setup, architecture, and lib sheets: [`docs/contrib/`](docs/contrib/). After phases 1–15 (R15): [`docs/roadmap-pos-fase-15.md`](docs/roadmap-pos-fase-15.md). Phase 16 is closed. Full index 16–49: [`implementation-plan-full.md`](docs/requirements/implementation-plan-full.md). Next to implement: [`phase-17.1-disk-table.md`](docs/requirements/demands/PHASE-17/phase-17.1-disk-table.md) (product: [`phase-17-installer.md`](docs/requirements/demands/PHASE-17/phase-17-installer.md); pendrive = [17.4](docs/requirements/demands/PHASE-17/phase-17.4-usb-msc.md)).

Kernel code: `kernel/`. Userspace: `userspace/libs/` (shared crates) and `userspace/apps/` (ELFs).

Build and run: `make all`, `make run` (detail in [`docs/contrib/develop.md`](docs/contrib/develop.md)). Product map: [`README.md`](README.md).

This file does not replace the spec. System behaviour and phase scope live there. Here is *how* to write and work; there is *what* to build.

## Practices

**YAGNI.** Do not build an extension, layer, or setting “for the day you need it”. Only what the current work requires.

**Small unit.** One file, function, or type: one responsibility. Names describe the observable effect, not the implementation.

**Clear boundaries.** The consumer does not need internals. Changing internals must not break the contract.

**Visible errors.** Do not swallow failure. An unrecoverable failure must be obvious to whoever is at the machine or the log.

**Verify before calling it done.** The acceptance criterion of the work in progress (command, screen, test) — not “it compiled”.

**Do not abstract on the third imaginary use.** A second identical use may still be copied. Extract when the duplication actually hurts.

**Dangerous surface in the smallest place.** Raw memory, hardware, credentials: the smallest enclosure possible, documented on the spot.

**Comments.** Explain *why*, not what the code already says.

**Version control.** Secrets and generated artefacts stay out of the repository.

**Agent.** Read this file for *how* to write; the spec for *what* to build. Do not widen the task’s scope.

## Folder map

The kernel is **one** crate. Folders are domain modules, not new crates.

| Domain | Folder | What lives there |
| --- | --- | --- |
| Boot / CPU | `kernel/src/boot/` | GDT, IDT, LAPIC, console, serial, kernel shell, `init` |
| Memory | `kernel/src/mem/` | PMM, VMM, heap |
| PCI | `kernel/src/bus/` | PCI config space |
| Disk / FAT | `kernel/src/fs/` | FAT (`fs/mod.rs`), AHCI, block, partitions |
| Network | `kernel/src/net/` | smoltcp (`net/mod.rs`), HTTP, virtio-net |
| Input | `kernel/src/input/` | keyboard, mouse, PS/2, UHCI |
| Desktop | `kernel/src/ui/` | compositor (`ui/comp/`), panel, KRunner, FM, VT, windows |
| Processes | `kernel/src/task/` | scheduler (`task/sched/`), syscalls, FDs, ELF |
| Kernel entry | `kernel/src/main.rs` | Limine + `kmain` |

Userspace (workspace in `userspace/Cargo.toml`):

| Type | Folder |
| --- | --- |
| `no_std` libs | `userspace/libs/` — `libcoeleo`, `libcoeleoui`, `coeleo-theme`, `coeleo-draw`, `coeleo-image`, `pkg` |

`coeleo-image` lives under `libs/` but is **not** a userspace workspace member: only the kernel compiles it. Including it in userspace `cargo` with `build-std` duplicates `core` (zune).
| Programs | `userspace/apps/` — `sh`, `ls`, `cat`, `hello`, … |
| Host tools | `tools/` — not in the image |

`kernel/vendor/` is not reorganized.

## Where to put new code

- Desktop paint or policy → `kernel/src/ui/comp/` (do not go back to a monolith `comp.rs`).
- Panel widget / KRunner geometry → `ui/panel.rs` / `ui/krunner.rs` if it is paint only; spawn and z-order stay in the compositor.
- New syscall **only when the spec requires it** → table in `kernel/src/task/syscall.rs`; implementation in the owning module (fs, win, sched).
- New app → `userspace/apps/<name>/`. Shared lib → `userspace/libs/` only when there is a real second consumer.
- Packer / package keys → `tools/coe-pack/`.

## When to split a file

Split when the file exceeds ~500 lines **and** has two responsibilities. Do not split for aesthetics.

Rust pattern: `foo.rs` → `foo/mod.rs` + children. The public API stays in `mod.rs` (facade). Internals are `pub(super)` or private.

In the compositor and the scheduler, types and `unsafe` are not scattered “just because”: context switch and naked asm stay in `task/sched/switch.rs`.

## Stable contract

Names the rest of the crate already uses (`crate::comp`, `crate::sched`, `crate::fs`, `crate::panel`, …) stay. `main.rs` reexports:

```rust
pub(crate) use ui::{clock, comp, desk, fbterm, fm, font8x16, krunner, panel, win};
pub(crate) use task::{elfload, fd, process, sched, syscall};
```

A new folder **does not** require a mass rename of consumers. Do not rewrite `crate::comp` to `crate::ui::comp` just to “look canonical”.

Do not mix a `git mv` with a behaviour, ABI, or message change in the same step. If a test fails after a move, revert the move — do not “fix” it with new logic.

## Patterns in this repository

Do not import a GoF catalogue. These are what the code already uses:

**Facade + state.** Each subsystem exposes a few functions (`init`, `poll`, `sys_*`). The `Mutex<State>` (or equivalent) lives in the module; the consumer does not construct the state.

**Dirty-rect.** The compositor paints what changed. Paint (shadow, deco, blit) is separate from policy (focus, z-order, launcher). No full-frame “to keep it simple”.

**Shared `no_std` crates.** Theme and draw already exist (`userspace/libs/coeleo-theme`, `coeleo-draw`). The kernel depends on them. Do not create a `kernel-ui` crate for internals.

**Dangerous surface.** `unsafe`, naked asm, DMA, and credentials: the smallest file, with the *why* on the spot. Do not dilute the context switch across half a dozen modules.

**Duplication.** A second identical use may still be copied (two `Rect` types in the compositor and KRunner are fine). Extract when the duplication hurts.

## What not to do

- A trait object for the window manager, a “compositor plugin”, or a layer for a toolkit we are not porting.
- An extra crate in the kernel “for the day the compositor leaves this crate”.
- `mkdir` of Unix-style `arch/`, `drivers/`, `mm/` folders the code does not need.
- Unifying types only because the name matches.

## Gate

After each organisation slice: `make -C kernel` and/or `make userspace`.

Before declaring a reorganisation done:

```
make userspace
make -C kernel
make coeleo.iso
make test-phase8 test-phase9 test-phase10 test-phase13 test-phase14
make test-plasma-p2 test-plasma-p3 test-plasma-p4 test-plasma-p5 test-plasma-p6 test-plasma-p7
```
