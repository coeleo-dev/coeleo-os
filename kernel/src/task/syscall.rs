//! `syscall` / `sysretq`. TCB: naked entry and MSRs.

use core::arch::naked_asm;

use x86_64::VirtAddr;
use x86_64::registers::model_specific::{
    Efer, EferFlags, GsBase, KernelGsBase, LStar, SFMask, Star,
};
use x86_64::registers::rflags::RFlags;

pub const SYS_EXIT: u64 = 1;
pub const SYS_WRITE: u64 = 2;
pub const SYS_OPEN: u64 = 3;
pub const SYS_READ: u64 = 4;
pub const SYS_CLOSE: u64 = 5;
pub const SYS_READDIR: u64 = 6;
pub const SYS_SPAWN: u64 = 7;
pub const SYS_WAIT: u64 = 8;
pub const SYS_KILL: u64 = 9;
pub const SYS_CLOCK_MS: u64 = 10;
pub const SYS_PS: u64 = 11;
pub const SYS_UNLINK: u64 = 12;
pub const SYS_SYNC: u64 = 13;
pub const SYS_SYSINFO: u64 = 14;
pub const SYS_NET_PING: u64 = 15;
pub const SYS_HTTP_GET: u64 = 16;
pub const SYS_WIN_CREATE: u64 = 17;
pub const SYS_WIN_DAMAGE: u64 = 18;
pub const SYS_POLL_INPUT: u64 = 19;
pub const SYS_DATE: u64 = 20;
pub const SYS_REBOOT: u64 = 21;
pub const SYS_POWEROFF: u64 = 22;
pub const SYS_DISKS: u64 = 23;
pub const SYS_INSTALL: u64 = 24;
pub const SYS_PIPE: u64 = 25;
pub const SYS_MKDIR: u64 = 26;

#[repr(C)]
pub struct CpuLocal {
    pub kernel_rsp: u64,
    pub user_rsp: u64,
}

pub static mut CPU_LOCAL: CpuLocal = CpuLocal {
    kernel_rsp: 0,
    user_rsp: 0,
};

pub fn init() {
    unsafe {
        CPU_LOCAL.kernel_rsp = crate::gdt::user_kernel_stack_top().as_u64();
        GsBase::write(VirtAddr::zero());
        KernelGsBase::write(VirtAddr::from_ptr(&raw const CPU_LOCAL));
        Efer::update(|f| {
            f.insert(EferFlags::SYSTEM_CALL_EXTENSIONS | EferFlags::NO_EXECUTE_ENABLE);
        });
    }
    Star::write(
        crate::gdt::user_code(),
        crate::gdt::user_data(),
        crate::gdt::kernel_code(),
        crate::gdt::kernel_data(),
    )
    .expect("STAR");
    LStar::write(VirtAddr::new(syscall_entry as *const () as usize as u64));
    SFMask::write(RFlags::INTERRUPT_FLAG | RFlags::DIRECTION_FLAG);
}

#[unsafe(naked)]
unsafe extern "C" fn syscall_entry() {
    naked_asm!(
        "swapgs",
        "mov gs:[{user}], rsp",
        "mov rsp, gs:[{kern}]",
        "push r11",
        "push rcx",
        "mov r8, rdx",
        "mov rdx, rsi",
        "mov rsi, rdi",
        "mov rdi, rax",
        "mov rcx, r8",
        "call {dispatch}",
        "pop rcx",
        "pop r11",
        "mov rsp, gs:[{user}]",
        "swapgs",
        "sysretq",
        user = const core::mem::offset_of!(CpuLocal, user_rsp),
        kern = const core::mem::offset_of!(CpuLocal, kernel_rsp),
        dispatch = sym syscall_dispatch,
    );
}

extern "C" fn syscall_dispatch(num: u64, a0: u64, a1: u64, a2: u64) -> u64 {
    crate::comp::poll();
    match num {
        SYS_EXIT => crate::process::exit_from_syscall(a0),
        SYS_WRITE => crate::fd::sys_write(a0, a1, a2),
        SYS_OPEN => crate::fd::sys_open(a0, a1, a2),
        SYS_READ => crate::fd::sys_read(a0, a1, a2),
        SYS_CLOSE => crate::fd::sys_close(a0),
        SYS_READDIR => crate::fd::sys_readdir(a0, a1, a2),
        SYS_SPAWN => crate::sched::sys_spawn(a0),
        SYS_WAIT => crate::sched::sys_wait(),
        SYS_KILL => crate::sched::sys_kill(a0),
        SYS_CLOCK_MS => crate::sched::sys_clock_ms(),
        SYS_PS => crate::sched::sys_ps(a0, a1),
        SYS_UNLINK => crate::fd::sys_unlink(a0, a1),
        SYS_SYNC => crate::fd::sys_sync(),
        SYS_SYSINFO => crate::fd::sys_sysinfo(a0, a1, a2),
        SYS_NET_PING => crate::net::sys_net_ping(a0),
        SYS_HTTP_GET => crate::net::sys_http_get(a0),
        SYS_WIN_CREATE => crate::win::sys_create(a0, a1, a2),
        SYS_WIN_DAMAGE => crate::win::sys_damage(a0, a1, a2),
        SYS_POLL_INPUT => crate::win::sys_poll_input(a0, a1),
        SYS_DATE => crate::rtc::sys_date(a0, a1),
        SYS_REBOOT => crate::acpi::sys_reboot(),
        SYS_POWEROFF => crate::acpi::sys_poweroff(),
        SYS_DISKS => crate::blk::sys_disks(a0, a1),
        SYS_INSTALL => crate::install::sys_install(a0),
        SYS_PIPE => crate::fd::sys_pipe(a0),
        SYS_MKDIR => crate::fd::sys_mkdir(a0, a1),
        _ => u64::MAX,
    }
}
