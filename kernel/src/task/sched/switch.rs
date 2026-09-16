//! Context switch, FPU, and LAPIC timer. All naked asm lives here.

use core::arch::naked_asm;

use x86_64::VirtAddr;
use x86_64::registers::control::{Cr0, Cr0Flags, Cr4, Cr4Flags};
use x86_64::registers::model_specific::{FsBase, GsBase, KernelGsBase};

use crate::elfload;
use crate::percpu::{self, PerCpu, MAX_CPU};
use crate::process::Outcome;

use super::pcb::{
    FxBuf, IrqFrame, KSTACK_SIZE, KernelCont, KernelStack, MAX_PROC, Pcb, SCHED, Sched, State,
    TrapFrame,
};

pub(super) static mut KSTACKS: [[KernelStack; MAX_PROC]; MAX_CPU] =
    [[KernelStack([0; KSTACK_SIZE]); MAX_PROC]; MAX_CPU];
pub(super) static mut KCONTS: [[KernelCont; MAX_PROC]; MAX_CPU] =
    [[KernelCont::zero(); MAX_PROC]; MAX_CPU];
pub(super) static mut FXSAVES: [[FxBuf; MAX_PROC]; MAX_CPU] =
    [[FxBuf::zero(); MAX_PROC]; MAX_CPU];
pub(super) static mut FX_TEMPLATE: FxBuf = FxBuf([0; 512]);

pub(super) fn kstack_top(slot: usize) -> VirtAddr {
    let cpu = percpu::cpu_id();
    let end = unsafe { (&raw const KSTACKS[cpu][slot].0).cast::<u8>().add(KSTACK_SIZE) };
    VirtAddr::from_ptr(end)
}

pub(super) fn set_current_slot(slot: usize) {
    let cpu = percpu::cpu_id();
    let pc = percpu::this_cpu();
    pc.current_slot = slot;
    pc.kcont_ptr = unsafe { &raw mut KCONTS[cpu][slot] as *mut KernelCont as usize };
    pc.fxsave_ptr = unsafe { core::ptr::addr_of_mut!(FXSAVES[cpu][slot].0) as usize };
}

/// Per-CPU FPU enable (CR0/CR4) and reset. Runs on the BSP and every AP.
pub fn init() {
    unsafe {
        Cr0::update(|f| {
            f.remove(Cr0Flags::EMULATE_COPROCESSOR);
            f.insert(Cr0Flags::MONITOR_COPROCESSOR);
        });
        Cr4::update(|f| {
            f.insert(Cr4Flags::OSFXSR | Cr4Flags::OSXMMEXCPT_ENABLE);
        });
        core::arch::asm!("fninit", options(nostack));
    }
}

/// Capture the pristine FPU state into `FX_TEMPLATE`. BSP only, right after
/// [`init`], before any FPU use: the template is copied to every new thread.
pub fn init_template() {
    unsafe {
        core::arch::asm!(
            "fxsave64 [{}]",
            in(reg) core::ptr::addr_of_mut!(FX_TEMPLATE.0),
            options(nostack),
        );
    }
}

pub(super) fn gs_enter_kernel_syscall() {
    unsafe {
        GsBase::write(VirtAddr::from_ptr(percpu::this_cpu() as *const PerCpu));
        KernelGsBase::write(VirtAddr::zero());
    }
}

pub(super) fn apply_hw(slot: usize) {
    let (l4, top, user_rsp) = {
        let s = SCHED.lock();
        let t = s.threads[slot].as_ref().unwrap();
        let p = s.procs[t.proc].as_ref().unwrap();
        (p.l4, kstack_top(slot), t.user_rsp)
    };
    let pc = percpu::this_cpu();
    pc.kernel_rsp = top.as_u64();
    pc.user_rsp = user_rsp;
    crate::gdt::set_user_kernel_stack(top);
    crate::vmm::load_cr3(l4);
}

pub(super) unsafe fn enter_scheduler() {
    unsafe {
        if save_host() == 0 {
            let cpu = percpu::cpu_id();
            let slot = SCHED.lock().current[cpu].unwrap();
            resume_user_slot(slot);
        }
    }
}

pub(super) fn leave_scheduler(outcome: Outcome) -> ! {
    unload_all();
    {
        let cpu = percpu::cpu_id();
        let mut s = SCHED.lock();
        s.current[cpu] = None;
        s.exclusive = false;
        s.init_pid = 0;
    }
    crate::vmm::restore_kernel_cr3();
    let cpu = percpu::cpu_id();
    let pc = percpu::this_cpu();
    pc.kernel_rsp = crate::gdt::user_kernel_stack_top(cpu).as_u64();
    crate::gdt::set_user_kernel_stack(crate::gdt::user_kernel_stack_top(cpu));
    unsafe {
        GsBase::write(VirtAddr::from_ptr(pc as *const PerCpu));
        KernelGsBase::write(VirtAddr::zero());
    }
    pc.outcome = match outcome {
        Outcome::Exited => 0,
        Outcome::Fault => 1,
    };
    unsafe {
        restore_host();
    }
}

