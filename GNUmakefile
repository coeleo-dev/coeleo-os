# Nuke built-in rules and variables.
MAKEFLAGS += -rR
.SUFFIXES:

# Convenience macro to reliably declare user overridable variables.
override USER_VARIABLE = $(if $(filter $(origin $(1)),default undefined),$(eval override $(1) := $(2)))

# Target architecture to build for. Default to x86_64.
$(call USER_VARIABLE,KARCH,x86_64)

# Default user QEMU flags. These are appended to the QEMU command calls.
$(call USER_VARIABLE,QEMUFLAGS,-m 512M -serial stdio)

override IMAGE_NAME := coeleo
override DISK_IMG := disk.img
override DISK_MIB := 64
override VIRTIO_BLK := -drive file=$(DISK_IMG),if=none,format=raw,id=vd0,cache=writethrough -device virtio-blk-pci,drive=vd0,bootindex=2
override VIRTIO_NET := -nic user,model=virtio-net-pci
override E1000E_NET := -nic user,model=e1000e
override USB_MOUSE := -device piix3-usb-uhci,id=uhci -device usb-mouse,bus=uhci.0
override AHCI_IMG := disk-ahci.img
$(call USER_VARIABLE,USB_DEV,)

.PHONY: all
all: $(IMAGE_NAME).iso userspace

.PHONY: all-hdd
all-hdd: $(IMAGE_NAME).hdd

.PHONY: run
run: run-uefi

.PHONY: run-uefi
run-uefi: edk2-ovmf $(IMAGE_NAME).iso ensure-fat32-disk
	qemu-system-x86_64 \
		-M q35 \
		-drive if=pflash,unit=0,format=raw,file=edk2-ovmf/ovmf-code-x86_64.fd,readonly=on \
		-cdrom $(IMAGE_NAME).iso \
		$(VIRTIO_BLK) \
		$(VIRTIO_NET) \
		$(USB_MOUSE) \
		$(QEMUFLAGS)

.PHONY: run-smp
run-smp: edk2-ovmf $(IMAGE_NAME).iso ensure-fat32-disk
	qemu-system-x86_64 \
		-M q35 \
		-drive if=pflash,unit=0,format=raw,file=edk2-ovmf/ovmf-code-x86_64.fd,readonly=on \
		-cdrom $(IMAGE_NAME).iso \
		$(VIRTIO_BLK) \
		$(VIRTIO_NET) \
		$(USB_MOUSE) \
		$(QEMUFLAGS) \
		-smp 2

.PHONY: run-e1000e
run-e1000e: edk2-ovmf $(IMAGE_NAME).iso ensure-fat32-disk
	qemu-system-x86_64 \
		-M q35 \
		-drive if=pflash,unit=0,format=raw,file=edk2-ovmf/ovmf-code-x86_64.fd,readonly=on \
		-cdrom $(IMAGE_NAME).iso \
		$(VIRTIO_BLK) \
		$(E1000E_NET) \
		$(USB_MOUSE) \
		$(QEMUFLAGS)

.PHONY: run-hdd
run-hdd: run-hdd-uefi

.PHONY: run-hdd-uefi
run-hdd-uefi: edk2-ovmf $(IMAGE_NAME).hdd ensure-fat32-disk
	qemu-system-x86_64 \
		-M q35 \
		-drive if=pflash,unit=0,format=raw,file=edk2-ovmf/ovmf-code-x86_64.fd,readonly=on \
		-hda $(IMAGE_NAME).hdd \
		$(VIRTIO_BLK) \
		$(VIRTIO_NET) \
		$(USB_MOUSE) \
		$(QEMUFLAGS)

.PHONY: run-bios
run-bios: $(IMAGE_NAME).iso ensure-fat32-disk
	qemu-system-x86_64 \
		-M q35 \
		-cdrom $(IMAGE_NAME).iso \
		-boot d \
		$(VIRTIO_BLK) \
		$(VIRTIO_NET) \
		$(USB_MOUSE) \
		$(QEMUFLAGS)

.PHONY: run-hdd-bios
run-hdd-bios: $(IMAGE_NAME).hdd ensure-fat32-disk
	qemu-system-x86_64 \
		-M q35 \
		-hda $(IMAGE_NAME).hdd \
		$(VIRTIO_BLK) \
		$(VIRTIO_NET) \
		$(USB_MOUSE) \
		$(QEMUFLAGS)

