//! ACPI FADT walk, reboot, and S5 poweroff. No boot-time serial.

use core::sync::atomic::{AtomicU8, Ordering};

use spin::Once;
use x86_64::PhysAddr;
use x86_64::instructions::port::Port;

use crate::vmm;

const RSDP_SIG: &[u8; 8] = b"RSD PTR ";
const FACP: &[u8; 4] = b"FACP";
const SDT_HDR: usize = 36;
const TABLE_CAP: usize = 1024 * 1024;
const RESET_REG_SUP: u32 = 1 << 10;
const SCI_EN: u16 = 1;
const SLP_EN: u16 = 1 << 13;
const SLP_TYP_SHIFT: u16 = 10;
const GAS_IO: u8 = 1;
const GAS_MEM: u8 = 0;
const NAME_OP: u8 = 0x08;
const DUAL_NAME: u8 = 0x2E;
const MULTI_NAME: u8 = 0x2F;
const ROOT_CHAR: u8 = 0x5C;
const PACKAGE_OP: u8 = 0x12;
const I8042_STATUS: u16 = 0x64;
const I8042_PULSE: u8 = 0xFE;
const I8042_IBF: u8 = 1 << 1;

static PM: Once<AcpiPm> = Once::new();
static CENTURY: AtomicU8 = AtomicU8::new(0);

#[derive(Clone, Copy)]
struct GasReg {
    mem: bool,
    addr: u64,
}

#[derive(Clone, Copy)]
struct AcpiPm {
    reset: Option<(GasReg, u8)>,
    pm1a: Option<GasReg>,
    pm1b: Option<GasReg>,
    smi_cmd: u32,
    acpi_enable: u8,
    slp_typa: u8,
    slp_typb: Option<u8>,
    have_s5: bool,
}

pub fn init(rsdp_addr: Option<usize>) {
    let Some(addr) = rsdp_addr else {
        return;
    };
    let Some(pm) = parse(addr) else {
        return;
    };
    if let Some(c) = century_from_parse() {
        crate::rtc::set_century_reg(c);
    }
    PM.call_once(|| pm);
}

fn century_from_parse() -> Option<u8> {
    let c = CENTURY.load(Ordering::Relaxed);
    if c == 0 {
        None
    } else {
        Some(c)
    }
}

pub fn reboot() {
    x86_64::instructions::interrupts::disable();
    if let Some(pm) = PM.get() {
        if let Some((reg, val)) = pm.reset {
            write_u8(reg, val);
            stall();
        }
    }
    i8042_reset();
    stall();
}

pub fn poweroff() -> bool {
    let Some(pm) = PM.get() else {
        return false;
    };
    if !pm.have_s5 {
        return false;
    }
    let Some(pm1a) = pm.pm1a else {
        return false;
    };
    x86_64::instructions::interrupts::disable();
    enable_acpi(pm, pm1a);
    write_sleep(pm1a, pm.slp_typa);
    if let (Some(pm1b), Some(typb)) = (pm.pm1b, pm.slp_typb) {
        write_sleep(pm1b, typb);
    }
    stall();
    false
}

pub fn sys_reboot() -> u64 {
    let _ = crate::fd::sys_sync();
    reboot();
    u64::MAX
}

pub fn sys_poweroff() -> u64 {
    let _ = crate::fd::sys_sync();
    let _ = poweroff();
    u64::MAX
}

fn parse(addr: usize) -> Option<AcpiPm> {
    let rsdp = rsdp_bytes(addr)?;
    if rsdp.len() < 20 || &rsdp[..8] != RSDP_SIG {
        return None;
    }
    if checksum(&rsdp[..20]) != 0 {
        return None;
    }
    let rev = rsdp[15];
    let xsdt = if rev >= 2 && rsdp.len() >= 36 {
        if checksum(&rsdp[..36]) != 0 {
            return None;
        }
        u64_at(rsdp, 24)
    } else {
        0
    };
    let facp = if xsdt != 0 {
        find_sig(xsdt, true, FACP)
    } else {
        find_sig(u32_at(rsdp, 16) as u64, false, FACP)
    }?;
    fadt_pm(facp)
}

fn rsdp_bytes(addr: usize) -> Option<&'static [u8]> {
    let phys = rsdp_phys(addr);
    let head = map_bytes(phys, 36)?;
    if &head[..8] != RSDP_SIG {
        return None;
    }
    let n = if head[15] >= 2 {
        let len = u32_at(head, 20) as usize;
        if (20..=36).contains(&len) {
            len
        } else {
            36
        }
    } else {
        20
    };
    Some(&head[..n])
}

fn rsdp_phys(addr: usize) -> u64 {
    let a = addr as u64;
    let hhdm = vmm::hhdm_offset();
    if a >= hhdm {
        a - hhdm
    } else {
        a
    }
}

