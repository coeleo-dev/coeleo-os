//! Process control block and run queue.

use spin::Mutex;
use x86_64::structures::paging::{PhysFrame, Size4KiB};

use crate::elfload::Image;
use crate::fd::FdTable;

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
pub(super) struct KernelCont {
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

#[repr(C)]
#[derive(Clone, Copy)]
pub(super) struct TrapFrame {
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
pub(super) struct FxBuf(pub(super) [u8; 512]);

#[repr(align(16))]
#[derive(Clone, Copy)]
pub(super) struct KernelStack(pub(super) [u8; KSTACK_SIZE]);

pub(super) struct Pcb {
    pub(super) pid: u32,
    pub(super) parent: u32,
    pub(super) state: State,
    pub(super) name: [u8; 12],
    pub(super) l4: PhysFrame<Size4KiB>,
    pub(super) trap: TrapFrame,
    pub(super) kcont_valid: bool,
    pub(super) user_rsp: u64,
    pub(super) fds: FdTable,
    pub(super) image: Option<Image>,
    pub(super) spawned_child: bool,
    pub(super) last_spawned: u32,
    pub(super) fault: bool,
}

pub(super) struct Sched {
    pub(super) procs: [Option<Pcb>; MAX_PROC],
    pub(super) current: Option<usize>,
    pub(super) next_pid: u32,
    pub(super) exclusive: bool,
    pub(super) init_pid: u32,
}

pub(super) static SCHED: Mutex<Sched> = Mutex::new(Sched {
    procs: [const { None }; MAX_PROC],
    current: None,
    next_pid: 1,
    exclusive: false,
    init_pid: 0,
});