.PHONY: run-ahci
run-ahci: edk2-ovmf $(IMAGE_NAME).iso ensure-ahci-disk
	qemu-system-x86_64 \
		-M q35 \
		-drive if=pflash,unit=0,format=raw,file=edk2-ovmf/ovmf-code-x86_64.fd,readonly=on \
		-cdrom $(IMAGE_NAME).iso \
		-drive file=$(AHCI_IMG),if=ide,format=raw \
		$(VIRTIO_NET) \
		$(USB_MOUSE) \
		$(QEMUFLAGS)

.PHONY: run-usb
run-usb: edk2-ovmf $(IMAGE_NAME).iso ensure-ahci-disk
	@test -f usb-stick.img || dd if=/dev/zero of=usb-stick.img bs=1M count=64 status=none
	qemu-system-x86_64 \
		-M q35 \
		-drive if=pflash,unit=0,format=raw,file=edk2-ovmf/ovmf-code-x86_64.fd,readonly=on \
		-cdrom $(IMAGE_NAME).iso \
		-drive file=$(AHCI_IMG),if=none,format=raw,id=live \
		-device ide-hd,bus=ide.0,drive=live \
		-device qemu-xhci,id=xhci \
		-drive file=usb-stick.img,if=none,format=raw,id=stick \
		-device usb-storage,bus=xhci.0,drive=stick \
		$(QEMUFLAGS)

# Real stick on the host, seen by Coeleo as usb-storage on qemu-xhci (probe only at boot).
# Build as your user; sudo is only for QEMU if you cannot write the device.
.PHONY: run-usb-dev
run-usb-dev: edk2-ovmf $(IMAGE_NAME).iso ensure-ahci-disk
	@if [ -z "$(USB_DEV)" ]; then \
		echo 'usage: make run-usb-dev USB_DEV=/dev/sdX'; \
		echo 'identify the stick with: lsblk -d -o NAME,SIZE,TRAN,MODEL'; \
		exit 2; \
	fi
	@if [ ! -b "$(USB_DEV)" ]; then echo "not a block device: $(USB_DEV)"; exit 1; fi
	@typ=$$(lsblk -nr -d -o TYPE "$(USB_DEV)"); \
	if [ "$$typ" != disk ]; then echo "pass the whole disk, not a partition: $(USB_DEV)"; exit 1; fi
	@tran=$$(lsblk -nr -d -o TRAN "$(USB_DEV)"); \
	if [ "$$tran" != usb ]; then echo "refusing non-USB $(USB_DEV) (tran=$$tran)"; exit 1; fi
	@if lsblk -nr -o MOUNTPOINTS "$(USB_DEV)" | grep -q '[^[:space:]]'; then \
		echo "unmount $(USB_DEV) on the host first"; lsblk "$(USB_DEV)"; exit 1; \
	fi
	@if [ ! -w "$(USB_DEV)" ]; then \
		echo "no write access to $(USB_DEV); starting qemu with sudo"; \
		sudo qemu-system-x86_64 \
			-M q35 \
			-drive if=pflash,unit=0,format=raw,file=edk2-ovmf/ovmf-code-x86_64.fd,readonly=on \
			-cdrom $(IMAGE_NAME).iso \
			-drive file=$(AHCI_IMG),if=none,format=raw,id=live \
			-device ide-hd,bus=ide.0,drive=live \
		-device qemu-xhci,id=xhci \
		-drive file=$(USB_DEV),if=none,format=raw,id=stick,cache=none \
		-device usb-storage,bus=xhci.0,drive=stick \
		$(QEMUFLAGS); \
	else \
		qemu-system-x86_64 \
			-M q35 \
			-drive if=pflash,unit=0,format=raw,file=edk2-ovmf/ovmf-code-x86_64.fd,readonly=on \
			-cdrom $(IMAGE_NAME).iso \
			-drive file=$(AHCI_IMG),if=none,format=raw,id=live \
			-device ide-hd,bus=ide.0,drive=live \
			-device qemu-xhci,id=xhci \
			-drive file=$(USB_DEV),if=none,format=raw,id=stick,cache=none \
			-device usb-storage,bus=xhci.0,drive=stick \
			$(QEMUFLAGS); \
	fi

