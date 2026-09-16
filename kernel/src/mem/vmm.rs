//! Higher-half direct map and the Limine page tables.
//!
//! ## SMP / TLB-shootdown invariant
//!
//! All CPUs share one kernel address space: Limine's L4 has its kernel half
//! (entries 256..512) pointing at shared L3/L2/L1 tables, and
//! [`new_user_l4`] copies those same kernel-half entries into every user L4.
//! That means a late mutation of the *kernel* half is instantly visible to all
//! CPUs' page tables, but a CPU that has already cached the old translation in
//! its TLB will not see it.
//!
//! Today this is safe without a shootdown because every kernel-half mapping
//! (`map_mmio`, `map_user`, `unmap_user`) happens on the BSP at boot, before
//! any AP starts, and `map_*`/`unmap_*` call `flush.flush()` on the local CPU.
//! When a mapping is first created or torn down *after* APs are running, the
//! caller must (a) propagate the change to every live user L4 — kernel-half
//! changes are automatic, user-half changes are not — and (b) shoot down the
//! stale TLB entry on the other CPUs (an IPI into a per-CPU `invlpg`).

use alloc::vec::Vec;
use core::sync::atomic::{AtomicU64, Ordering};

use spin::{Mutex, Once};
use x86_64::registers::control::{Cr3, Cr3Flags};
use x86_64::structures::paging::mapper::{MapToError, TranslateResult};
use x86_64::structures::paging::{
    FrameAllocator, Mapper, OffsetPageTable, Page, PageTable, PageTableFlags, PhysFrame, Size4KiB,
    Translate,
};
use x86_64::{PhysAddr, VirtAddr};

use crate::pmm;

static HHDM: AtomicU64 = AtomicU64::new(0);
static MAPPER: Once<Mutex<OffsetPageTable<'static>>> = Once::new();
static KERNEL_CR3: Mutex<Option<(PhysFrame<Size4KiB>, Cr3Flags)>> = Mutex::new(None);

struct PmmFrames;

unsafe impl FrameAllocator<Size4KiB> for PmmFrames {
    fn allocate_frame(&mut self) -> Option<PhysFrame<Size4KiB>> {
        pmm::alloc()
    }
}

struct RecAlloc {
    extra: Vec<PhysFrame<Size4KiB>>,
}

unsafe impl FrameAllocator<Size4KiB> for RecAlloc {
    fn allocate_frame(&mut self) -> Option<PhysFrame<Size4KiB>> {
        let f = pmm::alloc()?;
        self.extra.push(f);
        Some(f)
    }
}

/// Limine already mapped all physical memory at `phys + offset`.
pub fn init(hhdm_offset: u64) {
    HHDM.store(hhdm_offset, Ordering::Relaxed);
    let (frame, flags) = Cr3::read();
    *KERNEL_CR3.lock() = Some((frame, flags));
    let l4_virt = VirtAddr::new(frame.start_address().as_u64() + hhdm_offset);
    // SAFETY: Limine's L4 table lives in the HHDM for the rest of boot.
    let l4 = unsafe { &mut *l4_virt.as_mut_ptr::<PageTable>() };
    let mapper = unsafe { OffsetPageTable::new(l4, VirtAddr::new(hhdm_offset)) };
    MAPPER.call_once(|| Mutex::new(mapper));
}

pub fn phys_to_virt(phys: PhysAddr) -> VirtAddr {
    VirtAddr::new(phys.as_u64() + HHDM.load(Ordering::Relaxed))
}

pub fn hhdm_offset() -> u64 {
    HHDM.load(Ordering::Relaxed)
}

pub fn virt_to_phys(virt: VirtAddr) -> PhysAddr {
    let mapper = MAPPER.get().expect("vmm not initialized").lock();
    mapper
        .translate_addr(virt)
        .expect("vmm: virt_to_phys unmapped")
}

/// Map a 4 KiB MMIO frame at `HHDM + phys` as uncacheable.
pub fn map_mmio(phys: PhysAddr) -> VirtAddr {
    map_mmio_range(phys, pmm::FRAME_SIZE as usize)
}

