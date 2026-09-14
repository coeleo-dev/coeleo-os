//! Context switch, FPU, and LAPIC timer. All naked asm lives here.

use core::arch::naked_asm;
use core::sync::atomic::{AtomicU8, AtomicUsize, Ordering};

use x86_64::VirtAddr;
use x86_64::registers::control::{Cr0, Cr0Flags, Cr4, Cr4Flags};
use x86_64::registers::model_specific::{GsBase, KernelGsBase};

use crate::elfload;
use crate::process::Outcome;
use crate::syscall::CPU_LOCAL;

use super::pcb::{
    FxBuf, IrqFrame, KSTACK_SIZE, KernelCont, KernelStack, MAX_PROC, Pcb, SCHED, Sched, State,
    TrapFrame,
};

pub(super) static mut KSTACKS: [KernelStack; MAX_PROC] = [KernelStack([0; KSTACK_SIZE]); MAX_PROC];
pub(super) static mut KCONTS: [KernelCont; MAX_PROC] = [KernelCont {
    rbx: 0,
    rbp: 0,
    r12: 0,
    r13: 0,
    r14: 0,
    r15: 0,
    rsp: 0,
    rflags: 0,
    rip: 0,
}; MAX_PROC];
pub(super) static mut HOST_KCONT: KernelCont = KernelCont {
    rbx: 0,
    rbp: 0,
    r12: 0,
    r13: 0,
    r14: 0,
    r15: 0,
    rsp: 0,
    rflags: 0,
    rip: 0,
};
pub(super) static mut FXSAVES: [FxBuf; MAX_PROC] = [FxBuf([0; 512]); MAX_PROC];
pub(super) static mut FX_TEMPLATE: FxBuf = FxBuf([0; 512]);
pub(super) static mut IRET_TRAP: TrapFrame = TrapFrame {
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
};
pub(super) static mut IRET_FX: FxBuf = FxBuf([0; 512]);

pub(super) static CURRENT_SLOT: AtomicUsize = AtomicUsize::new(0);
pub(super) static KCONT_PTR: AtomicUsize = AtomicUsize::new(0);
pub(super) static FXSAVE_PTR: AtomicUsize = AtomicUsize::new(0);
pub(super) static OUTCOME: AtomicU8 = AtomicU8::new(0);

pub(super) fn kstack_top(slot: usize) -> VirtAddr {
    let end = unsafe { (&raw const KSTACKS[slot].0).cast::<u8>().add(KSTACK_SIZE) };
    VirtAddr::from_ptr(end)
}

pub(super) fn set_current_slot(slot: usize) {
    CURRENT_SLOT.store(slot, Ordering::SeqCst);
    let kptr = unsafe { &raw mut KCONTS[slot] as *mut KernelCont as usize };
    KCONT_PTR.store(kptr, Ordering::SeqCst);
    let fptr = unsafe { core::ptr::addr_of_mut!(FXSAVES[slot].0) as usize };
    FXSAVE_PTR.store(fptr, Ordering::SeqCst);
}

pub fn init() {
    unsafe {
        Cr0::update(|f| {
            f.remove(Cr0Flags::EMULATE_COPROCESSOR);
            f.insert(Cr0Flags::MONITOR_COPROCESSOR);
        });
        Cr4::update(|f| {
            f.insert(Cr4Flags::OSFXSR | Cr4Flags::OSXMMEXCPT_ENABLE);
        });
        core::arch::asm!(
            "fninit",
            "fxsave64 [{}]",
            in(reg) core::ptr::addr_of_mut!(FX_TEMPLATE.0),
            options(nostack),
        );
    }
}
pub(super) fn gs_enter_user() {
    unsafe {
        GsBase::write(x86_64::VirtAddr::zero());
        KernelGsBase::write(x86_64::VirtAddr::from_ptr(&raw const CPU_LOCAL));
    }
}

pub(super) fn gs_enter_kernel_syscall() {
    unsafe {
        GsBase::write(x86_64::VirtAddr::from_ptr(&raw const CPU_LOCAL));
        KernelGsBase::write(x86_64::VirtAddr::zero());
    }
}

pub(super) fn apply_hw(slot: usize) {
    let (l4, top, user_rsp) = {
        let s = SCHED.lock();
        let p = s.procs[slot].as_ref().unwrap();
        (p.l4, kstack_top(slot), p.user_rsp)
    };
    unsafe {
        CPU_LOCAL.kernel_rsp = top.as_u64();
        CPU_LOCAL.user_rsp = user_rsp;
    }
    crate::gdt::set_user_kernel_stack(top);
    crate::vmm::load_cr3(l4);
}

