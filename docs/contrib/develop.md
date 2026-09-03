# Development guide

Environment setup, how to build, and where to put new code. Product map: [`README.md`](../../README.md). Code practices: [`AGENTS.md`](../../AGENTS.md).

## Environment

Development machine: Linux (Debian/Ubuntu documented). Other Unix: `gmake` instead of `make`.

### Packages (Debian/Ubuntu)

```sh
sudo apt install make build-essential xorriso qemu-system-x86 python3 mtools gdisk
```

- `gcc` or `clang` — the kernel `build.rs` compiles Flanterm C (`cc`; `$CC` if set).
- `xorriso` — ISO.
- `qemu-system-x86` — `make run` and tests.
- `python3` — `scripts/test-phase*.py` and `test-plasma-p*.py`.
- `mtools` — `mformat`, `mcopy`, `mmd`, `mdir`, `mtype` on `disk.img` / `disk-ahci.img`.
- `sgdisk` (gdisk) — GPT on `disk-ahci.img` and `make all-hdd`.

Rust: [rustup](https://rustup.rs/). [`kernel/rust-toolchain.toml`](../../kernel/rust-toolchain.toml) asks for **nightly**, components `rust-src`, `clippy`, `rustfmt`, and the `x86_64-unknown-none` target. On the first build rustup installs this.

Make sure `cargo` is on `PATH` (`$HOME/.cargo/bin`).

## Build

At the repository root:

| Command | Effect |
| --- | --- |
| `make all` | kernel + userspace + `coeleo.iso` |
| `make -C kernel` | kernel only (**release** profile, `opt-level` from `Cargo.toml`) |
| `make userspace` | ELFs + `hello.coe` / `bad.coe` |
| `make run` | ISO + QEMU UEFI + virtio-blk + virtio-net + USB mouse |
| `make run-ahci` | ISO + QEMU UEFI + SATA (`disk-ahci.img`), no virtio-blk |

The kernel makefile sets:

```
RUSTFLAGS="-C relocation-model=static -C no-redzone=yes"
```

Use `make -C kernel`. A bare `cargo build` in `kernel/` without these flags is not the supported path (red zone / reloc).

Userspace: `make -C userspace` (or `make userspace` at the root). [`userspace/.cargo/config.toml`](../../userspace/.cargo/config.toml) enables `build-std` (`core`, `compiler_builtins`) and `--cfg curve25519_dalek_backend="serial"` (dalek’s SIMD backend breaks LLVM on `x86_64-unknown-none`).

**Do not** compile the packer with the userspace manifest:

```sh
# wrong — build-std duplicates core
cargo build --manifest-path userspace/Cargo.toml -p coe-pack

# right
make -C tools/coe-pack
```

`make userspace` already calls `make -C tools/coe-pack` and produces `userspace/libs/pkg/hello.coe` and `bad.coe`.

## FAT disks

| Image | Use | Host mtools access |
| --- | --- | --- |
| `disk.img` | virtio-blk (`make run`) | `mtype -i disk.img ::persist.txt` |
| `disk-ahci.img` | SATA GPT, partition at 1 MiB | `mtype -i disk-ahci.img@@1M ::persist.txt` |

`make run` / `make run-ahci` call `ensure-fat32-disk` / `ensure-ahci-disk`: they update ELFs if the FAT is readable; if `mdir` or `mcopy` fails, they **recreate** the image.

Do not run `mcopy` on the host while QEMU is using the same file. Do not answer interactive mtools prompts (see [troubleshooting](troubleshooting.md)).

Extra files without mounting (virtio):

```sh
mcopy -i disk.img my.txt ::
mtype -i disk.img ::persist.txt
```

`write` in the ELF `sh` asks for lines until a line that is only `.`. `sync` before closing QEMU.

## New app

Only if the current work requires it (YAGNI). Steps:

1. `userspace/apps/<name>/` with `Cargo.toml` (`edition = "2024"`) and `src/main.rs`: `#![no_std]`, `#![no_main]`, `_start`, `panic_handler`. Typical dependency: `libcoeleo = { path = "../../libs/libcoeleo" }`.
2. Add `"apps/<name>"` in [`userspace/Cargo.toml`](../../userspace/Cargo.toml).
3. `cp` + `strip` in [`userspace/GNUmakefile`](../../userspace/GNUmakefile).
4. `mcopy` of the binary in [`GNUmakefile`](../../GNUmakefile) (`$(DISK_IMG)`, `ensure-fat32-disk`, `$(AHCI_IMG)`, `ensure-ahci-disk`) and in any `test-*` targets that need the ELF.

New shared lib: `userspace/libs/<name>/` **only** if there is already a real second consumer. `coeleo-image` is the exception: folder under `libs/`, **outside** the userspace workspace.

## New syscall

Only when the [spec](../requirements/coeleo-os-specification-v02.md) requires it. Two places in the same step:

- Number and `match` in [`kernel/src/task/syscall.rs`](../../kernel/src/task/syscall.rs); implementation in the owning module (`fd`, `win`, `sched`, `net`, …).
- `SYS_*` constant and wrapper in [`userspace/libs/libcoeleo`](libs/libcoeleo.md).

Do not invent an ABI “for the day you need it”.

## Tests

Phase and Plasma gates are `make test-phase*` / `make test-plasma-p*` in the root [`GNUmakefile`](../../GNUmakefile). After an organisation slice (folders, splits):

```
make userspace
make -C kernel
```

Before declaring a reorganisation done, the gate in [`AGENTS.md`](../../AGENTS.md) (phase 8–10, 13–14 and plasma-p2…p7).

Acceptance criterion: the test (serial, screen, QMP) — not “it compiled”.