/// Map `size` bytes of MMIO starting at `phys` (page-aligned coverage).
/// Returns the virtual address of `phys` itself, not the aligned page start.
pub fn map_mmio_range(phys: PhysAddr, size: usize) -> VirtAddr {
    try_map_mmio_range(phys, size).unwrap_or_else(|| panic!("vmm: map_mmio"))
}

/// Like [`map_mmio_range`], but `None` if a frame cannot be mapped (ACPI holes).
pub fn try_map_mmio_range(phys: PhysAddr, size: usize) -> Option<VirtAddr> {
    let virt = phys_to_virt(phys);
    if size == 0 {
        return Some(virt);
    }
    let start = phys.align_down(pmm::FRAME_SIZE).as_u64();
    let last = phys.as_u64().checked_add(size as u64 - 1)?;
    let end = PhysAddr::try_new(last)
        .ok()?
        .align_down(pmm::FRAME_SIZE)
        .as_u64();
    let mut mapper = MAPPER.get()?.lock();
    let mut addr = start;
    loop {
        if !map_one(&mut mapper, PhysAddr::new(addr)) {
            return None;
        }
        if addr >= end {
            break;
        }
        addr = addr.checked_add(pmm::FRAME_SIZE)?;
    }
    Some(virt)
}

const USER_CANON_MAX: u64 = 0x0000_8000_0000_0000;

/// New L4: user half zero, kernel half copied from Limine (shared L3+).
pub fn new_user_l4() -> Option<PhysFrame<Size4KiB>> {
    let frame = pmm::alloc()?;
    let dst = unsafe { &mut *phys_to_virt(frame.start_address()).as_mut_ptr::<PageTable>() };
    dst.zero();
    let src_frame = (*KERNEL_CR3.lock()).expect("vmm").0;
    let src = unsafe { &*phys_to_virt(src_frame.start_address()).as_ptr::<PageTable>() };
    for i in 256..512 {
        dst[i] = src[i].clone();
    }
    Some(frame)
}

pub fn load_cr3(l4: PhysFrame<Size4KiB>) {
    let flags = (*KERNEL_CR3.lock()).expect("vmm").1;
    unsafe {
        Cr3::write(l4, flags);
    }
}

pub fn restore_kernel_cr3() {
    let (frame, flags) = (*KERNEL_CR3.lock()).expect("vmm");
    unsafe {
        Cr3::write(frame, flags);
    }
}

pub fn current_l4() -> PhysFrame<Size4KiB> {
    Cr3::read().0
}

/// Map a user page into `l4`. New table frames are appended to `pt_frames`.
pub fn map_user_in(
    l4: PhysFrame<Size4KiB>,
    virt: VirtAddr,
    frame: PhysFrame<Size4KiB>,
    writable: bool,
    executable: bool,
    pt_frames: &mut Vec<PhysFrame<Size4KiB>>,
) -> Result<(), ()> {
    if virt.as_u64() >= USER_CANON_MAX || virt.as_u64() % pmm::FRAME_SIZE != 0 {
        return Err(());
    }
    let page = Page::<Size4KiB>::containing_address(virt);
    let mut flags = PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE;
    if writable {
        flags |= PageTableFlags::WRITABLE;
    }
    if !executable {
        flags |= PageTableFlags::NO_EXECUTE;
    }
    let table = unsafe { &mut *phys_to_virt(l4.start_address()).as_mut_ptr::<PageTable>() };
    let mut mapper =
        unsafe { OffsetPageTable::new(table, VirtAddr::new(HHDM.load(Ordering::Relaxed))) };
    let mut alloc = RecAlloc { extra: Vec::new() };
    let result = unsafe { mapper.map_to(page, frame, flags, &mut alloc) };
    pt_frames.append(&mut alloc.extra);
    match result {
        Ok(flush) => {
            flush.ignore();
            Ok(())
        }
        Err(_) => Err(()),
    }
}