pub(super) unsafe fn enter_scheduler() {
    unsafe {
        if save_host() == 0 {
            let slot = SCHED.lock().current.unwrap();
            resume_user_slot(slot);
        }
    }
}

pub(super) fn leave_scheduler(outcome: Outcome) -> ! {
    unload_all();
    {
        let mut s = SCHED.lock();
        s.current = None;
        s.exclusive = false;
        s.init_pid = 0;
    }
    crate::vmm::restore_kernel_cr3();
    unsafe {
        CPU_LOCAL.kernel_rsp = crate::gdt::user_kernel_stack_top().as_u64();
        crate::gdt::set_user_kernel_stack(crate::gdt::user_kernel_stack_top());
        x86_64::registers::model_specific::GsBase::write(x86_64::VirtAddr::zero());
        x86_64::registers::model_specific::KernelGsBase::write(x86_64::VirtAddr::from_ptr(
            &raw const CPU_LOCAL,
        ));
        OUTCOME.store(
            match outcome {
                Outcome::Exited => 0,
                Outcome::Fault => 1,
            },
            Ordering::SeqCst,
        );
        restore_host();
    }
}

pub(super) fn unload_all() {
    let mut s = SCHED.lock();
    for slot in 0..MAX_PROC {
        if let Some(mut pcb) = s.procs[slot].take() {
            pcb.fds.close_all();
            if let Some(image) = pcb.image.take() {
                drop(s);
                elfload::unload(image);
                s = SCHED.lock();
            }
        }
    }
}

pub(super) fn drop_pcb(mut pcb: Pcb) {
    pcb.fds.close_all();
    if let Some(image) = pcb.image.take() {
        elfload::unload(image);
    }
}

pub(super) fn fxsave_current() {
    let slot = CURRENT_SLOT.load(Ordering::SeqCst);
    unsafe {
        core::arch::asm!(
            "fxsave64 [{}]",
            in(reg) core::ptr::addr_of_mut!(FXSAVES[slot].0),
            options(nostack, preserves_flags),
        );
    }
}

pub(super) fn yield_block(state: State) {
    unsafe {
        if save_kcont() == 0 {
            fxsave_current();
            {
                let mut s = SCHED.lock();
                let i = s.current.expect("yield");
                s.procs[i].as_mut().unwrap().state = state;
                s.procs[i].as_mut().unwrap().kcont_valid = true;
            }
            schedule_next();
        } else {
            let slot = CURRENT_SLOT.load(Ordering::SeqCst);
            {
                let mut s = SCHED.lock();
                if let Some(p) = s.procs[slot].as_mut() {
                    p.kcont_valid = false;
                }
            }
            core::arch::asm!(
                "fxrstor64 [{}]",
                in(reg) core::ptr::addr_of!(FXSAVES[slot].0),
                options(nostack, preserves_flags),
            );
        }
    }
}

pub fn schedule_next() -> ! {
    loop {
        let pick = {
            let s = SCHED.lock();
            pick_runnable(&s)
        };
        if let Some(slot) = pick {
            switch_to(slot);
        }
        crate::comp::poll();
        crate::clock::paint_if_second_elapsed();
        x86_64::instructions::interrupts::enable_and_hlt();
        x86_64::instructions::interrupts::disable();
    }
}

pub(super) fn pick_runnable(s: &Sched) -> Option<usize> {
    let start = s.current.map(|c| c + 1).unwrap_or(0);
    for k in 0..MAX_PROC {
        let i = (start + k) % MAX_PROC;
        let Some(p) = s.procs[i].as_ref() else {
            continue;
        };
        if p.state == State::Runnable || p.state == State::Running {
            return Some(i);
        }
    }
    None
}

pub(super) fn switch_to(slot: usize) -> ! {
    let kcont = {
        let mut s = SCHED.lock();
        if let Some(c) = s.current {
            if c != slot {
                if let Some(p) = s.procs[c].as_mut() {
                    if p.state == State::Running {
                        p.state = State::Runnable;
                    }
                    p.user_rsp = unsafe { CPU_LOCAL.user_rsp };
                }
            }
        }
        s.current = Some(slot);
        s.procs[slot].as_mut().unwrap().state = State::Running;
        s.procs[slot].as_ref().unwrap().kcont_valid
    };
    set_current_slot(slot);
    apply_hw(slot);
    if kcont {
        gs_enter_kernel_syscall();
        unsafe {
            restore_kcont();
        }
    } else {
        resume_user_slot(slot);
    }
}

