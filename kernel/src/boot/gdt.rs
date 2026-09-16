//! Kernel GDT + TSS, per CPU. IST1 for #DF; RSP0 for IRQs from Ring 3.

use alloc::boxed::Box;
use core::ptr;

use x86_64::VirtAddr;
use x86_64::instructions::segmentation::{CS, DS, ES, SS, Segment};
use x86_64::instructions::tables::load_tss;
use x86_64::structures::gdt::{
    Descriptor, DescriptorFlags, GlobalDescriptorTable, SegmentSelector,
};
use x86_64::structures::tss::TaskStateSegment;

use crate::percpu::MAX_CPU;

pub const DOUBLE_FAULT_IST_INDEX: u16 = 0;

const IST_SIZE: usize = 16 * 1024;
pub const USER_KERNEL_STACK_SIZE: usize = 128 * 1024;

#[repr(align(16))]
#[derive(Clone, Copy)]
struct IstStack([u8; IST_SIZE]);

#[repr(align(16))]
#[derive(Clone, Copy)]
struct KernelStack([u8; USER_KERNEL_STACK_SIZE]);

// `static mut` so rustc puts these in .bss (writable). A plain `static` lands
// in .rodata; the first `push` on RSP0 then #PFs and triple-faults.
static mut IST_STACKS: [IstStack; MAX_CPU] = [IstStack([0; IST_SIZE]); MAX_CPU];
static mut USER_KERNEL_STACKS: [KernelStack; MAX_CPU] =
    [KernelStack([0; USER_KERNEL_STACK_SIZE]); MAX_CPU];
static mut TSS_PTRS: [*mut TaskStateSegment; MAX_CPU] = [ptr::null_mut(); MAX_CPU];

#[derive(Clone, Copy)]
struct Selectors {
    kernel_code: SegmentSelector,
    kernel_data: SegmentSelector,
    user_data: SegmentSelector,
    user_code: SegmentSelector,
    tss: SegmentSelector,
}

static mut SELS: [Option<Selectors>; MAX_CPU] = [None; MAX_CPU];

pub fn user_kernel_stack_top(cpu: usize) -> VirtAddr {
    let end = unsafe {
        (&raw const USER_KERNEL_STACKS[cpu].0)
            .cast::<u8>()
            .add(USER_KERNEL_STACK_SIZE)
    };
    VirtAddr::from_ptr(end)
}

/// Switch RSP0 to this process's kernel stack. TCB: TSS is otherwise immutable.
pub fn set_user_kernel_stack(top: VirtAddr) {
    let p = unsafe { TSS_PTRS[crate::percpu::cpu_id()] };
    debug_assert!(!p.is_null());
    unsafe {
        (*p).privilege_stack_table[0] = top;
    }
}

pub fn kernel_code() -> SegmentSelector {
    unsafe { SELS[crate::percpu::cpu_id()] }.expect("gdt").kernel_code
}

pub fn kernel_data() -> SegmentSelector {
    unsafe { SELS[crate::percpu::cpu_id()] }.expect("gdt").kernel_data
}

pub fn user_code() -> SegmentSelector {
    unsafe { SELS[crate::percpu::cpu_id()] }.expect("gdt").user_code
}

pub fn user_data() -> SegmentSelector {
    unsafe { SELS[crate::percpu::cpu_id()] }.expect("gdt").user_data
}

pub fn init_bsp() {
    init_cpu(0);
}

/// Build this CPU's TSS + GDT and load them (`lgdt` + `ltr` + segment regs).
pub fn init_cpu(cpu: usize) {
    let ist_end = unsafe { (&raw const IST_STACKS[cpu].0).cast::<u8>().add(IST_SIZE) };

    // The TSS must live at a stable address (the GDT descriptor points at it).
    let tss: &'static mut TaskStateSegment = Box::leak(Box::new(TaskStateSegment::new()));
    tss.interrupt_stack_table[DOUBLE_FAULT_IST_INDEX as usize] = VirtAddr::from_ptr(ist_end);
    tss.privilege_stack_table[0] = user_kernel_stack_top(cpu);
    unsafe {
        TSS_PTRS[cpu] = tss;
    }

    let gdt: &'static mut GlobalDescriptorTable = Box::leak(Box::new(GlobalDescriptorTable::new()));
    let kernel_code = gdt.append(Descriptor::kernel_code_segment());
    let kernel_data = gdt.append(Descriptor::kernel_data_segment());
    let _dummy = gdt.append(Descriptor::UserSegment(DescriptorFlags::USER_CODE32.bits()));
    let user_data = gdt.append(Descriptor::user_data_segment());
    let user_code = gdt.append(Descriptor::user_code_segment());
    let tss_sel = gdt.append(Descriptor::tss_segment(tss));
    let sel = Selectors {
        kernel_code,
        kernel_data,
        user_data,
        user_code,
        tss: tss_sel,
    };
    unsafe {
        SELS[cpu] = Some(sel);
    }

    gdt.load();

    unsafe {
        CS::set_reg(sel.kernel_code);
        SS::set_reg(sel.kernel_data);
        DS::set_reg(sel.kernel_data);
        ES::set_reg(sel.kernel_data);
        load_tss(sel.tss);
    }
}