pub fn unmap_user_in(l4: PhysFrame<Size4KiB>, virt: VirtAddr) -> Result<PhysFrame<Size4KiB>, ()> {
    let page = Page::<Size4KiB>::containing_address(virt);
    let table = unsafe { &mut *phys_to_virt(l4.start_address()).as_mut_ptr::<PageTable>() };
    let mut mapper =
        unsafe { OffsetPageTable::new(table, VirtAddr::new(HHDM.load(Ordering::Relaxed))) };
    match mapper.unmap(page) {
        Ok((frame, flush)) => {
            flush.ignore();
            Ok(frame)
        }
        Err(_) => Err(()),
    }
}

pub fn map_user(
    virt: VirtAddr,
    frame: PhysFrame<Size4KiB>,
    writable: bool,
    executable: bool,
) -> Result<(), ()> {
    if virt.as_u64() >= USER_CANON_MAX || virt.as_u64() % pmm::FRAME_SIZE != 0 {
        return Err(());
    }
    let page = Page::<Size4KiB>::containing_address(virt);
    let mut flags = PageTableFlags::PRESENT | PageTableFlags::USER_ACCESSIBLE;
    if writable {
        flags |= PageTableFlags::WRITABLE;
    }
    if !executable {
        flags |= PageTableFlags::NO_EXECUTE;
    }
    let mut mapper = MAPPER.get().expect("vmm not initialized").lock();
    match unsafe { mapper.map_to(page, frame, flags, &mut PmmFrames) } {
        Ok(flush) => {
            flush.flush();
            Ok(())
        }
        Err(_) => Err(()),
    }
}

pub fn unmap_user(virt: VirtAddr) -> Result<PhysFrame<Size4KiB>, ()> {
    let page = Page::<Size4KiB>::containing_address(virt);
    let mut mapper = MAPPER.get().expect("vmm not initialized").lock();
    match mapper.unmap(page) {
        Ok((frame, flush)) => {
            flush.flush();
            Ok(frame)
        }
        Err(_) => Err(()),
    }
}

pub fn user_slice_ok(ptr: u64, len: u64) -> bool {
    if len == 0 {
        return true;
    }
    let Some(end) = ptr.checked_add(len) else {
        return false;
    };
    let mut va = ptr & !(pmm::FRAME_SIZE - 1);
    while va < end {
        if !user_page_mapped(VirtAddr::new(va)) {
            return false;
        }
        va = match va.checked_add(pmm::FRAME_SIZE) {
            Some(n) => n,
            None => return false,
        };
    }
    true
}

fn user_page_mapped(virt: VirtAddr) -> bool {
    let (frame, _) = Cr3::read();
    let table = unsafe { &mut *phys_to_virt(frame.start_address()).as_mut_ptr::<PageTable>() };
    let mapper =
        unsafe { OffsetPageTable::new(table, VirtAddr::new(HHDM.load(Ordering::Relaxed))) };
    match mapper.translate(virt) {
        TranslateResult::Mapped { flags, .. } => flags.contains(PageTableFlags::USER_ACCESSIBLE),
        _ => false,
    }
}

fn map_one(mapper: &mut OffsetPageTable<'static>, phys: PhysAddr) -> bool {
    let phys = phys.align_down(pmm::FRAME_SIZE);
    let virt = phys_to_virt(phys);
    let page = Page::<Size4KiB>::containing_address(virt);
    match mapper.translate(virt) {
        TranslateResult::Mapped { .. } => return true,
        TranslateResult::InvalidFrameAddress(_) => return false,
        TranslateResult::NotMapped => {}
    }
    let frame = PhysFrame::containing_address(phys);
    let flags = PageTableFlags::PRESENT
        | PageTableFlags::WRITABLE
        | PageTableFlags::NO_CACHE
        | PageTableFlags::NO_EXECUTE;
    // SAFETY: `phys` is MMIO (LAPIC / virtio BAR / ACPI tables), not kernel heap.
    match unsafe { mapper.map_to(page, frame, flags, &mut PmmFrames) } {
        Ok(flush) => {
            flush.flush();
            true
        }
        Err(MapToError::PageAlreadyMapped(_)) => true,
        Err(_) => false,
    }
}