fn map_bytes(phys: u64, len: usize) -> Option<&'static [u8]> {
    if phys == 0 || !(1..=TABLE_CAP).contains(&len) {
        return None;
    }
    let pa = PhysAddr::try_new(phys).ok()?;
    let v = vmm::try_map_mmio_range(pa, len)?;
    Some(unsafe { core::slice::from_raw_parts(v.as_ptr(), len) })
}

fn find_sig(root: u64, xsdt: bool, sig: &[u8; 4]) -> Option<&'static [u8]> {
    let tab = sdt(root)?;
    let ptr_sz = if xsdt { 8 } else { 4 };
    if tab.len() < SDT_HDR {
        return None;
    }
    let mut off = SDT_HDR;
    while off + ptr_sz <= tab.len() {
        let p = if xsdt {
            u64_at(tab, off)
        } else {
            u32_at(tab, off) as u64
        };
        off += ptr_sz;
        let Some(entry) = sdt(p) else {
            continue;
        };
        if entry.len() >= 4 && &entry[..4] == sig {
            return Some(entry);
        }
    }
    None
}

fn sdt(phys: u64) -> Option<&'static [u8]> {
    let hdr = map_bytes(phys, SDT_HDR)?;
    let len = u32_at(hdr, 4) as usize;
    if len < SDT_HDR || len > TABLE_CAP {
        return None;
    }
    let all = map_bytes(phys, len)?;
    if checksum(all) != 0 {
        return None;
    }
    Some(all)
}

fn fadt_pm(fadt: &[u8]) -> Option<AcpiPm> {
    if fadt.len() < 88 {
        return None;
    }
    if fadt.len() >= 109 {
        CENTURY.store(fadt[108], Ordering::Relaxed);
    }
    let flags = if fadt.len() >= 116 {
        u32_at(fadt, 112)
    } else {
        0
    };
    let reset = if flags & RESET_REG_SUP != 0 && fadt.len() >= 129 {
        gas_at(fadt, 116).map(|g| (g, fadt[128]))
    } else {
        None
    };
    let mut pm1a = io32(fadt, 64);
    let mut pm1b = io32(fadt, 68);
    if pm1a.is_none() && fadt.len() >= 184 {
        pm1a = gas_at(fadt, 172);
        pm1b = gas_at(fadt, 184);
    }
    let smi_cmd = u32_at(fadt, 48);
    let acpi_enable = fadt[52];
    let dsdt = dsdt_phys(fadt)?;
    let (slp_typa, slp_typb, have_s5) = match sdt(dsdt).and_then(parse_s5) {
        Some((a, b)) => (a & 7, b.map(|x| x & 7), true),
        None => (0, None, false),
    };
    Some(AcpiPm {
        reset,
        pm1a,
        pm1b,
        smi_cmd,
        acpi_enable,
        slp_typa,
        slp_typb,
        have_s5,
    })
}

fn dsdt_phys(fadt: &[u8]) -> Option<u64> {
    if fadt.len() >= 148 {
        let x = u64_at(fadt, 140);
        if x != 0 {
            return Some(x);
        }
    }
    let d = u32_at(fadt, 40) as u64;
    if d == 0 {
        None
    } else {
        Some(d)
    }
}

fn io32(fadt: &[u8], off: usize) -> Option<GasReg> {
    if fadt.len() < off + 4 {
        return None;
    }
    let a = u32_at(fadt, off) as u64;
    if a == 0 {
        None
    } else {
        Some(GasReg {
            mem: false,
            addr: a,
        })
    }
}

fn gas_at(tab: &[u8], off: usize) -> Option<GasReg> {
    if tab.len() < off + 12 {
        return None;
    }
    let space = tab[off];
    let addr = u64_at(tab, off + 4);
    if addr == 0 {
        return None;
    }
    match space {
        GAS_IO => Some(GasReg {
            mem: false,
            addr,
        }),
        GAS_MEM => Some(GasReg { mem: true, addr }),
        _ => None,
    }
}

fn parse_s5(dsdt: &[u8]) -> Option<(u8, Option<u8>)> {
    if dsdt.len() <= SDT_HDR + 4 {
        return None;
    }
    let body = &dsdt[SDT_HDR..];
    let mut i = 0;
    while i + 4 <= body.len() {
        if &body[i..i + 4] == b"_S5_" {
            let prev_ok = i == 0 || matches!(body[i - 1], NAME_OP | DUAL_NAME | MULTI_NAME | ROOT_CHAR);
            if prev_ok {
                if let Some(r) = parse_s5_pkg(&body[i + 4..]) {
                    return Some(r);
                }
            }
        }
        i += 1;
    }
    None
}

fn parse_s5_pkg(rest: &[u8]) -> Option<(u8, Option<u8>)> {
    let mut j = 0;
    while j < rest.len().min(16) {
        if rest[j] == PACKAGE_OP {
            return parse_pkg_ints(&rest[j..]);
        }
        j += 1;
    }
    None
}