pub(super) fn unload_all() {
    let mut s = SCHED.lock();
    for slot in 0..MAX_PROC {
        s.threads[slot] = None;
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
    let cpu = percpu::cpu_id();
    let slot = percpu::this_cpu().current_slot;
    unsafe {
        core::arch::asm!(
            "fxsave64 [{}]",
            in(reg) core::ptr::addr_of_mut!(FXSAVES[cpu][slot].0),
            options(nostack, preserves_flags),
        );
    }
}

pub(super) fn yield_block(state: State) {
    unsafe {
        if save_kcont() == 0 {
            fxsave_current();
            {
                let cpu = percpu::cpu_id();
                let mut s = SCHED.lock();
                let i = s.current[cpu].expect("yield");
                s.threads[i].as_mut().unwrap().state = state;
                s.threads[i].as_mut().unwrap().kcont_valid = true;
            }
            schedule_next();
        } else {
            let slot = percpu::this_cpu().current_slot;
            {
                let mut s = SCHED.lock();
                if let Some(t) = s.threads[slot].as_mut() {
                    t.kcont_valid = false;
                }
            }
            core::arch::asm!(
                "fxrstor64 [{}]",
                in(reg) core::ptr::addr_of!(FXSAVES[percpu::cpu_id()][slot].0),
                options(nostack, preserves_flags),
            );
        }
    }
}

pub fn schedule_next() -> ! {
    let cpu = percpu::cpu_id();
    loop {
        let pick = {
            let s = SCHED.lock();
            pick_runnable(&s, cpu)
        };
        if let Some(slot) = pick {
            switch_to(slot);
        }
        // The compositor and the clock overlay are painted only on the BSP;
        // an AP idling in the scheduler must not race the framebuffer.
        if cpu == 0 {
            crate::comp::poll();
            crate::clock::paint_if_second_elapsed();
        }
        x86_64::instructions::interrupts::enable_and_hlt();
        x86_64::instructions::interrupts::disable();
    }
}

pub(super) fn pick_runnable(s: &Sched, cpu: usize) -> Option<usize> {
    let start = s.current[cpu].map(|c| c + 1).unwrap_or(0);
    for k in 0..MAX_PROC {
        let i = (start + k) % MAX_PROC;
        let Some(t) = s.threads[i].as_ref() else {
            continue;
        };
        if t.state == State::Runnable || t.state == State::Running {
            return Some(i);
        }
    }
    None
}

pub(super) fn switch_to(slot: usize) -> ! {
    let cpu = percpu::cpu_id();
    let kcont = {
        let mut s = SCHED.lock();
        if let Some(c) = s.current[cpu] {
            if c != slot {
                if let Some(t) = s.threads[c].as_mut() {
                    if t.state == State::Running {
                        t.state = State::Runnable;
                    }
                    t.user_rsp = percpu::this_cpu().user_rsp;
                }
            }
        }
        s.current[cpu] = Some(slot);
        s.threads[slot].as_mut().unwrap().state = State::Running;
        s.threads[slot].as_ref().unwrap().kcont_valid
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
    let cpu = percpu::cpu_id();
    let (trap, tls) = {
        let s = SCHED.lock();
        let t = s.threads[slot].as_ref().unwrap();
        (t.trap, t.tls)
    };
    let pc = percpu::this_cpu();
    pc.iret_trap = trap;
    pc.iret_fx = unsafe { FXSAVES[cpu][slot] };
    // Restore this thread's TLS before dropping into Ring 3 (iretq does not
    // reload FS).
    FsBase::write(VirtAddr::new(tls));
    unsafe {
        resume_user();
    }
}

/// Timer IRQ from Ring 3. Returns if the same process continues.
extern "C" fn timer_from_user(frame: *const IrqFrame) {
    let frame = unsafe { &*frame };
    crate::clock::tick();
    crate::lapic::eoi();
    let cpu = percpu::cpu_id();
    let next = {
        let mut s = SCHED.lock();
        let Some(cur) = s.current[cpu] else {
            return;
        };
        if s.threads[cur]
            .as_ref()
            .is_none_or(|t| t.state == State::Zombie)
        {
            drop(s);
            schedule_next();
        }
        let other = pick_runnable_other(&s, cur);
        let Some(n) = other else {
            return;
        };
        if let Some(t) = s.threads[cur].as_mut() {
            t.trap = trap_from_irq(frame);
            t.state = State::Runnable;
            t.kcont_valid = false;
            t.user_rsp = percpu::this_cpu().user_rsp;
        }
        s.current[cpu] = Some(n);
        s.threads[n].as_mut().unwrap().state = State::Running;
        n
    };
    set_current_slot(next);
    apply_hw(next);
    let kcont = SCHED.lock().threads[next].as_ref().unwrap().kcont_valid;
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
        if s.threads[i]
            .as_ref()
            .is_some_and(|t| t.state == State::Runnable)
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
    // Only the BSP advances the system clock; every CPU still ACKs its own
    // timer so the LAPIC can fire again.
    if percpu::cpu_id() == 0 {
        crate::clock::tick();
    }
    crate::lapic::eoi();
}

#[unsafe(naked)]
unsafe extern "C" fn save_kcont() -> u64 {
    naked_asm!(
        "mov rax, gs:[{kptr}]",
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
        kptr = const core::mem::offset_of!(PerCpu, kcont_ptr),
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
        "mov rax, gs:[{kptr}]",
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
        kptr = const core::mem::offset_of!(PerCpu, kcont_ptr),
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
        "mov gs:[{host} + {off_rbx}], rbx",
        "mov gs:[{host} + {off_rbp}], rbp",
        "mov gs:[{host} + {off_r12}], r12",
        "mov gs:[{host} + {off_r13}], r13",
        "mov gs:[{host} + {off_r14}], r14",
        "mov gs:[{host} + {off_r15}], r15",
        "mov rcx, [rsp]",
        "mov gs:[{host} + {off_rip}], rcx",
        "lea rcx, [rsp + 8]",
        "mov gs:[{host} + {off_rsp}], rcx",
        "pushfq",
        "pop rcx",
        "mov gs:[{host} + {off_rflags}], rcx",
        "xor eax, eax",
        "ret",
        host = const core::mem::offset_of!(PerCpu, host_kcont),
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
        "mov rbx, gs:[{host} + {off_rbx}]",
        "mov rbp, gs:[{host} + {off_rbp}]",
        "mov r12, gs:[{host} + {off_r12}]",
        "mov r13, gs:[{host} + {off_r13}]",
        "mov r14, gs:[{host} + {off_r14}]",
        "mov r15, gs:[{host} + {off_r15}]",
        "mov rsp, gs:[{host} + {off_rsp}]",
        "push qword ptr gs:[{host} + {off_rflags}]",
        "popfq",
        "mov rcx, gs:[{host} + {off_rip}]",
        "mov eax, 1",
        "jmp rcx",
        host = const core::mem::offset_of!(PerCpu, host_kcont),
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
        "fxrstor64 gs:[{fx}]",
        "mov rax, gs:[{t} + {off_rax}]",
        "mov rbx, gs:[{t} + {off_rbx}]",
        "mov rcx, gs:[{t} + {off_rcx}]",
        "mov rdx, gs:[{t} + {off_rdx}]",
        "mov rsi, gs:[{t} + {off_rsi}]",
        "mov rdi, gs:[{t} + {off_rdi}]",
        "mov rbp, gs:[{t} + {off_rbp}]",
        "mov r8, gs:[{t} + {off_r8}]",
        "mov r9, gs:[{t} + {off_r9}]",
        "mov r10, gs:[{t} + {off_r10}]",
        "mov r11, gs:[{t} + {off_r11}]",
        "mov r12, gs:[{t} + {off_r12}]",
        "mov r13, gs:[{t} + {off_r13}]",
        "mov r14, gs:[{t} + {off_r14}]",
        "mov r15, gs:[{t} + {off_r15}]",
        "push qword ptr gs:[{t} + {off_ss}]",
        "push qword ptr gs:[{t} + {off_rsp}]",
        "push qword ptr gs:[{t} + {off_rflags}]",
        "push qword ptr gs:[{t} + {off_cs}]",
        "push qword ptr gs:[{t} + {off_rip}]",
        "swapgs",
        "iretq",
        t = const core::mem::offset_of!(PerCpu, iret_trap),
        fx = const core::mem::offset_of!(PerCpu, iret_fx),
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
        "swapgs",
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
        "mov rdx, gs:[{fxptr}]",
        "fxsave64 [rdx]",
        "mov rdi, rsp",
        "push rax",
        "call {user}",
        "add rsp, 8",
        "mov rdx, gs:[{fxptr}]",
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
        "swapgs",
        "iretq",
        kern = sym timer_kernel,
        user = sym timer_from_user,
        fxptr = const core::mem::offset_of!(PerCpu, fxsave_ptr),
    );
}
