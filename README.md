# Coeleo OS

Sistema operacional do zero em Rust, x86_64, boot via Limine. Spec normativo: [docs/requirements/coeleo-os-specification-v02.md](docs/requirements/coeleo-os-specification-v02.md). Contribuir: [docs/contrib/](docs/contrib/). Depois da fase 15: [docs/roadmap-pos-fase-15.md](docs/roadmap-pos-fase-15.md). Índice 16–49: [docs/requirements/implementation-plan-full.md](docs/requirements/implementation-plan-full.md).

**Fase 13 — Janela híbrida / Plasma-1.** Acima de um painel de 32 px (relógio mm:ss à direita) o compositor empilha janelas opacas: o shell (Flanterm / `sh` ELF) e o gestor de ficheiros. O rato (PS/2 e USB HID boot via UHCI) move um cursor por dirty rects; o clique na lista abre texto. Clicar no painel inferior não trata a faixa como lista. `make test-phase13` (alias `make test-plasma-p1`) mexe o rato por QMP, exige `wm: windows`, `panel: h=32`, `mouse: usb`, `blit:` pequeno e `fm: README.TXT`.

**Plasma-2 — Superfície Ring 3.** `run winprobe` cria um rectângulo 64×64 (`win_create` / `win_damage`); o compositor copia-o para a área de trabalho. `make test-plasma-p2` exige `win: create id=1` e `blit: 64x64`.

**Plasma-3 — Toolkit + botão.** `run widgets` desenha label, campo e botão via `libcoeleoui` numa janela com barra de 32 px. Um clique QMP no botão (abaixo da deco) escreve `ui: clicked` na serial. `make test-plasma-p3`.

**Plasma-4 — Janelas empilhadas + sombra.** VT e `files` são frames com título, overlap e anel de sombra; arrastar a barra move a janela; `X` fecha um cliente Ring 3. `make test-plasma-p4`.

**Plasma-5 — Launcher + tarefas.** Painel `[>]` | tarefas | relógio; minimizar deixa o botão; maximizar torna o painel opaco (`panel: opaque`). `make test-plasma-p5`.

**Plasma-6 — KRunner.** Alt+Space abre uma faixa ao centro; prefixo do nome; Enter lança o ELF (`run: hello` + `hello`). Esc fecha sem spawn. `make test-plasma-p6`.

**Plasma-7 — Secretária polida.** Painel flutuante preto, fonte GUI grayscale (~11 px, atlas no build), wallpaper JPEG, preview jpg/png no gestor, KRunner no painel. `make test-plasma-p7`.

**Fase 10 — Multitarefa.** Com `sh` na FAT, `ps` lista o shell e um `clock` em background. O timer LAPIC troca contexto em Ring 3: um `spin` em loop não engole o teclado; Ctrl+C mata o da frente e o prompt ELF continua. `hello` corre via `spawn` + `wait`.

**Fase 11 — Rede ping.** Com `sh` no disco, `ping 10.0.2.2` (syscall `net_ping`; smoltcp + virtio-net no kernel, IP estático `10.0.2.15/24`). O aceite é um `reply from` ao gateway SLIRP do QEMU. ICMP à Internet (`8.8.8.8`) no user-net **não** é aceite desta fase.

**Fase 15 — Hardware real (AHCI).** Sem virtio-blk, o mesmo `sh` ELF fala com um disco SATA: `disk` mostra `ahci`, `ls`/`write` numa FAT32 em partição GPT. No QEMU isso é `make test-phase15` / `make run-ahci`. O aceite do spec é um PC UEFI (checklist abaixo).

## Dependências

- GNU make (`make` no Linux; `gmake` noutros Unix)
- Rust nightly (o `kernel/rust-toolchain.toml` escolhe o toolchain e o target `x86_64-unknown-none`)
- `gcc` ou `clang` (o `build.rs` compila o Flanterm C com o crate `cc`; por omissão usa `gcc`, ou `$CC` se estiver definido)
- `xorriso` (ISO)
- `qemu-system-x86` para correr
- `python3` para `make test-phase2` até `make test-phase13`, `make test-plasma-p2` … `make test-plasma-p7` e `make test-phase15`
- `mtools` (`mformat`, `mcopy`, `mmd`, `mtype`) para formatar `disk.img` em FAT32 e ler ficheiros no host
- `sgdisk` (gdisk) para `make test-phase15`, `make run-ahci` e `make all-hdd`

No Debian/Ubuntu:

```sh
sudo apt install make build-essential xorriso qemu-system-x86 python3 mtools gdisk
```

