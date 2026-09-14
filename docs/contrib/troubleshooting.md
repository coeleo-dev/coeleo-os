# Troubleshooting

Symptom on the **host** (make, cargo, mtools). This is not the spec. If QEMU boots and the serial does not match acceptance, the phase test is the source of truth.

## Disks and mtools

| Symptom | Cause | What to do |
| --- | --- | --- |
| `Long file name "bin" already exists` and an `a)utorename …` prompt | `mmd ::bin` on a FAT that already has `/bin` (phase 14). mtools **waits for a key**; ignoring the command's exit status is not enough. | Do not answer. `Ctrl+C`. The makefile already uses `mmd -D s` (skip). Retry `make run` / `make run-ahci`. |
| `Bad FAT entry …` / `Fat error detected` | Broken FAT: mtools interrupted at a prompt, QEMU and `mcopy` on the same file, or `autorename` creating `bin-1`. | `ensure-ahci-disk` / `ensure-fat32-disk` recreate the image if `mdir`/`mcopy` fails. Or `rm disk-ahci.img disk.img` and the matching `make run*`. |
| `mtype` / `mcopy` “not found” on AHCI | The FAT is **not** at the start of the file. | 1 MiB offset: `mtype -i disk-ahci.img@@1M ::persist.txt`. virtio: `mtype -i disk.img ::persist.txt` (no `@@`). |
| A `write` file from `sh` is missing on the host | QEMU closed without `sync`. | In `sh`: `sync` before exit. |

Do not let two `make run-ahci` (or QEMU + `mcopy`) open the same `disk-ahci.img`.

## Cargo / Rust

| Symptom | Cause | What to do |
| --- | --- | --- |
| `error[E0152]: duplicate lang item … core` on `make userspace` | `coeleo-image` (zune) in the userspace workspace with `build-std` | Do not add `libs/coeleo-image` to [`userspace/Cargo.toml`](../../userspace/Cargo.toml). Only the kernel compiles it. |
| The same E0152 when building `coe-pack` “via userspace” | `build-std` + host `std` crate | `make -C tools/coe-pack`, never `--manifest-path userspace/Cargo.toml` for the packer. |
| Kernel that does not link, triple fault, or odd stack behaviour | `cargo build` in `kernel/` without red zone / static reloc | `make -C kernel` (`RUSTFLAGS` are in [`kernel/GNUmakefile`](../../kernel/GNUmakefile)). |
| `curve25519` / LLVM crash in userspace | dalek SIMD backend on `x86_64-unknown-none` | `--cfg curve25519_dalek_backend="serial"` is already in [`userspace/.cargo/config.toml`](../../userspace/.cargo/config.toml). Do not remove it. |
| `coeleo-draw` panic in `build.rs`: missing TTF | crate moved; path `../../../docs/fonts/Ubuntu-R.ttf` | Update the path in `build.rs` relative to `userspace/libs/coeleo-draw`. |
| rustup cannot find nightly / `x86_64-unknown-none` | toolchain not installed | Build once from `kernel/` so `rust-toolchain.toml` applies; `rustup target add x86_64-unknown-none` if needed. |

## QEMU

| Symptom | Cause | What to do |
| --- | --- | --- |
| No `sh` on the FAT, in-kernel prompt | `disk.img` without `::sh` | `make userspace` and `make run` (ensure copies the ELFs). Without userspace the in-kernel fallback is expected. |
| `ping 8.8.8.8` fails | QEMU user-net | Phase 11 acceptance is `ping 10.0.2.2`. ICMP to the Internet is out of scope. |
| Host serial seems to “glue” lines | QEMU `stdio` with `\r` | The Python tests normalize this. In the terminal it is `-serial stdio`. |

## Where to look

- Setup and new app: [develop.md](develop.md)
- Kernel/userspace map: [architecture.md](architecture.md)
- `.coe` packages: [libs/pkg.md](libs/pkg.md), [tools-coe-pack.md](tools-coe-pack.md)
- How to write code: [`AGENTS.md`](../../AGENTS.md)
