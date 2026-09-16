//! Round-robin Ring 3. One PCB per process; preemption only at CPL=3.

pub(crate) mod pcb;
mod spawn;
mod switch;

pub use pcb::{MAX_PROC, State};
pub use spawn::{
    block_pipe_read, block_pipe_write, block_stdin, current_is_zombie, current_pid,
    has_runnable_other, kill_foreground, reap_orphans, run_exclusive, run_init, spawn_path,
    sys_clock_ms, sys_kill, sys_ps, sys_spawn, sys_thread_create, sys_wait, user_exit, wake_pipe,
    wake_stdin, with_current_fds,
};
pub use switch::{init, init_template, lapic_timer_entry, schedule_next, timer_kernel};
