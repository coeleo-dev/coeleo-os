//! Kernel GDT + TSS. IST1 for #DF; RSP0 for IRQs from Ring 3.

use core::sync::atomic::{AtomicPtr, Ordering};

use spin::Once;
use x86_64::VirtAddr;
use x86_64::instructions::segmentation::{CS, DS, ES, SS, Segment};
use x86_64::instructions::tables::load_tss;
use x86_64::structures::gdt::{
    Descriptor, DescriptorFlags, GlobalDescriptorTable, SegmentSelector,
};
use x86_64::structures::tss::TaskStateSegment;

pub const DOUBLE_FAULT_IST_INDEX: u16 = 0;

const IST_SIZE: usize = 16 * 1024;
pub const USER_KERNEL_STACK_SIZE: usize = 128 * 1024;

#[repr(align(16))]
struct IstStack([u8; IST_SIZE]);

#[repr(align(16))]
struct KernelStack([u8; USER_KERNEL_STACK_SIZE]);

// `static mut` so rustc puts these in .bss (writable). A plain `static` lands in
// .rodata; the first `push` on RSP0 then #PFs and triple-faults.
static mut IST_STACK: IstStack = IstStack([0; IST_SIZE]);
static mut USER_KERNEL_STACK: KernelStack = KernelStack([0; USER_KERNEL_STACK_SIZE]);
static TSS: Once<TaskStateSegment> = Once::new();
static TSS_PTR: AtomicPtr<TaskStateSegment> = AtomicPtr::new(core::ptr::null_mut());
static GDT: Once<(GlobalDescriptorTable, Selectors)> = Once::new();

struct Selectors {
    kernel_code: SegmentSelector,
    kernel_data: SegmentSelector,
    user_data: SegmentSelector,
    user_code: SegmentSelector,
    tss: SegmentSelector,
}

pub fn user_kernel_stack_top() -> VirtAddr {
    let end = unsafe {
        (&raw const USER_KERNEL_STACK.0)
            .cast::<u8>()
            .add(USER_KERNEL_STACK_SIZE)
    };
    VirtAddr::from_ptr(end)
}

/// Switch RSP0 to this process's kernel stack. TCB: TSS is otherwise immutable.
pub fn set_user_kernel_stack(top: VirtAddr) {
    let p = TSS_PTR.load(Ordering::Acquire);
    debug_assert!(!p.is_null());
    unsafe {
        (*p).privilege_stack_table[0] = top;
    }
}

pub fn kernel_code() -> SegmentSelector {
    GDT.get().expect("gdt").1.kernel_code
}

pub fn kernel_data() -> SegmentSelector {
    GDT.get().expect("gdt").1.kernel_data
}

pub fn user_code() -> SegmentSelector {
    GDT.get().expect("gdt").1.user_code
}

pub fn user_data() -> SegmentSelector {
    GDT.get().expect("gdt").1.user_data
}

pub fn init() {
    let tss = TSS.call_once(|| {
        let mut tss = TaskStateSegment::new();
        let ist_end = unsafe { (&raw const IST_STACK.0).cast::<u8>().add(IST_SIZE) };
        tss.interrupt_stack_table[DOUBLE_FAULT_IST_INDEX as usize] = VirtAddr::from_ptr(ist_end);
        tss.privilege_stack_table[0] = user_kernel_stack_top();
        tss
    });
    TSS_PTR.store(
        TSS.get().expect("tss") as *const TaskStateSegment as *mut TaskStateSegment,
        Ordering::Release,
    );

    let (gdt, sel) = GDT.call_once(|| {
        let mut gdt = GlobalDescriptorTable::new();
        let kernel_code = gdt.append(Descriptor::kernel_code_segment());
        let kernel_data = gdt.append(Descriptor::kernel_data_segment());
        let _dummy = gdt.append(Descriptor::UserSegment(DescriptorFlags::USER_CODE32.bits()));
        let user_data = gdt.append(Descriptor::user_data_segment());
        let user_code = gdt.append(Descriptor::user_code_segment());
        let tss_sel = gdt.append(Descriptor::tss_segment(tss));
        (
            gdt,
            Selectors {
                kernel_code,
                kernel_data,
                user_data,
                user_code,
                tss: tss_sel,
            },
        )
    });

    gdt.load();
    unsafe {
        CS::set_reg(sel.kernel_code);
        SS::set_reg(sel.kernel_data);
        DS::set_reg(sel.kernel_data);
        ES::set_reg(sel.kernel_data);
        load_tss(sel.tss);
    }
}
