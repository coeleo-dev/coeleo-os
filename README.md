# Coeleo OS

A **from-scratch** operating system written in Rust for x86_64 hardware. Coeleo OS is not Linux, Windows, Redox, or an RTOS: its kernel, syscall ABI, design system, and userspace are built from the ground up, selectively reusing focused components (Limine, Flanterm, smoltcp, fatfs) rather than whole operating systems.

It is targeted at resource-constrained PCs (single-core, ~512 MiB RAM, software linear framebuffer, no 3D GPU required) delivering an elegant, fast, and responsive user experience that unifies a modern desktop environment, a software-rendered window compositor, and an interactive terminal.

```
  Ring 3                         Ring 0
  --------                       ------
  sh, edit, widgets,             Syscall Layer (27 syscalls)
  install, apps, …                  ├── FAT32 Filesystem (VFS, FDs, partitions)
         |                          ├── Network Stack (smoltcp / virtio-net / e1000e)
         +---- libcoeleo ---------->├── Preemptive Scheduler (LAPIC, static ELF)
                                    └── 2D Compositor (Slate Elegance, dirty-rects, input)
```

- Code standards and contributing contracts: [AGENTS.md](AGENTS.md)
- Technical architecture and domain map: [docs/contrib/architecture.md](docs/contrib/architecture.md)
- Development, build, test, and debug guide: [docs/contrib/develop.md](docs/contrib/develop.md)
- Troubleshooting and common issues: [docs/contrib/troubleshooting.md](docs/contrib/troubleshooting.md)

---

## Implemented Features

### 1. Graphical Desktop & Compositor (*Slate Elegance*)
- **Software 2D Compositor**: Pure CPU linear framebuffer rendering with dirty-rect clipping and selective damage updates (`present_damage`), achieving high frame rates without hardware acceleration.
- **Slate Elegance Design System**: Sophisticated dark palette (`BG`, `SURFACE`, `SURFACE_RAISED`, `PANEL_BG`, `BORDER`, `ACCENT`) inspired by modern desktop environments.
- **Subpixel Hairline Borders**: 1px rounded outlines rendered via Signed Distance Fields (SDF) with anti-aliased corners (`stroke_round`) and multi-layer drop shadows (`shade_round_ring`).
- **Window Controls & Chrome**:
  - Application icon embedded directly into the title bar.
  - Automatic title truncation and elision.
  - Interactive micro-buttons: Minimize, Maximize/Restore, and Close (with semantic danger hover).
  - Active window focus detection with dynamic glowing blue accent border (`0x0038_8BFD`).
  - Fluid window dragging and 8-direction interactive edge resizing.
- **Floating / Full-Width Taskbar Panel**:
  - Dynamic panel supporting floating pill or full-screen strut layout.
  - Prominent active-window illumination, background process dot indicator (`4x4`), and minimized window dimming.
  - Live system clock widget and network activity indicator.
  - Desktop / settings quick-access popup menu.
- **KRunner Application Launcher (Alt+Space)**:
  - Floating modal overlay with fuzzy application search.
  - Categorized result badges, keyboard navigation (Up/Down/Enter/Esc), and direct process spawning.
- **Graphical File Manager (`fm`)**:
  - Embedded dual-pane explorer with interactive breadcrumb navigation pills.
  - "Places" and "Devices" navigation sidebar, file list with dedicated icons, and keyboard navigation.
  - File execution and path copy shortcut (`Ctrl+C`).
- **Wallpaper & Personalization**:
  - Full-screen desktop rendering with solid colors or decoded JPEG wallpapers (`coeleo-image`).
  - Persistent desktop settings loaded from `/desk.cfg`.
- **System Clipboard & Toasts**:
  - Kernel-level global clipboard accessible via syscalls (`SYS_CLIPBOARD`), linking terminal, shell, and GUI apps (`edit`, `clip`).
  - Floating rounded notification toasts for background tasks, copy events, and system alerts.

### 2. Interactive Shell (`sh`) & Terminal Emulator
- **Virtual Terminal (Flanterm)**: Window-embedded VT with a 16-color ANSI palette harmonized with the Slate theme, supporting cursor positioning, bold text, and clean blits.
- **Readline / ZLE Navigation & Line Editing**:
  - Navigation: `Home` / `Ctrl+A`, `End` / `Ctrl+E`, `Delete` (`\x1b[3~`) / `Ctrl+D`.
  - Word jumping: `Alt+B` / `Ctrl+Left`, `Alt+F` / `Ctrl+Right`.
  - Line rubout & kill: `Ctrl+K` (kill to end), `Ctrl+U` (kill to start), `Ctrl+W` (kill word backward).
  - Utility: `Ctrl+T` (transpose characters), `Ctrl+L` (clear screen and redraw), `Ctrl+V` (paste from system clipboard).
