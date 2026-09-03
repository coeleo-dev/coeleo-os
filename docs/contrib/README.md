# Contributing to Coeleo OS

Documentation for people who write code in this repository. The normative spec remains [`docs/requirements/coeleo-os-specification-v02.md`](../requirements/coeleo-os-specification-v02.md): what the system *does* and the phase scope. [`AGENTS.md`](../../AGENTS.md) is *how* to write: YAGNI, folders, facade, test gate. Here is the setup, the map, the lib contracts, and host-side failures.

This does not replace the spec or the Plasma plans. After phases 1–15: [product order R15](../roadmap-pos-fase-15.md). Index **16–49**: [implementation-plan-full.md](../requirements/implementation-plan-full.md). Phase 16 closed. Next: [17.1 disk table](../requirements/demands/PHASE-17/phase-17.1-disk-table.md) ([installer](../requirements/demands/PHASE-17/phase-17-installer.md)).

## Reading order

1. [Development guide](develop.md) — environment, commands, new app.
2. [Architecture](architecture.md) — kernel, userspace, disk.
3. [After 1–15](../roadmap-pos-fase-15.md) — three walls, R15 machine, what is not next (Wi-Fi, browser, sudo, SMP).
4. The sheet for the lib you are touching (`libs/`).
5. [Troubleshooting](troubleshooting.md) — when make or cargo fails on the host.

Package packer (host tool, not a userspace workspace crate): [tools-coe-pack.md](tools-coe-pack.md).

## Libs

| Crate | Folder | Userspace workspace | Consumers |
| --- | --- | --- | --- |
| [libcoeleo](libs/libcoeleo.md) | `userspace/libs/libcoeleo` | yes | all ELFs |
| [libcoeleoui](libs/libcoeleoui.md) | `userspace/libs/libcoeleoui` | yes | `widgets` |
| [coeleo-theme](libs/coeleo-theme.md) | `userspace/libs/coeleo-theme` | yes | kernel, draw, ui |
| [coeleo-draw](libs/coeleo-draw.md) | `userspace/libs/coeleo-draw` | yes | kernel, libcoeleoui |
| [coeleo-image](libs/coeleo-image.md) | `userspace/libs/coeleo-image` | **no** | kernel only |
| [pkg](libs/pkg.md) | `userspace/libs/pkg` | yes | `sh`; `coe-pack` on the host |

`coeleo-image` lives under `libs/` but is **not** a member of [`userspace/Cargo.toml`](../../userspace/Cargo.toml). Including it in userspace `cargo` with `build-std` duplicates `core` (zune). The kernel depends on it by path.
