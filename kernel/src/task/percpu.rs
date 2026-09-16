//! Per-CPU state, reached through the GS base register.
//!
//! While in kernel mode GS points here (after `swapgs`); in user mode GS is
//! zero and the kernel pointer lives in the swap area (`KERNEL_GS_BASE`), so a
//! user `syscall`/IRQ swaps GS back to the kernel pointer on entry.

use x86_64::registers::model_specific::GsBase;

use super::sched::pcb::{FxBuf, KernelCont, TrapFrame};

/// Upper bound on CPUs the scheduler supports. Each CPU carries `MAX_PROC`
/// kernel stacks (64 KiB each), so this number directly drives BSS size:
/// `8 * 8 * 64 KiB = 4 MiB` for `KSTACKS`, plus ~1 MiB of TSS/IST stacks.
pub const MAX_CPU: usize = 8;

#[repr(C)]
#[derive(Clone, Copy)]
pub struct PerCpu {
    /// Syscall trampoline: kernel/user RSP saved across `syscall`/`sysretq`.
    pub kernel_rsp: u64,
    pub user_rsp: u64,
    /// Slot the scheduler is currently running on this CPU.
    pub current_slot: usize,
    /// Address of `KCONTS[cpu][slot]` for the naked save/restore trampolines.
    pub kcont_ptr: usize,
    /// Address of `FXSAVES[cpu][slot].0` for the timer IRQ FPU save/restore.
    pub fxsave_ptr: usize,
    /// Trap frame pushed for the next `iretq` (`resume_user`).
    pub iret_trap: TrapFrame,
    /// FPU image restored together with `iret_trap`.
    pub iret_fx: FxBuf,
    /// Host (kernel-mode) context saved when entering the scheduler.
    pub host_kcont: KernelCont,
    /// Exit outcome passed from `leave_scheduler` back to `run_with`.
    pub outcome: u8,
}

impl PerCpu {
    const fn zero() -> Self {
        PerCpu {
            kernel_rsp: 0,
            user_rsp: 0,
            current_slot: 0,
            kcont_ptr: 0,
            fxsave_ptr: 0,
            iret_trap: TrapFrame::zero(),
            iret_fx: FxBuf::zero(),
            host_kcont: KernelCont::zero(),
            outcome: 0,
        }
    }
}

/// One `PerCpu` per CPU. Index 0 is the BSP.
pub static mut PERCPU: [PerCpu; MAX_CPU] = [PerCpu::zero(); MAX_CPU];

/// The current CPU's per-CPU block, from the GS base.
pub fn this_cpu() -> &'static mut PerCpu {
    // SAFETY: GS base is set to `&PERCPU[cpu]` during per-CPU init and points
    // inside `PERCPU` for the rest of boot.
    unsafe { &mut *GsBase::read().as_mut_ptr::<PerCpu>() }
}

/// Dense CPU index (0 = BSP), derived from the pointer position in `PERCPU`.
pub fn cpu_id() -> usize {
    // SAFETY: `this_cpu()` always points within `PERCPU`, so `offset_from` is
    // a valid element index.
    unsafe {
        let base = (&raw const PERCPU) as *const PerCpu;
        (this_cpu() as *const PerCpu).offset_from(base) as usize
    }
}