- **Dynamic Context-Aware Prompt**:
  - Displays formatted working directory (`<cwd>`) in bold blue.
  - Semantic exit status indicator (green on success, red on error), preserving the literal `coeleo>` prompt token.
- **Intelligent Autocompletion (Tab)**:
  - Directory completion with automatic trailing slash (`/`) enabling continuous path traversal.
  - Contextual completion for subdirectories, files, and shell built-in commands.
  - Longest Common Prefix (LCP) multi-match completion with colorized candidate columns.
- **Command History Engine**:
  - 128-entry session memory with Up/Down arrow recall.
  - Interactive incremental reverse search (`Ctrl+R`) with live query matching.
- **Command Chaining & Control Operators**:
  - Semicolon (`;`) sequential execution (`mkdir test; cd test; touch app.rs`).
  - Pipeline chaining (`|`) and file redirections (`>` and `<`).
  - Exit code inspection (`echo $?`).
- **Alias & Environment System**:
  - In-memory alias manager (`alias ll='ls -l'`, `alias ..='cd ..'`), `unalias`, and `which`.
  - Script batch execution with `source <file.sh>` or `. <file.sh>`.
- **User-Friendly Error Architecture**:
  - Semantically styled prefixes (`erro:`, `aviso:`, `dica:`, `uso:`).
  - Target-specific contextual descriptions.
  - Levenshtein-based intelligent suggestion engine ("Did you mean...?").

### 3. Built-in Commands & Utilities Catalog

| Category | Commands | Description |
| --- | --- | --- |
| **Files & Dirs** | `ls` (with `-l`), `cd`, `pwd`, `mkdir`, `cp`, `mv`, `rm`, `touch`, `write`, `stat`, `tree` | Complete directory and file management, recursive tree view, metadata inspection, and multiline text creation. |
| **Text Processing** | `cat`, `head` (with `-n`), `tail` (with `-n`), `wc` (`-l`, `-w`, `-c`), `grep` (`-i`, `-n`, `-v`), `echo` | File reading, head/tail inspection, word/line counts, text pattern search with ANSI match highlighting, and stdout printing. |
| **System & Procs** | `ps`, `kill`, `spin`, `fault`, `mem`, `disk`, `disks`/`lsblk`, `uptime`, `date`, `uname`/`version` (`-a`), `free`, `df`, `sleep`, `clock` | Process table listing and termination, memory statistics, storage hardware inspection, civil RTC date/time, human-readable heap/disk usage, and delay loops. |
| **Productivity** | `clip` (`get`/`set`), `alias`, `unalias`, `which`, `history` (`-c`), `source`/`.`, `clear`, `help` | Clipboard integration, command shortcuts, command location, session history, and script execution. |
| **Packages & Apps** | `pkg` (`install`/`remove`), `install`, `edit`, `run` | Cryptographically signed package manager, OS installer, VT text editor, and direct ELF execution. |
| **Network** | `ping`, `get` | ICMP connectivity testing to gateway/host, and plaintext HTTP GET queries. |
| **Power** | `sync`, `reboot`, `poweroff`/`halt` | Filesystem cache flush, ACPI restart, and ACPI shutdown. |

### 4. Kernel Core & Hardware Subsystems
- **Boot & CPU**: Limine bootloader support (UEFI and BIOS), GDT, IDT, LAPIC timer, interrupt routing, and naked-assembly context switching.
- **Memory Management**:
  - Physical Memory Manager (PMM) with frame bitmap allocation.
  - Virtual Memory Manager (VMM) with 4-level paging and strict user/kernel address space isolation.
  - Kernel heap allocator.
- **Storage & Block Devices**:
  - FAT32 filesystem implementation with cluster allocation, long filenames, directory traversal, read, write, truncate, and sync.
  - Disk backends: VirtIO-blk (QEMU standard), SATA AHCI with GPT partition tables, and USB Mass Storage (MSC).
- **Networking**:
  - Embedded `smoltcp` stack over VirtIO-net or Intel e1000e.
  - Static IPv4 / DHCP configuration, ARP, ICMP echo, DNS A resolution, and HTTP client.
