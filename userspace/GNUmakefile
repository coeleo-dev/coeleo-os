MAKEFLAGS += -rR
.SUFFIXES:

override TARGET := x86_64-unknown-none
override PROFILE_DIR := debug

.PHONY: all
all:
	cargo build --target $(TARGET)
	cp target/$(TARGET)/$(PROFILE_DIR)/hello apps/hello/hello
	cp target/$(TARGET)/$(PROFILE_DIR)/fault apps/fault/fault
	cp target/$(TARGET)/$(PROFILE_DIR)/sh apps/sh/sh
	cp target/$(TARGET)/$(PROFILE_DIR)/ls apps/ls/ls
	cp target/$(TARGET)/$(PROFILE_DIR)/cat apps/cat/cat
	cp target/$(TARGET)/$(PROFILE_DIR)/clock apps/clock/clock
	cp target/$(TARGET)/$(PROFILE_DIR)/spin apps/spin/spin
	cp target/$(TARGET)/$(PROFILE_DIR)/winprobe apps/winprobe/winprobe
	cp target/$(TARGET)/$(PROFILE_DIR)/widgets apps/widgets/widgets
	cp target/$(TARGET)/$(PROFILE_DIR)/install apps/install/install
	strip -s apps/hello/hello apps/fault/fault apps/sh/sh apps/ls/ls apps/cat/cat apps/clock/clock apps/spin/spin apps/winprobe/winprobe apps/widgets/widgets apps/install/install
	$(MAKE) -C ../tools/coe-pack
	../tools/coe-pack/target/release/coe-pack pack hello apps/hello/hello -o libs/pkg/hello.coe
	../tools/coe-pack/target/release/coe-pack bad hello apps/hello/hello -o libs/pkg/bad.coe

.PHONY: clean
clean:
	cargo clean
	rm -f apps/hello/hello apps/fault/fault apps/sh/sh apps/ls/ls apps/cat/cat apps/clock/clock apps/spin/spin apps/winprobe/winprobe apps/widgets/widgets apps/install/install
	rm -f libs/pkg/hello.coe libs/pkg/bad.coe
