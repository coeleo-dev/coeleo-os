//! Userspace process entry from the in-kernel fallback (`run`).

use crate::elfload::Image;
use crate::sched;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outcome {
    Exited,
    Fault,
}

pub fn run_named(image: Image, name: &str) -> Outcome {
    sched::run_exclusive(image, name)
}

pub fn exit_from_syscall(_code: u64) -> ! {
    sched::user_exit(Outcome::Exited);
}

pub fn return_to_kernel(outcome: Outcome) -> ! {
    // Fault handlers (page fault, GP fault) are `x86-interrupt` entries: the
    // IDT exception path does not `swapgs`, so GS still holds the user value
    // (0) here. Recover the kernel per-CPU block before touching the scheduler.
    unsafe {
        core::arch::asm!("swapgs", options(nostack));
    }
    sched::user_exit(outcome);
}
