//! Process control block (process) and thread control block (thread).

use x86_64::structures::paging::{PhysFrame, Size4KiB};

use crate::elfload::Image;
use crate::fd::FdTable;
use crate::irqlock::IrqLock;
use crate::percpu::MAX_CPU;

pub const MAX_PROC: usize = 8;
// 16 KiB was not enough: the wallpaper PNG decode (zune-png) runs inline in
// comp::poll, deep under a read syscall, and its call chain overflows the
// stack, silently corrupting the scheduler statics just below KSTACKS[0].
pub(super) const KSTACK_SIZE: usize = 64 * 1024;
pub(super) const ERR: u64 = u64::MAX;
pub(super) const PS_REC: usize = 16;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    Runnable,
    Running,
    BlockedStdin,
    BlockedWait,
    BlockedPipeRead,
    BlockedPipeWrite,
    Zombie,
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct KernelCont {
    pub(super) rbx: u64,
    pub(super) rbp: u64,
    pub(super) r12: u64,
    pub(super) r13: u64,
    pub(super) r14: u64,
    pub(super) r15: u64,
    pub(super) rsp: u64,
    pub(super) rflags: u64,
    pub(super) rip: u64,
}

impl KernelCont {
    pub(crate) const fn zero() -> Self {
        KernelCont {
            rbx: 0,
            rbp: 0,
            r12: 0,
            r13: 0,
            r14: 0,
            r15: 0,
            rsp: 0,
            rflags: 0,
            rip: 0,
        }
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct TrapFrame {
    pub(super) rax: u64,
    pub(super) rbx: u64,
    pub(super) rcx: u64,
    pub(super) rdx: u64,
    pub(super) rsi: u64,
    pub(super) rdi: u64,
    pub(super) rbp: u64,
    pub(super) r8: u64,
    pub(super) r9: u64,
    pub(super) r10: u64,
    pub(super) r11: u64,
    pub(super) r12: u64,
    pub(super) r13: u64,
    pub(super) r14: u64,
    pub(super) r15: u64,
    pub(super) rip: u64,
    pub(super) cs: u64,
    pub(super) rflags: u64,
    pub(super) rsp: u64,
    pub(super) ss: u64,
}

impl TrapFrame {
    pub(crate) const fn zero() -> Self {
        TrapFrame {
            rax: 0,
            rbx: 0,
            rcx: 0,
            rdx: 0,
            rsi: 0,
            rdi: 0,
            rbp: 0,
            r8: 0,
            r9: 0,
            r10: 0,
            r11: 0,
            r12: 0,
            r13: 0,
            r14: 0,
            r15: 0,
            rip: 0,
            cs: 0,
            rflags: 0,
            rsp: 0,
            ss: 0,
        }
    }
}

#[repr(C)]
pub(super) struct IrqFrame {
    pub(super) rax: u64,
    pub(super) rbx: u64,
    pub(super) rcx: u64,
    pub(super) rdx: u64,
    pub(super) rsi: u64,
    pub(super) rdi: u64,
    pub(super) rbp: u64,
    pub(super) r8: u64,
    pub(super) r9: u64,
    pub(super) r10: u64,
    pub(super) r11: u64,
    pub(super) r12: u64,
    pub(super) r13: u64,
    pub(super) r14: u64,
    pub(super) r15: u64,
    pub(super) rip: u64,
    pub(super) cs: u64,
    pub(super) rflags: u64,
    pub(super) rsp: u64,
    pub(super) ss: u64,
}

#[repr(C, align(16))]
#[derive(Clone, Copy)]
pub(crate) struct FxBuf(pub(super) [u8; 512]);

impl FxBuf {
    pub(crate) const fn zero() -> Self {
        FxBuf([0; 512])
    }
}

#[repr(align(16))]
#[derive(Clone, Copy)]
pub(super) struct KernelStack(pub(super) [u8; KSTACK_SIZE]);

/// Thread: the scheduler's unit of execution. A process has one or more; each
/// thread owns its own `TrapFrame`, FPU image, kernel stack and `user_rsp`, and
/// shares the process's address space, FDs and image.
pub(super) struct Thread {
    pub(super) tid: u32,
    pub(super) proc: usize,
    pub(super) state: State,
    pub(super) trap: TrapFrame,
    pub(super) kcont_valid: bool,
    pub(super) user_rsp: u64,
    pub(super) tls: u64,
}

/// Process: shared address space (`l4`), FDs and ELF image, plus metadata.
pub(super) struct Pcb {
    pub(super) pid: u32,
    pub(super) parent: u32,
    pub(super) name: [u8; 12],
    pub(super) l4: PhysFrame<Size4KiB>,
    pub(super) fds: FdTable,
    pub(super) image: Option<Image>,
    pub(super) spawned_child: bool,
    pub(super) last_spawned: u32,
    pub(super) fault: bool,
    pub(super) zombie: bool,
    pub(super) live_threads: usize,
}

pub(super) struct Sched {
    pub(super) procs: [Option<Pcb>; MAX_PROC],
    pub(super) threads: [Option<Thread>; MAX_PROC],
    pub(super) current: [Option<usize>; MAX_CPU],
    pub(super) next_pid: u32,
    pub(super) next_tid: u32,
    pub(super) exclusive: bool,
    pub(super) init_pid: u32,
}

pub(super) static SCHED: IrqLock<Sched> = IrqLock::new(Sched {
    procs: [const { None }; MAX_PROC],
    threads: [const { None }; MAX_PROC],
    current: [None; MAX_CPU],
    next_pid: 1,
    next_tid: 1,
    exclusive: false,
    init_pid: 0,
});