edk2-ovmf:
	curl -L https://github.com/osdev0/edk2-ovmf-nightly/releases/latest/download/edk2-ovmf.tar.gz | gunzip | tar -xf -

limine/limine:
	rm -rf limine
	git clone https://github.com/limine-bootloader/limine.git --branch=v10.x-binary --depth=1
	$(MAKE) -C limine

.PHONY: kernel
kernel:
	$(MAKE) -C kernel

.PHONY: userspace
userspace:
	$(MAKE) -C userspace

# Regenerate site/ from tools/site-gen/content.json. Like tools/gen_wall_thumbs.py,
# this is codegen whose output is committed - the Pages workflow uploads site/ as-is
# and needs no toolchain, so nothing here runs in CI.
.PHONY: site
site:
	python3 tools/site-gen/gen.py

# Fail if site/ drifts from content.json. Reads only; safe to run in CI.
.PHONY: site-check
site-check:
	python3 tools/site-gen/gen.py
	git diff --exit-code -- site/

$(IMAGE_NAME).iso: limine/limine kernel
	rm -rf iso_root
	mkdir -p iso_root/boot
	cp -v kernel/kernel iso_root/boot/
	mkdir -p iso_root/boot/limine
	cp -v limine.conf iso_root/boot/limine/
	mkdir -p iso_root/EFI/BOOT
	cp -v limine/limine-bios.sys limine/limine-bios-cd.bin limine/limine-uefi-cd.bin iso_root/boot/limine/
	cp -v limine/BOOTX64.EFI iso_root/EFI/BOOT/
	cp -v limine/BOOTIA32.EFI iso_root/EFI/BOOT/
	xorriso -as mkisofs -b boot/limine/limine-bios-cd.bin \
		-no-emul-boot -boot-load-size 4 -boot-info-table \
		--efi-boot boot/limine/limine-uefi-cd.bin \
		-efi-boot-part --efi-boot-image --protective-msdos-label \
		iso_root -o $(IMAGE_NAME).iso
	./limine/limine bios-install $(IMAGE_NAME).iso
	rm -rf iso_root

$(IMAGE_NAME).hdd: limine/limine kernel
	rm -f $(IMAGE_NAME).hdd
	dd if=/dev/zero bs=1M count=0 seek=64 of=$(IMAGE_NAME).hdd
	sgdisk $(IMAGE_NAME).hdd -n 1:2048 -t 1:ef00
	./limine/limine bios-install $(IMAGE_NAME).hdd
	mformat -i $(IMAGE_NAME).hdd@@1M
	mmd -i $(IMAGE_NAME).hdd@@1M ::/EFI ::/EFI/BOOT ::/boot ::/boot/limine
	mcopy -i $(IMAGE_NAME).hdd@@1M kernel/kernel ::/boot
	mcopy -i $(IMAGE_NAME).hdd@@1M limine.conf ::/boot/limine
	mcopy -i $(IMAGE_NAME).hdd@@1M limine/limine-bios.sys ::/boot/limine
	mcopy -i $(IMAGE_NAME).hdd@@1M limine/BOOTX64.EFI ::/EFI/BOOT
	mcopy -i $(IMAGE_NAME).hdd@@1M limine/BOOTIA32.EFI ::/EFI/BOOT

