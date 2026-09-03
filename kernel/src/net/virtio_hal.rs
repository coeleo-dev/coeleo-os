//! VirtIO DMA / MMIO HAL. The only new TCB surface for virtio.

use core::ptr::NonNull;

use virtio_drivers::{BufferDirection, Hal, PhysAddr as VirtioPhys};
use x86_64::structures::paging::{PhysFrame, Size4KiB};
use x86_64::{PhysAddr, VirtAddr};

use crate::{pmm, vmm};

pub struct VirtioHal;

unsafe impl Hal for VirtioHal {
    fn dma_alloc(pages: usize, _direction: BufferDirection) -> (VirtioPhys, NonNull<u8>) {
        let n = pages.max(1) as u64;
        let frame = pmm::alloc_contiguous(n).expect("virtio dma");
        let virt = vmm::phys_to_virt(frame.start_address());
        let ptr = virt.as_mut_ptr::<u8>();
        let len = n as usize * pmm::FRAME_SIZE as usize;
        // SAFETY: these frames were just allocated and are mapped in the HHDM.
        unsafe {
            core::ptr::write_bytes(ptr, 0, len);
        }
        (frame.start_address().as_u64(), NonNull::new(ptr).unwrap())
    }

    unsafe fn dma_dealloc(paddr: VirtioPhys, _vaddr: NonNull<u8>, pages: usize) -> i32 {
        let frame = PhysFrame::<Size4KiB>::from_start_address(PhysAddr::new(paddr))
            .expect("virtio dma frame");
        pmm::free_contiguous(frame, pages.max(1) as u64);
        0
    }

    unsafe fn mmio_phys_to_virt(paddr: VirtioPhys, size: usize) -> NonNull<u8> {
        let virt = vmm::map_mmio_range(PhysAddr::new(paddr), size);
        NonNull::new(virt.as_mut_ptr::<u8>()).expect("virtio mmio")
    }

    unsafe fn share(buffer: NonNull<[u8]>, _direction: BufferDirection) -> VirtioPhys {
        let virt = VirtAddr::from_ptr(buffer.cast::<u8>().as_ptr());
        vmm::virt_to_phys(virt).as_u64()
    }

    unsafe fn unshare(_paddr: VirtioPhys, _buffer: NonNull<[u8]>, _direction: BufferDirection) {}
}
