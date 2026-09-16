//! Symmetric multiprocessing: bring up non-boot CPUs (APs).
//!
//! The bootloader already handed us the AP list through the `MpRequest`. Each
//! AP runs [`ap_entry`] on its own 64 KiB bootloader stack, initializes its own
//! GDT/TSS, IDT, syscall MSRs, FPU and LAPIC, then idles. Process scheduling
//! and the compositor stay on the BSP for now, so APs never touch `SCHED`.

use core::sync::atomic::{AtomicU32, Ordering};

use limine::mp::Cpu;
use limine::response::MpResponse;
use x86_64::registers::model_specific::{GsBase, KernelGsBase};
use x86_64::VirtAddr;

use crate::percpu;

/// Dense CPU index handed to the next AP. The BSP is 0, so APs start at 1.
static NEXT_CPU: AtomicU32 = AtomicU32::new(1);

/// Start every non-BSP CPU. Returns how many were started.
pub fn start_aps(resp: &MpResponse) -> usize {
    let bsp = resp.bsp_lapic_id();
    let mut started = 0;
    for cpu in resp.cpus() {
        if cpu.lapic_id == bsp {
            continue;
        }
        let idx = NEXT_CPU.fetch_add(1, Ordering::SeqCst);
        if idx as usize >= percpu::MAX_CPU {
            break;
        }
        // Hand the dense index to the AP through `extra`; `goto_address.write`
        // synchronizes all prior writes before the AP jumps in.
        cpu.extra.store(u64::from(idx), Ordering::SeqCst);
        cpu.goto_address.write(ap_entry);
        started += 1;
    }
    started
}

/// Entry the bootloader jumps each AP into. Never returns.
///
/// # Safety
/// Limine calls this with interrupts disabled and a valid 64 KiB stack.
unsafe extern "C" fn ap_entry(cpu: &Cpu) -> ! {
    let id = cpu.extra.load(Ordering::Acquire) as usize;
    if id == 0 || id >= percpu::MAX_CPU {
        crate::console::write("smp: bad ap id\n");
        crate::hcf();
    }

    // Point GS at this CPU's block before any `this_cpu()`/`cpu_id()` use.
    unsafe {
        GsBase::write(VirtAddr::from_ptr(&raw const percpu::PERCPU[id]));
        KernelGsBase::write(VirtAddr::zero());
    }

    crate::gdt::init_cpu(id);
    crate::interrupts::load_idt();
    crate::syscall::init_cpu(id);
    crate::sched::init(); // FPU enable, per-CPU (CR0/CR4)
    crate::lapic::init_ap();
    crate::serial_println!("smp: ap {} up", id);

    // Idle forever. Only the LAPIC timer (100 Hz, acked in `timer_kernel`)
    // wakes us; scheduling and the compositor remain BSP-only.
    x86_64::instructions::interrupts::enable();
    loop {
        x86_64::instructions::interrupts::enable_and_hlt();
    }
}