$(DISK_IMG): userspace
	dd if=/dev/zero of=$@ bs=1M count=$(DISK_MIB)
	MTOOLS_SKIP_CHECK=1 mformat -i $@ -F -c 1 -v COELEO ::
	MTOOLS_SKIP_CHECK=1 mcopy -i $@ disk-seed/README.TXT ::README.TXT
	MTOOLS_SKIP_CHECK=1 mmd -i $@ ::docs ::bin ::pacotes ::wallpapers
	MTOOLS_SKIP_CHECK=1 mcopy -i $@ disk-seed/docs/HELLO.TXT ::docs/HELLO.TXT
	MTOOLS_SKIP_CHECK=1 mcopy -i $@ userspace/apps/hello/hello ::hello
	MTOOLS_SKIP_CHECK=1 mcopy -i $@ userspace/apps/fault/fault ::fault
	MTOOLS_SKIP_CHECK=1 mcopy -i $@ userspace/apps/sh/sh ::sh
	MTOOLS_SKIP_CHECK=1 mcopy -i $@ userspace/apps/ls/ls ::ls
	MTOOLS_SKIP_CHECK=1 mcopy -i $@ userspace/apps/cat/cat ::cat
	MTOOLS_SKIP_CHECK=1 mcopy -i $@ userspace/apps/echo/echo ::echo
	MTOOLS_SKIP_CHECK=1 mcopy -i $@ userspace/apps/edit/edit ::edit
	MTOOLS_SKIP_CHECK=1 mcopy -i $@ userspace/apps/clock/clock ::clock
	MTOOLS_SKIP_CHECK=1 mcopy -i $@ userspace/apps/spin/spin ::spin
	MTOOLS_SKIP_CHECK=1 mcopy -i $@ userspace/apps/winprobe/winprobe ::winprobe
	MTOOLS_SKIP_CHECK=1 mcopy -i $@ userspace/apps/widgets/widgets ::widgets
	MTOOLS_SKIP_CHECK=1 mcopy -i $@ userspace/apps/install/install ::install
	MTOOLS_SKIP_CHECK=1 mcopy -i $@ userspace/apps/threads/threads ::threads
	MTOOLS_SKIP_CHECK=1 mcopy -i $@ userspace/libs/pkg/hello.coe ::pacotes/hello.coe
	MTOOLS_SKIP_CHECK=1 mcopy -i $@ disk-seed/wallpapers/*.png ::wallpapers/

.PHONY: ensure-fat32-disk
ensure-fat32-disk: userspace
	@ok=0; \
	if [ -f $(DISK_IMG) ] && file -b $(DISK_IMG) | grep -q 'FAT (32 bit)' \
		&& MTOOLS_SKIP_CHECK=1 mdir -i $(DISK_IMG) :: >/dev/null 2>&1; then \
		MTOOLS_SKIP_CHECK=1 mmd -D s -i $(DISK_IMG) ::bin || true; \
		MTOOLS_SKIP_CHECK=1 mmd -D s -i $(DISK_IMG) ::pacotes || true; \
		MTOOLS_SKIP_CHECK=1 mmd -D s -i $(DISK_IMG) ::wallpapers || true; \
		MTOOLS_SKIP_CHECK=1 mcopy -o -i $(DISK_IMG) userspace/apps/hello/hello ::hello \
		&& MTOOLS_SKIP_CHECK=1 mcopy -o -i $(DISK_IMG) userspace/apps/fault/fault ::fault \
		&& MTOOLS_SKIP_CHECK=1 mcopy -o -i $(DISK_IMG) userspace/apps/sh/sh ::sh \
		&& MTOOLS_SKIP_CHECK=1 mcopy -o -i $(DISK_IMG) userspace/apps/ls/ls ::ls \
		&& MTOOLS_SKIP_CHECK=1 mcopy -o -i $(DISK_IMG) userspace/apps/cat/cat ::cat \
		&& MTOOLS_SKIP_CHECK=1 mcopy -o -i $(DISK_IMG) userspace/apps/echo/echo ::echo \
		&& MTOOLS_SKIP_CHECK=1 mcopy -o -i $(DISK_IMG) userspace/apps/edit/edit ::edit \
		&& MTOOLS_SKIP_CHECK=1 mcopy -o -i $(DISK_IMG) userspace/apps/clock/clock ::clock \
		&& MTOOLS_SKIP_CHECK=1 mcopy -o -i $(DISK_IMG) userspace/apps/spin/spin ::spin \
		&& MTOOLS_SKIP_CHECK=1 mcopy -o -i $(DISK_IMG) userspace/apps/winprobe/winprobe ::winprobe \
		&& MTOOLS_SKIP_CHECK=1 mcopy -o -i $(DISK_IMG) userspace/apps/widgets/widgets ::widgets \
		&& MTOOLS_SKIP_CHECK=1 mcopy -o -i $(DISK_IMG) userspace/apps/install/install ::install \
		&& MTOOLS_SKIP_CHECK=1 mcopy -o -i $(DISK_IMG) userspace/apps/threads/threads ::threads \
		&& MTOOLS_SKIP_CHECK=1 mcopy -o -i $(DISK_IMG) userspace/libs/pkg/hello.coe ::pacotes/hello.coe \
		&& MTOOLS_SKIP_CHECK=1 mcopy -o -i $(DISK_IMG) disk-seed/wallpapers/*.png ::wallpapers/ \
		&& ok=1; \
	fi; \
	if [ $$ok -eq 0 ]; then \
		echo 'FAT disk unusable; recreating $(DISK_IMG)'; \
		rm -f $(DISK_IMG); \
		$(MAKE) $(DISK_IMG); \
	fi

$(AHCI_IMG): userspace kernel
	dd if=/dev/zero of=$@ bs=1M count=$(DISK_MIB)
	sgdisk $@ -n 1:2048 -t 1:0700
	MTOOLS_SKIP_CHECK=1 mformat -i $@@@1M -F -c 1 -v COELEO ::
	MTOOLS_SKIP_CHECK=1 mcopy -i $@@@1M disk-seed/README.TXT ::README.TXT
	MTOOLS_SKIP_CHECK=1 mmd -i $@@@1M ::docs ::bin ::pacotes ::wallpapers ::/boot ::/boot/limine ::/EFI ::/EFI/BOOT
	MTOOLS_SKIP_CHECK=1 mcopy -i $@@@1M disk-seed/docs/HELLO.TXT ::docs/HELLO.TXT
	MTOOLS_SKIP_CHECK=1 mcopy -i $@@@1M kernel/kernel ::/boot/kernel
	MTOOLS_SKIP_CHECK=1 mcopy -i $@@@1M limine.conf ::/boot/limine/limine.conf
	MTOOLS_SKIP_CHECK=1 mcopy -i $@@@1M limine/limine-bios.sys ::/boot/limine/limine-bios.sys
	MTOOLS_SKIP_CHECK=1 mcopy -i $@@@1M limine/BOOTX64.EFI ::/EFI/BOOT/BOOTX64.EFI
	@if [ -f limine/BOOTIA32.EFI ]; then MTOOLS_SKIP_CHECK=1 mcopy -i $@@@1M limine/BOOTIA32.EFI ::/EFI/BOOT/BOOTIA32.EFI; fi
	MTOOLS_SKIP_CHECK=1 mcopy -i $@@@1M userspace/apps/hello/hello ::hello
	MTOOLS_SKIP_CHECK=1 mcopy -i $@@@1M userspace/apps/fault/fault ::fault
	MTOOLS_SKIP_CHECK=1 mcopy -i $@@@1M userspace/apps/sh/sh ::sh
	MTOOLS_SKIP_CHECK=1 mcopy -i $@@@1M userspace/apps/ls/ls ::ls
	MTOOLS_SKIP_CHECK=1 mcopy -i $@@@1M userspace/apps/cat/cat ::cat
	MTOOLS_SKIP_CHECK=1 mcopy -i $@@@1M userspace/apps/echo/echo ::echo
	MTOOLS_SKIP_CHECK=1 mcopy -i $@@@1M userspace/apps/edit/edit ::edit
	MTOOLS_SKIP_CHECK=1 mcopy -i $@@@1M userspace/apps/clock/clock ::clock
	MTOOLS_SKIP_CHECK=1 mcopy -i $@@@1M userspace/apps/spin/spin ::spin
	MTOOLS_SKIP_CHECK=1 mcopy -i $@@@1M userspace/apps/winprobe/winprobe ::winprobe
	MTOOLS_SKIP_CHECK=1 mcopy -i $@@@1M userspace/apps/widgets/widgets ::widgets
	MTOOLS_SKIP_CHECK=1 mcopy -i $@@@1M userspace/apps/install/install ::install
	MTOOLS_SKIP_CHECK=1 mcopy -i $@@@1M userspace/libs/pkg/hello.coe ::pacotes/hello.coe
	MTOOLS_SKIP_CHECK=1 mcopy -i $@@@1M disk-seed/wallpapers/*.png ::wallpapers/

.PHONY: ensure-ahci-disk
ensure-ahci-disk: userspace kernel
	@ok=0; \
	if [ -f $(AHCI_IMG) ] \
		&& MTOOLS_SKIP_CHECK=1 mdir -i $(AHCI_IMG)@@1M :: >/dev/null 2>&1; then \
		MTOOLS_SKIP_CHECK=1 mmd -D s -i $(AHCI_IMG)@@1M ::bin || true; \
		MTOOLS_SKIP_CHECK=1 mmd -D s -i $(AHCI_IMG)@@1M ::pacotes || true; \
		MTOOLS_SKIP_CHECK=1 mmd -D s -i $(AHCI_IMG)@@1M ::wallpapers || true; \
		MTOOLS_SKIP_CHECK=1 mmd -D s -i $(AHCI_IMG)@@1M ::/boot ::/boot/limine ::/EFI ::/EFI/BOOT || true; \
		MTOOLS_SKIP_CHECK=1 mcopy -o -i $(AHCI_IMG)@@1M userspace/apps/hello/hello ::hello \
		&& MTOOLS_SKIP_CHECK=1 mcopy -o -i $(AHCI_IMG)@@1M userspace/apps/fault/fault ::fault \
		&& MTOOLS_SKIP_CHECK=1 mcopy -o -i $(AHCI_IMG)@@1M userspace/apps/sh/sh ::sh \
		&& MTOOLS_SKIP_CHECK=1 mcopy -o -i $(AHCI_IMG)@@1M userspace/apps/ls/ls ::ls \
		&& MTOOLS_SKIP_CHECK=1 mcopy -o -i $(AHCI_IMG)@@1M userspace/apps/cat/cat ::cat \
		&& MTOOLS_SKIP_CHECK=1 mcopy -o -i $(AHCI_IMG)@@1M userspace/apps/echo/echo ::echo \
		&& MTOOLS_SKIP_CHECK=1 mcopy -o -i $(AHCI_IMG)@@1M userspace/apps/edit/edit ::edit \
		&& MTOOLS_SKIP_CHECK=1 mcopy -o -i $(AHCI_IMG)@@1M userspace/apps/clock/clock ::clock \
		&& MTOOLS_SKIP_CHECK=1 mcopy -o -i $(AHCI_IMG)@@1M userspace/apps/spin/spin ::spin \
		&& MTOOLS_SKIP_CHECK=1 mcopy -o -i $(AHCI_IMG)@@1M userspace/apps/winprobe/winprobe ::winprobe \
		&& MTOOLS_SKIP_CHECK=1 mcopy -o -i $(AHCI_IMG)@@1M userspace/apps/widgets/widgets ::widgets \
		&& MTOOLS_SKIP_CHECK=1 mcopy -o -i $(AHCI_IMG)@@1M userspace/apps/install/install ::install \
		&& MTOOLS_SKIP_CHECK=1 mcopy -o -i $(AHCI_IMG)@@1M userspace/libs/pkg/hello.coe ::pacotes/hello.coe \
		&& MTOOLS_SKIP_CHECK=1 mcopy -o -i $(AHCI_IMG)@@1M disk-seed/wallpapers/*.png ::wallpapers/ \
		&& MTOOLS_SKIP_CHECK=1 mcopy -o -i $(AHCI_IMG)@@1M kernel/kernel ::/boot/kernel \
		&& MTOOLS_SKIP_CHECK=1 mcopy -o -i $(AHCI_IMG)@@1M limine.conf ::/boot/limine/limine.conf \
		&& MTOOLS_SKIP_CHECK=1 mcopy -o -i $(AHCI_IMG)@@1M limine/limine-bios.sys ::/boot/limine/limine-bios.sys \
		&& MTOOLS_SKIP_CHECK=1 mcopy -o -i $(AHCI_IMG)@@1M limine/BOOTX64.EFI ::/EFI/BOOT/BOOTX64.EFI \
		&& ok=1; \
		if [ -f limine/BOOTIA32.EFI ]; then MTOOLS_SKIP_CHECK=1 mcopy -o -i $(AHCI_IMG)@@1M limine/BOOTIA32.EFI ::/EFI/BOOT/BOOTIA32.EFI; fi; \
	fi; \
	if [ $$ok -eq 0 ]; then \
		echo 'AHCI FAT unusable; recreating $(AHCI_IMG)'; \
		rm -f $(AHCI_IMG); \
		$(MAKE) $(AHCI_IMG); \
	fi

.PHONY: clean
clean:
	$(MAKE) -C kernel clean
	$(MAKE) -C userspace clean
	rm -rf iso_root $(IMAGE_NAME).iso $(IMAGE_NAME).hdd $(DISK_IMG) $(AHCI_IMG)

.PHONY: test-phase1
test-phase1: $(IMAGE_NAME).iso
	@set -e; \
	log=$$(mktemp); \
	trap 'rm -f "$$log"' EXIT; \
	timeout 8 qemu-system-x86_64 \
		-M q35 \
		-m 512M \
		-cdrom $(IMAGE_NAME).iso \
		-serial stdio \
		-display none \
		-no-reboot \
		-no-shutdown \
		>"$$log" 2>&1 \
		|| true; \
	if grep -q 'Coeleo OS' "$$log" && grep -q 'coeleo>' "$$log"; then \
		echo 'test-phase1: ok'; \
	else \
		echo 'test-phase1: serial did not contain Coeleo OS and coeleo>'; \
		cat "$$log"; \
		exit 1; \
	fi

.PHONY: test-phase2
test-phase2: $(IMAGE_NAME).iso
	python3 scripts/test-phase2.py $(IMAGE_NAME).iso

.PHONY: test-phase3
test-phase3: $(IMAGE_NAME).iso
	python3 scripts/test-phase3.py $(IMAGE_NAME).iso

.PHONY: test-phase4
test-phase4: $(IMAGE_NAME).iso
	python3 scripts/test-phase4.py $(IMAGE_NAME).iso

.PHONY: test-phase5
test-phase5: $(IMAGE_NAME).iso
	python3 scripts/test-phase5.py $(IMAGE_NAME).iso

.PHONY: test-phase6
test-phase6: $(IMAGE_NAME).iso
	python3 scripts/test-phase6.py $(IMAGE_NAME).iso

.PHONY: test-phase7
test-phase7: $(IMAGE_NAME).iso
	python3 scripts/test-phase7.py $(IMAGE_NAME).iso

.PHONY: test-phase8
test-phase8: $(IMAGE_NAME).iso userspace
	python3 scripts/test-phase8.py $(IMAGE_NAME).iso userspace/apps/hello/hello userspace/apps/fault/fault

.PHONY: test-phase9
test-phase9: $(IMAGE_NAME).iso userspace
	python3 scripts/test-phase9.py $(IMAGE_NAME).iso userspace/apps/sh/sh userspace/apps/ls/ls userspace/apps/cat/cat

.PHONY: test-phase10
test-phase10: $(IMAGE_NAME).iso userspace
	python3 scripts/test-phase10.py $(IMAGE_NAME).iso userspace/apps/sh/sh userspace/apps/clock/clock userspace/apps/spin/spin userspace/apps/hello/hello

.PHONY: test-phase35
test-phase35: $(IMAGE_NAME).iso userspace
	python3 scripts/test-phase35.py $(IMAGE_NAME).iso userspace/apps/sh/sh userspace/apps/clock/clock userspace/apps/hello/hello

.PHONY: test-phase36
test-phase36: $(IMAGE_NAME).iso userspace
	python3 scripts/test-phase36.py $(IMAGE_NAME).iso userspace/apps/sh/sh userspace/apps/threads/threads

.PHONY: test-phase20
test-phase20: $(IMAGE_NAME).iso userspace
	python3 scripts/test-phase20.py $(IMAGE_NAME).iso userspace/apps/sh/sh userspace/apps/hello/hello userspace/apps/cat/cat

.PHONY: test-phase20b
test-phase20b: $(IMAGE_NAME).iso userspace
	python3 scripts/test-phase20b.py $(IMAGE_NAME).iso userspace/apps/sh/sh userspace/apps/hello/hello

.PHONY: test-phase20c
test-phase20c: $(IMAGE_NAME).iso userspace
	python3 scripts/test-phase20c.py $(IMAGE_NAME).iso userspace/apps/sh/sh userspace/apps/echo/echo userspace/apps/cat/cat

.PHONY: test-phase21
test-phase21: $(IMAGE_NAME).iso userspace
	python3 scripts/test-phase21.py $(IMAGE_NAME).iso userspace/apps/sh/sh userspace/apps/edit/edit userspace/apps/cat/cat userspace/libs/pkg/hello.coe

.PHONY: test-phase11
test-phase11: $(IMAGE_NAME).iso userspace
	python3 scripts/test-phase11.py $(IMAGE_NAME).iso userspace/apps/sh/sh

.PHONY: test-phase12
test-phase12: $(IMAGE_NAME).iso userspace
	python3 scripts/test-phase12.py $(IMAGE_NAME).iso userspace/apps/sh/sh

.PHONY: test-phase18
test-phase18: $(IMAGE_NAME).iso userspace
	python3 scripts/test-phase18.py $(IMAGE_NAME).iso userspace/apps/sh/sh

.PHONY: test-phase19
test-phase19: $(IMAGE_NAME).iso userspace
	python3 scripts/test-phase19.py $(IMAGE_NAME).iso userspace/apps/sh/sh

.PHONY: test-phase13
test-phase13: $(IMAGE_NAME).iso userspace
	python3 scripts/test-phase13.py $(IMAGE_NAME).iso userspace/apps/sh/sh

.PHONY: test-phase14
test-phase14: $(IMAGE_NAME).iso userspace
	python3 scripts/test-phase14.py $(IMAGE_NAME).iso userspace/apps/sh/sh userspace/libs/pkg/hello.coe userspace/libs/pkg/bad.coe

.PHONY: test-plasma-p1
test-plasma-p1: test-phase13

.PHONY: test-plasma-p2
test-plasma-p2: $(IMAGE_NAME).iso userspace
	python3 scripts/test-plasma-p2.py $(IMAGE_NAME).iso userspace/apps/sh/sh userspace/apps/winprobe/winprobe

.PHONY: test-plasma-p3
test-plasma-p3: $(IMAGE_NAME).iso userspace
	python3 scripts/test-plasma-p3.py $(IMAGE_NAME).iso userspace/apps/sh/sh userspace/apps/widgets/widgets

.PHONY: test-plasma-p4
test-plasma-p4: $(IMAGE_NAME).iso userspace
	python3 scripts/test-plasma-p4.py $(IMAGE_NAME).iso userspace/apps/sh/sh userspace/apps/widgets/widgets

.PHONY: test-plasma-p5
test-plasma-p5: $(IMAGE_NAME).iso userspace
	python3 scripts/test-plasma-p5.py $(IMAGE_NAME).iso userspace/apps/sh/sh userspace/apps/widgets/widgets

.PHONY: test-plasma-p6
test-plasma-p6: $(IMAGE_NAME).iso userspace
	python3 scripts/test-plasma-p6.py $(IMAGE_NAME).iso userspace/apps/sh/sh userspace/apps/hello/hello

.PHONY: test-plasma-p7
test-plasma-p7: $(IMAGE_NAME).iso userspace
	python3 scripts/test-plasma-p7.py $(IMAGE_NAME).iso userspace/apps/sh/sh

.PHONY: test-desk
test-desk: $(IMAGE_NAME).iso userspace
	python3 scripts/test-desk.py $(IMAGE_NAME).iso userspace/apps/sh/sh

.PHONY: test-phase15
test-phase15: $(IMAGE_NAME).iso userspace
	python3 scripts/test-phase15.py $(IMAGE_NAME).iso

.PHONY: test-phase16
test-phase16: $(IMAGE_NAME).iso userspace
	python3 scripts/test-phase16.py $(IMAGE_NAME).iso userspace/apps/sh/sh

.PHONY: test-phase17.1
test-phase17.1: $(IMAGE_NAME).iso userspace
	python3 scripts/test-phase17.1.py $(IMAGE_NAME).iso

# Live FAT is 17.1 (GPT 0700) plus the all-hdd tree: /boot/kernel, limine, EFI/BOOT.
.PHONY: test-phase17.2
test-phase17.2: $(IMAGE_NAME).iso userspace edk2-ovmf
	python3 scripts/test-phase17.2.py $(IMAGE_NAME).iso

.PHONY: test-phase17
test-phase17: $(IMAGE_NAME).iso userspace edk2-ovmf
	python3 scripts/test-phase17.py $(IMAGE_NAME).iso

.PHONY: test-phase17-usb
test-phase17-usb: $(IMAGE_NAME).iso userspace edk2-ovmf
	python3 scripts/test-phase17-usb.py $(IMAGE_NAME).iso

.PHONY: test-phase22
test-phase22: $(IMAGE_NAME).iso userspace
	python3 scripts/test-phase22.py $(IMAGE_NAME).iso userspace/apps/sh/sh userspace/libs/pkg/hello.coe userspace/libs/pkg/bad.coe

.PHONY: test-phase25
test-phase25: $(IMAGE_NAME).iso
	python3 scripts/test-phase25.py $(IMAGE_NAME).iso

.PHONY: distclean
distclean: clean
	$(MAKE) -C kernel distclean
	rm -rf limine edk2-ovmf ovmf