- **Input Drivers**:
  - PS/2 keyboard with full scancode translation and key event dispatching.
  - PS/2 mouse and USB HID mouse over UHCI and xHCI controllers.
- **Process Scheduler**:
  - Preemptive round-robin scheduler driven by LAPIC timer interrupts.
  - Process Control Block (PCB) table, single-thread per process model, zombie reaping, and clean resource cleanup.
- **Syscall Layer**:
  - 27 system calls exposed through `libcoeleo` covering processes, files, pipes, network, memory, time, window surfaces, and clipboard.
- **Security & Package Verification**:
  - Ed25519 digital signatures and SHA-256 integrity verification for `.coe` software packages.

---

## Userspace Applications (ELFs)

Userspace binaries live in `userspace/apps/` and compile to static `no_std` ELFs (`x86_64-unknown-none`) using `_start`:

| Application | Role |
| --- | --- |
| `sh` | Interactive shell with Readline navigation, autocompletion, dynamic prompt, aliases, and builtins |
| `edit` | TUI text editor on the VT (`Ctrl+S` save, `Ctrl+Q` quit) |
| `widgets` | GUI demo showcasing the `libcoeleoui` immediate-mode toolkit |
| `install` | Graphical and CLI OS installer for GPT/AHCI disk deployment |
| `winprobe` | Direct Ring 3 surface creation and animation benchmark (`SYS_WIN_CREATE`) |
| `clock` | Background graphical clock widget process |
| `ls`, `cat`, `echo` | Standalone CLI file utilities |
| `hello`, `fault`, `spin` | Process spawning, fault handling, and CPU loop verification binaries |

---

## Libraries (`no_std`)

Shared crates live in `userspace/libs/`. Specifications and API contracts:

| Crate | Role | Documentation Sheet |
| --- | --- | --- |
| `libcoeleo` | Syscall wrappers (allocation-free, safe API) | [docs/contrib/libs/libcoeleo.md](docs/contrib/libs/libcoeleo.md) |
| `libcoeleoui` | Immediate-mode UI widget toolkit | [docs/contrib/libs/libcoeleoui.md](docs/contrib/libs/libcoeleoui.md) |
| `coeleo-theme` | Design tokens, color palette, and layout metrics | [docs/contrib/libs/coeleo-theme.md](docs/contrib/libs/coeleo-theme.md) |
| `coeleo-draw` | 2D primitives: SDF round strokes, text, and icons | [docs/contrib/libs/coeleo-draw.md](docs/contrib/libs/coeleo-draw.md) |
| `coeleo-image` | JPEG and PNG image decoding for wallpapers | [docs/contrib/libs/coeleo-image.md](docs/contrib/libs/coeleo-image.md) |
| `pkg` | Ed25519 verification and `.coe` package handling | [docs/contrib/libs/pkg.md](docs/contrib/libs/pkg.md) |

On the host, the package creation tool is [`tools/coe-pack`](docs/contrib/tools-coe-pack.md).

---

## Build & Run

### Prerequisites (Ubuntu/Debian)
```sh
sudo apt install make build-essential xorriso qemu-system-x86 python3 mtools gdisk
```
Rust nightly is configured automatically via `kernel/rust-toolchain.toml`.

### Compilation & Execution
```sh
# Build kernel, userspace ELFs, packages, and disk image
make all

# Run with QEMU (UEFI + VirtIO-blk + VirtIO-net + USB mouse)
make run

# Run with SATA/AHCI disk image (GPT partition)
make run-ahci

# Run with Intel e1000e network controller
make run-e1000e
```

### Automated Tests
```sh
# Run integration test suites
make test-phase8 test-phase9 test-phase10 test-phase13 test-phase14
make test-plasma-p2 test-plasma-p3 test-plasma-p4 test-plasma-p5 test-plasma-p6 test-plasma-p7
```

Detailed test workflows, disk seeding, and development environment setup: [docs/contrib/develop.md](docs/contrib/develop.md).

---

## License

Coeleo OS is distributed under the terms of either the MIT License or the Apache License 2.0, at your option (`MIT OR Apache-2.0`):

- [LICENSE-MIT](LICENSE-MIT)
- [LICENSE-APACHE](LICENSE-APACHE)

Third-party components (under `kernel/vendor/` and the external crates and boot firmware the build links, such as Limine, Flanterm, `smoltcp`, and `fatfs`) keep their own licenses.
