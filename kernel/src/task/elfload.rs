//! Load a static ET_EXEC into a per-process L4. No Relink.

use alloc::vec::Vec;

use elf::ElfBytes;
use elf::abi::{EM_X86_64, ET_EXEC, PF_W, PF_X, PT_DYNAMIC, PT_LOAD};
use elf::endian::LittleEndian;
use x86_64::VirtAddr;
use x86_64::structures::paging::{PhysFrame, Size4KiB};

use crate::pmm;
use crate::vmm;

pub const USER_STACK_BASE: u64 = 0x7FFF_0000;
pub const USER_STACK_PAGES: u64 = 16;
pub const USER_STACK_TOP: u64 = USER_STACK_BASE + USER_STACK_PAGES * pmm::FRAME_SIZE;

pub struct Image {
    pub entry: VirtAddr,
    pub l4: PhysFrame<Size4KiB>,
    pages: Vec<(VirtAddr, PhysFrame<Size4KiB>)>,
    pt_frames: Vec<PhysFrame<Size4KiB>>,
}

pub fn load(bytes: &[u8]) -> Result<Image, ()> {
    let l4 = vmm::new_user_l4().ok_or(())?;
    let file = ElfBytes::<LittleEndian>::minimal_parse(bytes).map_err(|_| ())?;
    if file.ehdr.e_machine != EM_X86_64 || file.ehdr.e_type != ET_EXEC {
        pmm::free(l4);
        return Err(());
    }
    let phdrs = file.segments().ok_or(())?;
    let mut pages = Vec::new();
    let mut pt_frames = Vec::new();
    let mut ok = false;
    for ph in phdrs.iter() {
        if ph.p_type == PT_DYNAMIC {
            fail(l4, &pages, &pt_frames);
            return Err(());
        }
        if ph.p_type != PT_LOAD {
            continue;
        }
        ok = true;
        if let Err(()) = map_segment(bytes, &ph, l4, &mut pages, &mut pt_frames) {
            fail(l4, &pages, &pt_frames);
            return Err(());
        }
    }
    if !ok {
        fail(l4, &pages, &pt_frames);
        return Err(());
    }
    if map_stack(l4, &mut pages, &mut pt_frames).is_err() {
        fail(l4, &pages, &pt_frames);
        return Err(());
    }
    Ok(Image {
        entry: VirtAddr::new(file.ehdr.e_entry),
        l4,
        pages,
        pt_frames,
    })
}

pub fn unload(image: Image) {
    if vmm::current_l4() == image.l4 {
        vmm::restore_kernel_cr3();
    }
    fail(image.l4, &image.pages, &image.pt_frames);
}

fn fail(
    l4: PhysFrame<Size4KiB>,
    pages: &[(VirtAddr, PhysFrame<Size4KiB>)],
    pt_frames: &[PhysFrame<Size4KiB>],
) {
    for &(_, frame) in pages {
        pmm::free(frame);
    }
    for &frame in pt_frames {
        pmm::free(frame);
    }
    pmm::free(l4);
}

fn map_segment(
    bytes: &[u8],
    ph: &elf::segment::ProgramHeader,
    l4: PhysFrame<Size4KiB>,
    pages: &mut Vec<(VirtAddr, PhysFrame<Size4KiB>)>,
    pt_frames: &mut Vec<PhysFrame<Size4KiB>>,
) -> Result<(), ()> {
    if ph.p_filesz > ph.p_memsz {
        return Err(());
    }
    let writable = ph.p_flags & PF_W != 0;
    let executable = ph.p_flags & PF_X != 0;
    if writable && executable {
        return Err(());
    }
    let start = ph.p_vaddr;
    let end = start.checked_add(ph.p_memsz).ok_or(())?;
    if end > 0x0000_8000_0000_0000 {
        return Err(());
    }
    let page_start = start & !(pmm::FRAME_SIZE - 1);
    let mut va = page_start;
    while va < end {
        let frame = pmm::alloc().ok_or(())?;
        zero_frame(frame);
        let page_end = va + pmm::FRAME_SIZE;
        let copy_lo = start.max(va);
        let copy_hi = (start + ph.p_filesz).min(page_end);
        if copy_lo < copy_hi {
            let file_off = ph.p_offset + (copy_lo - start);
            let n = (copy_hi - copy_lo) as usize;
            let src_end = file_off as usize + n;
            if src_end > bytes.len() {
                pmm::free(frame);
                return Err(());
            }
            copy_into_frame(
                frame,
                (copy_lo - va) as usize,
                &bytes[file_off as usize..src_end],
            );
        }
        let virt = VirtAddr::new(va);
        if vmm::map_user_in(l4, virt, frame, writable, executable, pt_frames).is_err() {
            pmm::free(frame);
            return Err(());
        }
        pages.push((virt, frame));
        va = page_end;
    }
    Ok(())
}

fn map_stack(
    l4: PhysFrame<Size4KiB>,
    pages: &mut Vec<(VirtAddr, PhysFrame<Size4KiB>)>,
    pt_frames: &mut Vec<PhysFrame<Size4KiB>>,
) -> Result<(), ()> {
    for i in 0..USER_STACK_PAGES {
        let va = USER_STACK_BASE + i * pmm::FRAME_SIZE;
        let frame = pmm::alloc().ok_or(())?;
        zero_frame(frame);
        let virt = VirtAddr::new(va);
        if vmm::map_user_in(l4, virt, frame, true, false, pt_frames).is_err() {
            pmm::free(frame);
            return Err(());
        }
        pages.push((virt, frame));
    }
    Ok(())
}

fn zero_frame(frame: PhysFrame<Size4KiB>) {
    let virt = vmm::phys_to_virt(frame.start_address());
    unsafe {
        core::ptr::write_bytes(virt.as_mut_ptr::<u8>(), 0, pmm::FRAME_SIZE as usize);
    }
}

fn copy_into_frame(frame: PhysFrame<Size4KiB>, off: usize, src: &[u8]) {
    let virt = vmm::phys_to_virt(frame.start_address());
    unsafe {
        core::ptr::copy_nonoverlapping(src.as_ptr(), virt.as_mut_ptr::<u8>().add(off), src.len());
    }
}