pub(super) fn resume_user_slot(slot: usize) -> ! {
    let trap = SCHED.lock().procs[slot].as_ref().unwrap().trap;
    unsafe {
        IRET_TRAP = trap;
        IRET_FX = FXSAVES[slot];
        gs_enter_user();
        resume_user();
    }
}

/// Timer IRQ from Ring 3. Returns if the same process continues.
extern "C" fn timer_from_user(frame: *const IrqFrame) {
    let frame = unsafe { &*frame };
    crate::clock::tick();
    crate::lapic::eoi();
    let next = {
        let mut s = SCHED.lock();
        let Some(cur) = s.current else {
            return;
        };
        if s.procs[cur]
            .as_ref()
            .is_none_or(|p| p.state == State::Zombie)
        {
            drop(s);
            schedule_next();
        }
        let other = pick_runnable_other(&s, cur);
        let Some(n) = other else {
            return;
        };
        if let Some(p) = s.procs[cur].as_mut() {
            p.trap = trap_from_irq(frame);
            p.state = State::Runnable;
            p.kcont_valid = false;
            p.user_rsp = unsafe { CPU_LOCAL.user_rsp };
        }
        s.current = Some(n);
        s.procs[n].as_mut().unwrap().state = State::Running;
        n
    };
    set_current_slot(next);
    apply_hw(next);
    let kcont = SCHED.lock().procs[next].as_ref().unwrap().kcont_valid;
    if kcont {
        gs_enter_kernel_syscall();
        unsafe {
            restore_kcont();
        }
    } else {
        resume_user_slot(next);
    }
}

pub(super) fn pick_runnable_other(s: &Sched, cur: usize) -> Option<usize> {
    for k in 1..MAX_PROC {
        let i = (cur + k) % MAX_PROC;
        if s.procs[i]
            .as_ref()
            .is_some_and(|p| p.state == State::Runnable)
        {
            return Some(i);
        }
    }
    None
}

pub(super) fn trap_from_irq(frame: &IrqFrame) -> TrapFrame {
    TrapFrame {
        rax: frame.rax,
        rbx: frame.rbx,
        rcx: frame.rcx,
        rdx: frame.rdx,
        rsi: frame.rsi,
        rdi: frame.rdi,
        rbp: frame.rbp,
        r8: frame.r8,
        r9: frame.r9,
        r10: frame.r10,
        r11: frame.r11,
        r12: frame.r12,
        r13: frame.r13,
        r14: frame.r14,
        r15: frame.r15,
        rip: frame.rip,
        cs: frame.cs,
        rflags: frame.rflags,
        rsp: frame.rsp,
        ss: frame.ss,
    }
}

pub extern "C" fn timer_kernel() {
    crate::clock::tick();
    crate::lapic::eoi();
}

#[unsafe(naked)]
unsafe extern "C" fn save_kcont() -> u64 {
    naked_asm!(
        "mov rax, [{ptr}]",
        "mov [rax + {off_rbx}], rbx",
        "mov [rax + {off_rbp}], rbp",
        "mov [rax + {off_r12}], r12",
        "mov [rax + {off_r13}], r13",
        "mov [rax + {off_r14}], r14",
        "mov [rax + {off_r15}], r15",
        "mov rcx, [rsp]",
        "mov [rax + {off_rip}], rcx",
        "lea rcx, [rsp + 8]",
        "mov [rax + {off_rsp}], rcx",
        "pushfq",
        "pop rcx",
        "mov [rax + {off_rflags}], rcx",
        "xor eax, eax",
        "ret",
        ptr = sym KCONT_PTR,
        off_rbx = const core::mem::offset_of!(KernelCont, rbx),
        off_rbp = const core::mem::offset_of!(KernelCont, rbp),
        off_r12 = const core::mem::offset_of!(KernelCont, r12),
        off_r13 = const core::mem::offset_of!(KernelCont, r13),
        off_r14 = const core::mem::offset_of!(KernelCont, r14),
        off_r15 = const core::mem::offset_of!(KernelCont, r15),
        off_rsp = const core::mem::offset_of!(KernelCont, rsp),
        off_rflags = const core::mem::offset_of!(KernelCont, rflags),
        off_rip = const core::mem::offset_of!(KernelCont, rip),
    );
}