fn pkg_length(data: &[u8]) -> Option<usize> {
    let b0 = *data.first()?;
    let extra = (b0 >> 6) as usize;
    if extra == 0 {
        return Some(1);
    }
    if data.len() < 1 + extra {
        return None;
    }
    Some(1 + extra)
}

fn parse_pkg_ints(data: &[u8]) -> Option<(u8, Option<u8>)> {
    if data.first() != Some(&PACKAGE_OP) {
        return None;
    }
    let after = pkg_length(&data[1..])?;
    let mut off = 1 + after;
    if off >= data.len() {
        return None;
    }
    let n_elem = data[off];
    off += 1;
    let a = parse_int(data, &mut off)?;
    let b = if n_elem >= 2 {
        parse_int(data, &mut off)
    } else {
        None
    };
    Some((a, b))
}

fn parse_int(data: &[u8], off: &mut usize) -> Option<u8> {
    if *off >= data.len() {
        return None;
    }
    match data[*off] {
        0x00 => {
            *off += 1;
            Some(0)
        }
        0x01 => {
            *off += 1;
            Some(1)
        }
        0x0A => {
            *off += 1;
            let v = *data.get(*off)?;
            *off += 1;
            Some(v)
        }
        0x0B => {
            *off += 1;
            let v = *data.get(*off)?;
            *off += 2;
            Some(v)
        }
        0x0C => {
            *off += 1;
            let v = *data.get(*off)?;
            *off += 4;
            Some(v)
        }
        _ => None,
    }
}

fn enable_acpi(pm: &AcpiPm, pm1a: GasReg) {
    if read_u16(pm1a) & SCI_EN != 0 {
        return;
    }
    if pm.smi_cmd == 0 || pm.acpi_enable == 0 {
        return;
    }
    unsafe {
        Port::<u8>::new(pm.smi_cmd as u16).write(pm.acpi_enable);
    }
    for _ in 0..100_000 {
        if read_u16(pm1a) & SCI_EN != 0 {
            return;
        }
        core::hint::spin_loop();
    }
}

fn write_sleep(reg: GasReg, typ: u8) {
    let mut v = read_u16(reg);
    v &= !((0b111 << SLP_TYP_SHIFT) | SLP_EN);
    v |= (u16::from(typ) << SLP_TYP_SHIFT) | SLP_EN;
    write_u16(reg, v);
}

fn map_reg(reg: GasReg, n: usize) -> Option<PhysAddr> {
    if !reg.mem {
        return PhysAddr::try_new(reg.addr).ok();
    }
    let pa = PhysAddr::try_new(reg.addr).ok()?;
    vmm::try_map_mmio_range(pa, n)?;
    Some(pa)
}

fn read_u16(reg: GasReg) -> u16 {
    if reg.mem {
        let Some(_) = map_reg(reg, 2) else {
            return 0;
        };
        let p = vmm::phys_to_virt(PhysAddr::new(reg.addr)).as_ptr::<u16>();
        unsafe { core::ptr::read_volatile(p) }
    } else {
        unsafe { Port::<u16>::new(reg.addr as u16).read() }
    }
}

fn write_u16(reg: GasReg, val: u16) {
    if reg.mem {
        if map_reg(reg, 2).is_none() {
            return;
        }
        let p = vmm::phys_to_virt(PhysAddr::new(reg.addr)).as_mut_ptr::<u16>();
        unsafe { core::ptr::write_volatile(p, val) };
    } else {
        unsafe { Port::<u16>::new(reg.addr as u16).write(val) };
    }
}

fn write_u8(reg: GasReg, val: u8) {
    if reg.mem {
        if map_reg(reg, 1).is_none() {
            return;
        }
        let p = vmm::phys_to_virt(PhysAddr::new(reg.addr)).as_mut_ptr::<u8>();
        unsafe { core::ptr::write_volatile(p, val) };
    } else {
        unsafe { Port::<u8>::new(reg.addr as u16).write(val) };
    }
}

fn i8042_reset() {
    for _ in 0..3 {
        for _ in 0..100_000 {
            let st = unsafe { Port::<u8>::new(I8042_STATUS).read() };
            if st & I8042_IBF == 0 {
                break;
            }
            core::hint::spin_loop();
        }
        unsafe {
            Port::<u8>::new(I8042_STATUS).write(I8042_PULSE);
        }
        stall();
    }
}

fn stall() {
    for _ in 0..400_000 {
        core::hint::spin_loop();
    }
}

fn checksum(b: &[u8]) -> u8 {
    b.iter().fold(0u8, |a, x| a.wrapping_add(*x))
}

fn u32_at(b: &[u8], off: usize) -> u32 {
    let mut a = [0u8; 4];
    a.copy_from_slice(&b[off..off + 4]);
    u32::from_le_bytes(a)
}

fn u64_at(b: &[u8], off: usize) -> u64 {
    let mut a = [0u8; 8];
    a.copy_from_slice(&b[off..off + 8]);
    u64::from_le_bytes(a)
}