Rust: [rustup](https://rustup.rs/). Na primeira compilação o rustup instala nightly e o target a partir do `rust-toolchain.toml`.

## Comandos

| Comando | Efeito |
| --- | --- |
| `make all` | Compila o kernel, os ELF `hello`/`fault`/`sh`/`ls`/`cat`/`clock`/`spin`/`winprobe`/`widgets`, e gera `coeleo.iso` |
| `make run` | ISO + QEMU UEFI (OVMF) + virtio-blk (`disk.img`) + virtio-net (user-net) + rato USB (UHCI); texto no ecrã e na serial do host. O kernel é **release** (`opt-level` do perfil `release`). |
| `make run-uefi` | Igual a `make run` |
| `make run-bios` | ISO + QEMU BIOS + virtio-blk + virtio-net |
| `make run-ahci` | ISO + QEMU UEFI + SATA/AHCI (`disk-ahci.img` GPT+FAT32); sem virtio-blk; virtio-net para `ping` |
| `make run-usb` | ISO + live AHCI + `qemu-xhci` + imagem `usb-stick.img` (não é o pendrive físico) |
| `make run-usb-dev USB_DEV=/dev/sdX` | Igual ao `run-usb`, mas o stick real (`lsblk`, TRAN=usb). Sem `sudo make` (o root não tem `cargo`); o alvo usa `sudo` só no QEMU. `install` apaga o dispositivo. |
| `make test-phase1` | BIOS headless: a serial do host tem de conter `Coeleo OS` e `coeleo>` |
| `make test-phase2` | BIOS headless: QMP envia teclas; a serial tem de conter o texto de `help` |
| `make test-phase3` | BIOS headless: QMP envia `mem` e `panic`; a serial tem frames e `panic:` |
| `make test-phase4` | BIOS headless: dois `uptime` com pelo menos 1 s de avanço; `help` ainda funciona |
| `make test-phase5` | BIOS headless: QMP envia `disk`; a serial tem `64 MiB` e `131072 sectors` |
| `make test-phase6` | BIOS headless: QMP envia `ls`/`cd`/`cat` numa imagem FAT temporária; `cat README.TXT` mostra o conteúdo |
| `make test-phase7` | BIOS headless: três boots; `write persist.txt` + `sync`; o host lê o ficheiro com `mtype` |
| `make test-phase8` | BIOS headless: `run hello` imprime `hello`; `run fault` imprime `run: fault` e o prompt volta |
| `make test-phase9` | BIOS headless: boot no `sh` ELF; `ls`/`cat`/`cd`; `exit` respawna o prompt |
| `make test-phase10` | BIOS headless: `ps` vê `sh`+`clock`; `hello`; `spin`+Ctrl+C; o shell ELF continua |
| `make test-phase11` | BIOS headless: FAT + `sh` ELF + virtio-net; `ping 10.0.2.2` imprime `reply from 10.0.2.2` |
| `make test-phase12` | BIOS headless: FAT + `sh` ELF; `get http://10.0.2.2:<port>/` imprime o corpo HTTP |
| `make test-phase13` | BIOS headless: FAT + `sh` ELF + UHCI `usb-mouse`; `wm: windows`; `panel: h=32`; clique no strut **não** foca o gestor; clique na lista do fm abre `fm: README.TXT` (`make test-plasma-p1` é o mesmo teste) |
| `make test-plasma-p2` | BIOS headless: FAT + `sh` + `winprobe`; `run winprobe` produz `win: create id=1` e `blit: 64x64` |
| `make test-plasma-p3` | BIOS headless: FAT + `sh` + `widgets` + UHCI `usb-mouse`; `run widgets` e clique QMP no botão (coords de cliente abaixo da deco); serial contém `ui: clicked` |
| `make test-plasma-p4` | BIOS headless: FAT + `sh` + `widgets` + UHCI; `shadow:` no boot; drag da barra; `win: close id=1` no `X` |
| `make test-plasma-p5` | BIOS headless: FAT + `sh` + `widgets` + UHCI; `panel: alpha`/`opaque`; `wm: min`; launcher abre `widgets` |
| `make test-plasma-p6` | BIOS headless: FAT + `sh` + `hello` + UHCI; Alt+Space abre KRunner; `hello`+Enter → `run: hello`; Esc fecha sem segundo spawn |
| `make test-plasma-p7` | BIOS headless: FAT + `sh` + `wallpaper.jpg`; `desk: wallpaper`; lupa do painel abre KRunner; clique no jpg → `fm: image` |
| `make test-phase15` | BIOS headless: SATA/AHCI sem virtio; `sh` ELF; `disk: ahci`; `ls`/`write`/`sync` numa partição GPT; `mtype @@1M` no host |
| `make clean` | Binários, ISO, `disk.img` e `disk-ahci.img` |
| `make distclean` | Também Limine clonado e firmware OVMF |

QEMU usa 512 MiB e `-serial stdio`. Rede: virtio-net com IP estático **10.0.2.15/24** e gateway **10.0.2.2** (user-net). No `sh` ELF, `ping 10.0.2.2` deve mostrar `reply from 10.0.2.2`. ICMP para a Internet no user-net **não** faz parte do aceite.

`make run` gera `disk.img` (64 MiB, FAT32 com as sementes em `disk-seed/`) se ainda não existir, e recria a imagem se for zeros da fase 5 (não toca numa FAT32 já válida).

Ficheiros extra do host, sem montar:

```sh
mcopy -i disk.img meu.txt ::
mtype -i disk.img ::persist.txt
```

`write` no `sh` ELF pede linhas até uma linha só com `.`. `sync` antes de fechar o QEMU para o host e o próximo boot verem o ficheiro. Os ELF `hello`, `fault`, `sh`, `ls`, `cat`, `clock` e `spin` vêm de `make -C userspace` e são copiados para `disk.img` e `disk-ahci.img`. Sem `sh` no disco, o kernel fica no prompt in-kernel (`run`, `write`, `help`, …) — fallback para testes sem userspace.

`make run-ahci` usa `disk-ahci.img` (GPT, uma partição FAT32 a 1 MiB), com as mesmas sementes e o mesmo userspace (`sh`, `hello`, `clock`, …). O disco é SATA (sem virtio-blk); a NIC continua virtio-net para `ping`. O host lê essa partição com `mtype -i disk-ahci.img@@1M ::persist.txt`.

## Hardware real (aceite da fase 15)

O spec fecha a fase 15 num PC, não só no QEMU. `make test-phase15` prova o driver; isto prova a máquina.

**Máquina**

- x86_64, firmware **UEFI**, modo SATA **AHCI** (não Intel RST/RAID, não IDE).
- Disco **SATA** sobressalente (HDD/SSD interno). NVMe e SSD em caixa USB estão fora.
- Teclado **PS/2** na porta laranja/roxa. USB-only está fora desta fase (sem pilha USB).
- RAM ≥ 128 MiB (512 MiB típico). Sem requisito de GPU.

**Instalação** (disco **alvo**, nunca o disco do Linux hospedeiro)

1. `make all-hdd` gera `coeleo.hdd`.
2. Copiar sementes para a ESP: `mcopy -i coeleo.hdd@@1M disk-seed/README.TXT ::README.TXT` (e `docs/` analogamente: `mmd` + `mcopy` de `HELLO.TXT`).
3. Identificar o dispositivo (`lsblk`); conferir **duas vezes**.
4. `sudo dd if=coeleo.hdd of=/dev/sdX bs=4M status=progress conv=fsync`.
5. Firmware: boot UEFI dessa unidade; Limine → Coeleo OS.
6. No `sh`: `disk` mostra `disk: ahci …`; `ls` mostra `README.TXT` e `EFI/` ou `boot/`; `cat README.TXT`; `write` + `sync`; reboot; `cat` outra vez.
7. Teclado: ASCII + Backspace + Enter, como na fase 2.

Fora desta fase: USB, Wi-Fi, rede, GPU, suspend, segundo disco, NVMe.

## O que deve aparecer

`make all` deixa `coeleo.iso` na raiz. Em `make run`, o menu Limine arranca **Coeleo OS** e o ecrã (e a serial) mostram:

```
Coeleo OS
coeleo>
```

No ecrã, o split (shell | ficheiros) fica **acima** de uma faixa de 32 px com o relógio `mm:ss` à direita. À direita, o gestor `files` lista a FAT; um clique (ou Tab) muda o foco. O clique no painel inferior não selecciona linhas da lista. O cursor do rato (PS/2 ou USB) move-se só com dirty rects. Com `sh` no disco (`make run`), o prompt `coeleo>` é o shell ELF: `ls` lista `README.TXT` e `docs/`; `cat README.TXT` imprime `README from FAT`; `cd docs` e `cat HELLO.TXT` mostram `nested ok`; `ps` mostra `sh`; `clock` arranca um processo em background e `ps` passa a listar `sh` e `clock` (linhas `tick N` no ecrã); `hello` imprime `hello` e volta o prompt; `spin` entra em loop e Ctrl+C mata-o sem cair no REPL in-kernel; `exit` volta a mostrar o prompt. Sem `sh` (testes 1–4 sem disco, ou FAT só com sementes), o prompt in-kernel responde a `uptime`, `mem`, `disk`, `touch`/`write`/`sync`, `run hello`, `ping 10.0.2.2` e `help` (1ª linha igual; 2ª `mem, panic, uptime, disk, ls, cd, cat, touch, write, rm, sync, run, ping`). `disk` imprime `disk: virtio-blk 64 MiB (131072 sectors)` em `make run` com fallback, ou `disk: ahci …` em `make run-ahci` no fallback. `panic` mostra `panic:` e pára. Sem disco no QEMU, `disk` imprime `disk: none` e `ls` imprime `ls: no filesystem`.