#[unsafe(naked)]
unsafe extern "C" fn restore_kcont() -> ! {
    naked_asm!(
        "mov rax, [{ptr}]",
        "mov rbx, [rax + {off_rbx}]",
        "mov rbp, [rax + {off_rbp}]",
        "mov r12, [rax + {off_r12}]",
        "mov r13, [rax + {off_r13}]",
        "mov r14, [rax + {off_r14}]",
        "mov r15, [rax + {off_r15}]",
        "mov rsp, [rax + {off_rsp}]",
        "push qword ptr [rax + {off_rflags}]",
        "popfq",
        "mov rcx, [rax + {off_rip}]",
        "mov eax, 1",
        "jmp rcx",
        ptr = sym KCONT_PTR,
        off_rbx = const core::mem::offset_of!(KernelCont, rbx),
        off_rbp = const core::mem::offset_of!(KernelCont, rbp),
        off_r12 = const core::mem::offset_of!(KernelCont, r12),
        off_r13 = const core::mem::offset_of!(KernelCont, r13),
        off_r14 = const core::mem::offset_of!(KernelCont, r14),
        off_r15 = const core::mem::offset_of!(KernelCont, r15),
        off_rsp = const core::mem::offset_of!(KernelCont, rsp),
        off_rflags = const core::mem::offset_of!(KernelCont, rflags),
        off_rip = const core::mem::offset_of!(KernelCont, rip),
    );
}

#[unsafe(naked)]
unsafe extern "C" fn save_host() -> u64 {
    naked_asm!(
        "mov [{k} + {off_rbx}], rbx",
        "mov [{k} + {off_rbp}], rbp",
        "mov [{k} + {off_r12}], r12",
        "mov [{k} + {off_r13}], r13",
        "mov [{k} + {off_r14}], r14",
        "mov [{k} + {off_r15}], r15",
        "mov rcx, [rsp]",
        "mov [{k} + {off_rip}], rcx",
        "lea rcx, [rsp + 8]",
        "mov [{k} + {off_rsp}], rcx",
        "pushfq",
        "pop rcx",
        "mov [{k} + {off_rflags}], rcx",
        "xor eax, eax",
        "ret",
        k = sym HOST_KCONT,
        off_rbx = const core::mem::offset_of!(KernelCont, rbx),
        off_rbp = const core::mem::offset_of!(KernelCont, rbp),
        off_r12 = const core::mem::offset_of!(KernelCont, r12),
        off_r13 = const core::mem::offset_of!(KernelCont, r13),
        off_r14 = const core::mem::offset_of!(KernelCont, r14),
        off_r15 = const core::mem::offset_of!(KernelCont, r15),
        off_rsp = const core::mem::offset_of!(KernelCont, rsp),
        off_rflags = const core::mem::offset_of!(KernelCont, rflags),
        off_rip = const core::mem::offset_of!(KernelCont, rip),
    );
}

#[unsafe(naked)]
unsafe extern "C" fn restore_host() -> ! {
    naked_asm!(
        "mov rbx, [{k} + {off_rbx}]",
        "mov rbp, [{k} + {off_rbp}]",
        "mov r12, [{k} + {off_r12}]",
        "mov r13, [{k} + {off_r13}]",
        "mov r14, [{k} + {off_r14}]",
        "mov r15, [{k} + {off_r15}]",
        "mov rsp, [{k} + {off_rsp}]",
        "push qword ptr [{k} + {off_rflags}]",
        "popfq",
        "mov rcx, [{k} + {off_rip}]",
        "mov eax, 1",
        "jmp rcx",
        k = sym HOST_KCONT,
        off_rbx = const core::mem::offset_of!(KernelCont, rbx),
        off_rbp = const core::mem::offset_of!(KernelCont, rbp),
        off_r12 = const core::mem::offset_of!(KernelCont, r12),
        off_r13 = const core::mem::offset_of!(KernelCont, r13),
        off_r14 = const core::mem::offset_of!(KernelCont, r14),
        off_r15 = const core::mem::offset_of!(KernelCont, r15),
        off_rsp = const core::mem::offset_of!(KernelCont, rsp),
        off_rflags = const core::mem::offset_of!(KernelCont, rflags),
        off_rip = const core::mem::offset_of!(KernelCont, rip),
    );
}

