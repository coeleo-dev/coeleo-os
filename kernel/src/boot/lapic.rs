//! Local APIC: xAPIC MMIO, PIT channel 2 used once to calibrate, then 100 Hz periodic.

use core::ptr;
use core::sync::atomic::{AtomicPtr, AtomicU32, Ordering};

use x86_64::instructions::port::Port;
use x86_64::registers::model_specific::{ApicBase, ApicBaseFlags};

use crate::vmm;

pub const TIMER_VECTOR: u8 = 48;
pub const SPURIOUS_VECTOR: u8 = 0xFF;

const EOI: u32 = 0xB0;
const SVR: u32 = 0xF0;
const LVT_TIMER: u32 = 0x320;
const LVT_LINT0: u32 = 0x350;
const LVT_ERROR: u32 = 0x370;
const INIT: u32 = 0x380;
const CURRENT: u32 = 0x390;
const DIVIDE: u32 = 0x3E0;

const SVR_SW_ENABLE: u32 = 1 << 8;
const LVT_MASKED: u32 = 1 << 16;
const LVT_PERIODIC: u32 = 1 << 17;
const LINT_EXTINT: u32 = 0b111 << 8;
const DIVIDE_BY_16: u32 = 0b0011;

const PIT_HZ: u32 = 1_193_182;
const CALIBRATE_MS: u32 = 10;

static MMIO: AtomicPtr<u8> = AtomicPtr::new(ptr::null_mut());
static TICKS_PER_10MS: AtomicU32 = AtomicU32::new(0);

pub fn init_bsp() {
    map_common();
    write_reg(SVR, SVR_SW_ENABLE | u32::from(SPURIOUS_VECTOR));
    write_reg(LVT_ERROR, LVT_MASKED | u32::from(SPURIOUS_VECTOR));
    // Virtual wire: PIC IRQs still reach the CPU through LINT0.
    write_reg(LVT_LINT0, LINT_EXTINT);
    write_reg(DIVIDE, DIVIDE_BY_16);

    let initial = pit_ticks_per_10ms();
    TICKS_PER_10MS.store(initial, Ordering::Release);
    write_reg(LVT_TIMER, u32::from(TIMER_VECTOR) | LVT_PERIODIC);
    write_reg(INIT, initial);
}

/// AP bring-up: reuse the BSP's PIT calibration; no PIT access here.
pub fn init_ap() {
    write_reg(SVR, SVR_SW_ENABLE | u32::from(SPURIOUS_VECTOR));
    write_reg(LVT_ERROR, LVT_MASKED | u32::from(SPURIOUS_VECTOR));
    // APs have no 8259 PIC wired to LINT0.
    write_reg(LVT_LINT0, LVT_MASKED);
    write_reg(DIVIDE, DIVIDE_BY_16);

    let initial = TICKS_PER_10MS.load(Ordering::Acquire);
    write_reg(LVT_TIMER, u32::from(TIMER_VECTOR) | LVT_PERIODIC);
    write_reg(INIT, initial);
}

fn map_common() {
    let (frame, mut flags) = ApicBase::read();
    flags.remove(ApicBaseFlags::X2APIC_ENABLE);
    flags.insert(ApicBaseFlags::LAPIC_ENABLE);
    // SAFETY: keep the firmware APIC base; xAPIC MMIO only, never x2APIC MSRs.
    unsafe {
        ApicBase::write(frame, flags);
    }

    let virt = vmm::map_mmio(frame.start_address());
    MMIO.store(virt.as_mut_ptr(), Ordering::Release);
}

pub fn eoi() {
    if MMIO.load(Ordering::Relaxed).is_null() {
        return;
    }
    write_reg(EOI, 0);
}

fn pit_ticks_per_10ms() -> u32 {
    let reload = (PIT_HZ * CALIBRATE_MS / 1000) as u16;
    let mut pit_cmd: Port<u8> = Port::new(0x43);
    let mut pit_ch2: Port<u8> = Port::new(0x42);
    let mut port61: Port<u8> = Port::new(0x61);

    // SAFETY: PIT channel 2 + speaker gate are not used as a system clock.
    unsafe {
        let mut spk = port61.read();
        spk &= !0x02;
        spk &= !0x01;
        port61.write(spk);

        pit_cmd.write(0xB0);
        pit_ch2.write(reload as u8);
        pit_ch2.write((reload >> 8) as u8);

        write_reg(LVT_TIMER, u32::from(TIMER_VECTOR) | LVT_MASKED);
        write_reg(DIVIDE, DIVIDE_BY_16);
        write_reg(INIT, 0xFFFF_FFFF);

        port61.write(spk | 0x01);

        let mut spins: u64 = 0;
        while port61.read() & 0x20 == 0 {
            spins += 1;
            if spins > 1_000_000_000 {
                panic!("lapic: PIT calibrate timeout");
            }
            core::hint::spin_loop();
        }
    }

    let elapsed = 0xFFFF_FFFF - read_reg(CURRENT);
    if elapsed == 0 || elapsed == 0xFFFF_FFFF {
        panic!("lapic: PIT calibrate");
    }
    elapsed
}

fn write_reg(offset: u32, value: u32) {
    let base = MMIO.load(Ordering::Relaxed);
    debug_assert!(!base.is_null());
    // SAFETY: MMIO is the LAPIC page mapped UC for the rest of boot.
    unsafe {
        ptr::write_volatile(base.add(offset as usize).cast::<u32>(), value);
    }
}

fn read_reg(offset: u32) -> u32 {
    let base = MMIO.load(Ordering::Relaxed);
    debug_assert!(!base.is_null());
    // SAFETY: same LAPIC page as write_reg.
    unsafe { ptr::read_volatile(base.add(offset as usize).cast::<u32>()) }
}
