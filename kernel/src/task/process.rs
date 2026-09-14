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
    sched::user_exit(outcome);
}