#[unsafe(naked)]
unsafe extern "C" fn resume_user() -> ! {
    naked_asm!(
        "fxrstor64 [{fx}]",
        "mov rax, [{t} + {off_rax}]",
        "mov rbx, [{t} + {off_rbx}]",
        "mov rcx, [{t} + {off_rcx}]",
        "mov rdx, [{t} + {off_rdx}]",
        "mov rsi, [{t} + {off_rsi}]",
        "mov rdi, [{t} + {off_rdi}]",
        "mov rbp, [{t} + {off_rbp}]",
        "mov r8, [{t} + {off_r8}]",
        "mov r9, [{t} + {off_r9}]",
        "mov r10, [{t} + {off_r10}]",
        "mov r11, [{t} + {off_r11}]",
        "mov r12, [{t} + {off_r12}]",
        "mov r13, [{t} + {off_r13}]",
        "mov r14, [{t} + {off_r14}]",
        "mov r15, [{t} + {off_r15}]",
        "push qword ptr [{t} + {off_ss}]",
        "push qword ptr [{t} + {off_rsp}]",
        "push qword ptr [{t} + {off_rflags}]",
        "push qword ptr [{t} + {off_cs}]",
        "push qword ptr [{t} + {off_rip}]",
        "iretq",
        t = sym IRET_TRAP,
        fx = sym IRET_FX,
        off_rax = const core::mem::offset_of!(TrapFrame, rax),
        off_rbx = const core::mem::offset_of!(TrapFrame, rbx),
        off_rcx = const core::mem::offset_of!(TrapFrame, rcx),
        off_rdx = const core::mem::offset_of!(TrapFrame, rdx),
        off_rsi = const core::mem::offset_of!(TrapFrame, rsi),
        off_rdi = const core::mem::offset_of!(TrapFrame, rdi),
        off_rbp = const core::mem::offset_of!(TrapFrame, rbp),
        off_r8 = const core::mem::offset_of!(TrapFrame, r8),
        off_r9 = const core::mem::offset_of!(TrapFrame, r9),
        off_r10 = const core::mem::offset_of!(TrapFrame, r10),
        off_r11 = const core::mem::offset_of!(TrapFrame, r11),
        off_r12 = const core::mem::offset_of!(TrapFrame, r12),
        off_r13 = const core::mem::offset_of!(TrapFrame, r13),
        off_r14 = const core::mem::offset_of!(TrapFrame, r14),
        off_r15 = const core::mem::offset_of!(TrapFrame, r15),
        off_rip = const core::mem::offset_of!(TrapFrame, rip),
        off_cs = const core::mem::offset_of!(TrapFrame, cs),
        off_rflags = const core::mem::offset_of!(TrapFrame, rflags),
        off_rsp = const core::mem::offset_of!(TrapFrame, rsp),
        off_ss = const core::mem::offset_of!(TrapFrame, ss),
    );
}

/// Naked LAPIC timer: CPL=3 may `iretq` another RIP; CPL=0 only ticks.
#[unsafe(naked)]
pub unsafe extern "C" fn lapic_timer_entry() {
    naked_asm!(
        "test qword ptr [rsp + 8], 3",
        "jnz 2f",
        "push rax",
        "push rcx",
        "push rdx",
        "push rsi",
        "push rdi",
        "push r11",
        "call {kern}",
        "pop r11",
        "pop rdi",
        "pop rsi",
        "pop rdx",
        "pop rcx",
        "pop rax",
        "iretq",
        "2:",
        "push r15",
        "push r14",
        "push r13",
        "push r12",
        "push r11",
        "push r10",
        "push r9",
        "push r8",
        "push rbp",
        "push rdi",
        "push rsi",
        "push rdx",
        "push rcx",
        "push rbx",
        "push rax",
        "mov rdx, [{fxptr}]",
        "fxsave64 [rdx]",
        "mov rdi, rsp",
        "push rax",
        "call {user}",
        "add rsp, 8",
        "mov rdx, [{fxptr}]",
        "fxrstor64 [rdx]",
        "pop rax",
        "pop rbx",
        "pop rcx",
        "pop rdx",
        "pop rsi",
        "pop rdi",
        "pop rbp",
        "pop r8",
        "pop r9",
        "pop r10",
        "pop r11",
        "pop r12",
        "pop r13",
        "pop r14",
        "pop r15",
        "iretq",
        kern = sym timer_kernel,
        user = sym timer_from_user,
        fxptr = sym FXSAVE_PTR,
    );
}
