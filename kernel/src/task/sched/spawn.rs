//! Spawn, wait, kill, thread create, and the init/exclusive run loops.

use crate::elfload::{self, Image};
use crate::fd::FdTable;
use crate::percpu;
use crate::process::Outcome;

use super::pcb::{ERR, KernelCont, MAX_PROC, PS_REC, Pcb, SCHED, Sched, State, Thread, TrapFrame};
use super::switch::{
    FX_TEMPLATE, FXSAVES, KCONTS, apply_hw, drop_pcb, enter_scheduler, fxsave_current,
    leave_scheduler, schedule_next, set_current_slot, yield_block,
};

pub fn with_current_fds<T>(f: impl FnOnce(&mut FdTable) -> T) -> Option<T> {
    let mut s = SCHED.lock();
    let i = s.current[percpu::cpu_id()]?;
    let proc = s.threads[i].as_ref()?.proc;
    Some(f(&mut s.procs[proc].as_mut()?.fds))
}

pub fn current_pid() -> Option<u32> {
    let s = SCHED.lock();
    let i = s.current[percpu::cpu_id()]?;
    let proc = s.threads[i].as_ref()?.proc;
    s.procs[proc].as_ref().map(|p| p.pid)
}

pub fn has_runnable_other() -> bool {
    let s = SCHED.lock();
    let cur = s.current[percpu::cpu_id()];
    s.threads.iter().enumerate().any(|(i, t)| {
        Some(i) != cur
            && t.as_ref()
                .is_some_and(|t| t.state == State::Runnable || t.state == State::Running)
    })
}

pub fn current_is_zombie() -> bool {
    let s = SCHED.lock();
    s.current[percpu::cpu_id()]
        .and_then(|i| s.threads[i].as_ref())
        .is_some_and(|t| t.state == State::Zombie)
}

pub fn wake_stdin() {
    let mut s = SCHED.lock();
    for t in s.threads.iter_mut().flatten() {
        if t.state == State::BlockedStdin {
            t.state = State::Runnable;
        }
    }
}

pub fn block_stdin() {
    yield_block(State::BlockedStdin);
}

pub fn block_pipe_read() {
    yield_block(State::BlockedPipeRead);
}

pub fn block_pipe_write() {
    yield_block(State::BlockedPipeWrite);
}

pub fn wake_pipe() {
    let mut s = SCHED.lock();
    wake_pipe_locked(&mut s);
}

pub(super) fn wake_pipe_locked(s: &mut Sched) {
    for t in s.threads.iter_mut().flatten() {
        if t.state == State::BlockedPipeRead || t.state == State::BlockedPipeWrite {
            t.state = State::Runnable;
        }
    }
}

pub fn run_init(image: Image) -> Outcome {
    run_with(image, "sh", false)
}

pub fn run_exclusive(image: Image, name: &str) -> Outcome {
    run_with(image, name, true)
}

pub(super) fn run_with(image: Image, name: &str, exclusive: bool) -> Outcome {
    let cpu = percpu::cpu_id();
    {
        let mut s = SCHED.lock();
        s.exclusive = exclusive;
        s.next_pid = 1;
        s.init_pid = 0;
        s.current[cpu] = None;
    }
    let argv = match elfload::write_argv(&image, &[basename(name)]) {
        Ok(a) => a,
        Err(()) => {
            elfload::unload(image);
            return Outcome::Fault;
        }
    };
    let Some(slot) = insert_pcb(image, name, 0, argv, u64::MAX, u64::MAX) else {
        return Outcome::Fault;
    };
    {
        let mut s = SCHED.lock();
        let proc = s.threads[slot].as_ref().unwrap().proc;
        let pid = s.procs[proc].as_ref().unwrap().pid;
        s.init_pid = pid;
        s.current[cpu] = Some(slot);
        s.threads[slot].as_mut().unwrap().state = State::Running;
    }
    set_current_slot(slot);
    apply_hw(slot);
    unsafe {
        enter_scheduler();
    }
    match percpu::this_cpu().outcome {
        1 => Outcome::Fault,
        _ => Outcome::Exited,
    }
}

/// Insert a process and its main thread. Returns the main thread's slot.
pub(super) fn insert_pcb(
    image: Image,
    name: &str,
    parent: u32,
    argv: elfload::ArgvSetup,
    stdin_fd: u64,
    stdout_fd: u64,
) -> Option<usize> {
    let mut s = SCHED.lock();
    let proc_slot = s.procs.iter().position(|p| p.is_none());
    let thread_slot = s.threads.iter().position(|t| t.is_none());
    if proc_slot.is_none() || thread_slot.is_none() {
        drop(s);
        elfload::unload(image);
        return None;
    }
    let proc_slot = proc_slot.unwrap();
    let thread_slot = thread_slot.unwrap();

    let fds = if parent == 0 {
        FdTable::new_stdio()
    } else {
        match s.procs.iter().flatten().find(|p| p.pid == parent) {
            Some(p) => match p.fds.inherit_from(stdin_fd, stdout_fd) {
                Some(t) => t,
                None => {
                    drop(s);
                    elfload::unload(image);
                    return None;
                }
            },
            None => {
                drop(s);
                elfload::unload(image);
                return None;
            }
        }
    };
    let pid = s.next_pid;
    s.next_pid = s.next_pid.saturating_add(1);
    let tid = s.next_tid;
    s.next_tid = s.next_tid.saturating_add(1);
    let trap = TrapFrame {
        rax: 0,
        rbx: 0,
        rcx: 0,
        rdx: 0,
        rsi: argv.argv_ptr,
        rdi: argv.argc,
        rbp: 0,
        r8: 0,
        r9: 0,
        r10: 0,
        r11: 0,
        r12: 0,
        r13: 0,
        r14: 0,
        r15: 0,
        rip: image.entry.as_u64(),
        cs: u64::from(crate::gdt::user_code().0),
        rflags: 0x202,
        rsp: argv.rsp,
        ss: u64::from(crate::gdt::user_data().0),
    };
    let l4 = image.l4;
    s.procs[proc_slot] = Some(Pcb {
        pid,
        parent,
        name: name_from_path(name),
        l4,
        fds,
        image: Some(image),
        spawned_child: false,
        last_spawned: 0,
        fault: false,
        zombie: false,
        live_threads: 1,
    });
    s.threads[thread_slot] = Some(Thread {
        tid,
        proc: proc_slot,
        state: State::Runnable,
        trap,
        kcont_valid: false,
        user_rsp: 0,
        tls: 0,
    });
    let cpu = percpu::cpu_id();
    unsafe {
        FXSAVES[cpu][thread_slot] = FX_TEMPLATE;
        KCONTS[cpu][thread_slot] = KernelCont::zero();
    }
    if parent != 0 {
        if let Some(p) = s.procs.iter_mut().flatten().find(|p| p.pid == parent) {
            p.spawned_child = true;
            p.last_spawned = pid;
        }
    }
    Some(thread_slot)
}

pub(super) fn name_from_path(path: &str) -> [u8; 12] {
    let base = basename(path);
    let mut name = [0u8; 12];
    let b = base.as_bytes();
    let n = b.len().min(12);
    name[..n].copy_from_slice(&b[..n]);
    name
}

fn basename(path: &str) -> &str {
    let base = path.rsplit('/').next().unwrap_or(path);
    if base.is_empty() { path } else { base }
}

pub fn user_exit(outcome: Outcome) -> ! {
    fxsave_current();
    let (is_init, exclusive, is_last, proc_idx, pid) = {
        let cpu = percpu::cpu_id();
        let mut s = SCHED.lock();
        let slot = s.current[cpu].expect("user_exit");
        let proc_idx = s.threads[slot].as_ref().unwrap().proc;
        if let Some(t) = s.threads[slot].as_mut() {
            t.state = State::Zombie;
            t.kcont_valid = false;
        }
        let live = s.procs[proc_idx].as_mut().unwrap().live_threads.saturating_sub(1);
        s.procs[proc_idx].as_mut().unwrap().live_threads = live;
        let is_last = live == 0;
        let is_init = s.procs[proc_idx].as_ref().unwrap().pid == s.init_pid;
        let pid = s.procs[proc_idx].as_ref().unwrap().pid;
        (is_init, s.exclusive, is_last, proc_idx, pid)
    };
    if is_last {
        crate::win::drop_pid(pid);
        {
            let mut s = SCHED.lock();
            make_zombie_locked(&mut s, proc_idx, outcome == Outcome::Fault);
        }
        if exclusive || is_init {
            leave_scheduler(outcome);
        }
        wake_parent_of_proc(proc_idx);
    }
    schedule_next();
}

/// Mark a process (and every one of its threads) as exited, awaiting reap.
pub(super) fn make_zombie_locked(s: &mut Sched, proc_idx: usize, fault: bool) {
    if let Some(p) = s.procs[proc_idx].as_mut() {
        p.fds.close_all();
        p.zombie = true;
        p.fault = fault;
        p.live_threads = 0;
    }
    for t in s.threads.iter_mut().flatten() {
        if t.proc == proc_idx {
            t.state = State::Zombie;
            t.kcont_valid = false;
        }
    }
    wake_pipe_locked(s);
}

pub(super) fn wake_parent_of_proc(proc_idx: usize) {
    let mut s = SCHED.lock();
    let parent = s.procs[proc_idx].as_ref().map(|p| p.parent).unwrap_or(0);
    if parent == 0 {
        return;
    }
    let parent_proc = s
        .procs
        .iter()
        .position(|p| p.as_ref().is_some_and(|p| p.pid == parent));
    let Some(pp) = parent_proc else {
        return;
    };
    for t in s.threads.iter_mut().flatten() {
        if t.proc == pp && t.state == State::BlockedWait {
            t.state = State::Runnable;
        }
    }
}

pub fn kill_foreground() -> bool {
    let mut s = SCHED.lock();
    let waiter = s
        .threads
        .iter()
        .position(|t| t.as_ref().is_some_and(|t| t.state == State::BlockedWait));
    let Some(w) = waiter else {
        return false;
    };
    let waiter_proc = s.threads[w].as_ref().unwrap().proc;
    let last = s.procs[waiter_proc].as_ref().unwrap().last_spawned;
    let init = s.init_pid;
    if last == 0 || last == init {
        return false;
    }
    let Some(v) = s
        .procs
        .iter()
        .position(|p| p.as_ref().is_some_and(|p| p.pid == last && !p.zombie))
    else {
        return false;
    };
    make_zombie_locked(&mut s, v, false);
    let parent = s.procs[v].as_ref().unwrap().parent;
    let parent_proc = s
        .procs
        .iter()
        .position(|p| p.as_ref().is_some_and(|p| p.pid == parent));
    if let Some(pp) = parent_proc {
        for t in s.threads.iter_mut().flatten() {
            if t.proc == pp && t.state == State::BlockedWait {
                t.state = State::Runnable;
            }
        }
    }
    true
}

const SPAWN_ARGS: usize = 48;
const ARGV_BLOB_MAX: usize = 1024;

pub fn sys_spawn(args_ptr: u64) -> u64 {
    if !crate::vmm::user_slice_ok(args_ptr, SPAWN_ARGS as u64) {
        return ERR;
    }
    let mut raw = [0u8; SPAWN_ARGS];
    if crate::fd::copy_from_user(args_ptr, SPAWN_ARGS, &mut raw).is_err() {
        return ERR;
    }
    let path_ptr = u64::from_le_bytes(raw[0..8].try_into().unwrap());
    let path_len = u64::from_le_bytes(raw[8..16].try_into().unwrap());
    let argv_ptr = u64::from_le_bytes(raw[16..24].try_into().unwrap());
    let argv_len = u64::from_le_bytes(raw[24..32].try_into().unwrap());
    let stdin_fd = u64::from_le_bytes(raw[32..40].try_into().unwrap());
    let stdout_fd = u64::from_le_bytes(raw[40..48].try_into().unwrap());
    let Some(path) = crate::fd::copy_path(path_ptr, path_len) else {
        return ERR;
    };
    let mut argv_buf = [0u8; ARGV_BLOB_MAX];
    let blob: &[u8] = if argv_len == 0 {
        &[]
    } else {
        if argv_len > ARGV_BLOB_MAX as u64 {
            return ERR;
        }
        if !crate::vmm::user_slice_ok(argv_ptr, argv_len) {
            return ERR;
        }
        let n = argv_len as usize;
        if crate::fd::copy_from_user(argv_ptr, n, &mut argv_buf).is_err() {
            return ERR;
        }
        &argv_buf[..n]
    };
    let parent = {
        let s = SCHED.lock();
        match s.current[percpu::cpu_id()].and_then(|i| s.threads[i].as_ref()) {
            Some(t) => s.procs[t.proc].as_ref().map(|p| p.pid).unwrap_or(0),
            None => 0,
        }
    };
    spawn_with(&path, parent, blob, stdin_fd, stdout_fd)
        .map(u64::from)
        .unwrap_or(ERR)
}

/// Spawn from a kernel path. `parent == 0` does not mark `sh` as having a
/// waitable child (launcher).
pub fn spawn_path(path: &str) -> Option<u32> {
    spawn_with_parent(path, 0)
}

pub(super) fn spawn_with_parent(path: &str, parent: u32) -> Option<u32> {
    spawn_with(path, parent, &[], u64::MAX, u64::MAX)
}

fn spawn_with(
    path: &str,
    parent: u32,
    argv_blob: &[u8],
    stdin_fd: u64,
    stdout_fd: u64,
) -> Option<u32> {
    let bytes = crate::fs::read_file(path).ok()?;
    let image = elfload::load(&bytes).ok()?;
    let argv = match argv_setup(&image, path, argv_blob) {
        Some(a) => a,
        None => {
            elfload::unload(image);
            return None;
        }
    };
    let slot = insert_pcb(image, path, parent, argv, stdin_fd, stdout_fd)?;
    let pid = {
        let s = SCHED.lock();
        let proc = s.threads[slot].as_ref().unwrap().proc;
        s.procs[proc].as_ref().unwrap().pid
    };
    Some(pid)
}

fn argv_setup(image: &Image, path: &str, blob: &[u8]) -> Option<elfload::ArgvSetup> {
    let mut strs: [&str; elfload::ARGV_MAX] = [""; elfload::ARGV_MAX];
    let n = if blob.is_empty() {
        strs[0] = basename(path);
        1
    } else {
        parse_argv_blob(blob, &mut strs)?
    };
    elfload::write_argv(image, &strs[..n]).ok()
}

fn parse_argv_blob<'a>(blob: &'a [u8], out: &mut [&'a str; elfload::ARGV_MAX]) -> Option<usize> {
    if blob.last() != Some(&0) {
        return None;
    }
    let mut n = 0usize;
    let mut start = 0usize;
    for i in 0..blob.len() {
        if blob[i] != 0 {
            continue;
        }
        if i == start || n >= elfload::ARGV_MAX {
            return None;
        }
        let s = core::str::from_utf8(&blob[start..i]).ok()?;
        if s.is_empty() || s.len() > elfload::ARGV_STR_MAX {
            return None;
        }
        out[n] = s;
        n += 1;
        start = i + 1;
    }
    if n == 0 { None } else { Some(n) }
}

pub fn reap_orphans() {
    loop {
        let mut s = SCHED.lock();
        let z = s
            .procs
            .iter()
            .position(|p| p.as_ref().is_some_and(|p| p.parent == 0 && p.zombie));
        let Some(i) = z else {
            return;
        };
        let pcb = s.procs[i].take().unwrap();
        for t in s.threads.iter_mut() {
            if t.as_ref().is_some_and(|t| t.proc == i) {
                *t = None;
            }
        }
        drop(s);
        drop_pcb(pcb);
    }
}

pub fn sys_wait() -> u64 {
    loop {
        match try_reap() {
            WaitResult::Pid(p) => return p as u64,
            WaitResult::NeverHadChildren => return ERR,
            WaitResult::MustBlock => yield_block(State::BlockedWait),
        }
    }
}

pub(super) enum WaitResult {
    Pid(u32),
    NeverHadChildren,
    MustBlock,
}

pub(super) fn try_reap() -> WaitResult {
    let mut s = SCHED.lock();
    let cur = match s.current[percpu::cpu_id()] {
        Some(i) => i,
        None => return WaitResult::NeverHadChildren,
    };
    let proc = s.threads[cur].as_ref().unwrap().proc;
    let pid = s.procs[proc].as_ref().unwrap().pid;
    let spawned = s.procs[proc].as_ref().unwrap().spawned_child;
    let mut zombie = None;
    for (i, p) in s.procs.iter().enumerate() {
        if let Some(pcb) = p {
            if pcb.parent == pid && pcb.zombie {
                zombie = Some(i);
                break;
            }
        }
    }
    if let Some(i) = zombie {
        let pcb = s.procs[i].take().unwrap();
        let cpid = pcb.pid;
        for t in s.threads.iter_mut() {
            if t.as_ref().is_some_and(|t| t.proc == i) {
                *t = None;
            }
        }
        drop(s);
        drop_pcb(pcb);
        return WaitResult::Pid(cpid);
    }
    if !spawned {
        WaitResult::NeverHadChildren
    } else {
        WaitResult::MustBlock
    }
}

pub fn sys_kill(pid: u64) -> u64 {
    let pid = pid as u32;
    let mut s = SCHED.lock();
    let cur = match s.current[percpu::cpu_id()] {
        Some(i) => i,
        None => return ERR,
    };
    let cur_proc = s.threads[cur].as_ref().unwrap().proc;
    let parent_pid = s.procs[cur_proc].as_ref().unwrap().pid;
    if pid == s.init_pid {
        return ERR;
    }
    let Some(v) = s
        .procs
        .iter()
        .position(|p| p.as_ref().is_some_and(|p| p.pid == pid))
    else {
        return ERR;
    };
    if s.procs[v].as_ref().unwrap().parent != parent_pid {
        return ERR;
    }
    if s.procs[v].as_ref().unwrap().zombie {
        return ERR;
    }
    make_zombie_locked(&mut s, v, false);
    0
}

pub fn sys_clock_ms() -> u64 {
    crate::clock::millis()
}

pub fn sys_ps(buf: u64, len: u64) -> u64 {
    if len < PS_REC as u64 {
        return ERR;
    }
    let nrec = (len as usize / PS_REC).min(MAX_PROC);
    let nbytes = nrec * PS_REC;
    if !crate::vmm::user_slice_ok(buf, nbytes as u64) {
        return ERR;
    }
    let mut tmp = [0u8; MAX_PROC * PS_REC];
    let mut n = 0usize;
    {
        let s = SCHED.lock();
        for p in s.procs.iter().flatten() {
            if n >= nrec {
                break;
            }
            tmp[n * PS_REC..n * PS_REC + 4].copy_from_slice(&p.pid.to_le_bytes());
            tmp[n * PS_REC + 4..n * PS_REC + 16].copy_from_slice(&p.name);
            n += 1;
        }
    }
    let wrote = n * PS_REC;
    if crate::fd::copy_to_user(buf, &tmp[..wrote]).is_err() {
        return ERR;
    }
    wrote as u64
}

const THREAD_ARGS: usize = 32;

/// `SYS_THREAD_CREATE`: run `entry(arg)` on a new thread that shares this
/// process's address space, FDs and image. `stack` is the top of the new
/// thread's user stack; `tls` becomes its `FsBase`. Returns the new tid.
pub fn sys_thread_create(args_ptr: u64) -> u64 {
    if !crate::vmm::user_slice_ok(args_ptr, THREAD_ARGS as u64) {
        return ERR;
    }
    let mut raw = [0u8; THREAD_ARGS];
    if crate::fd::copy_from_user(args_ptr, THREAD_ARGS, &mut raw).is_err() {
        return ERR;
    }
    let entry = u64::from_le_bytes(raw[0..8].try_into().unwrap());
    let arg = u64::from_le_bytes(raw[8..16].try_into().unwrap());
    let stack = u64::from_le_bytes(raw[16..24].try_into().unwrap());
    let tls = u64::from_le_bytes(raw[24..32].try_into().unwrap());

    // The stack top must sit in a mapped, user-accessible page.
    if !crate::vmm::user_slice_ok(stack, 1) {
        return ERR;
    }

    let mut s = SCHED.lock();
    let cpu = percpu::cpu_id();
    let Some(cur) = s.current[cpu] else {
        return ERR;
    };
    let proc_idx = s.threads[cur].as_ref().map(|t| t.proc).unwrap_or(usize::MAX);
    if proc_idx == usize::MAX || s.procs[proc_idx].is_none() {
        return ERR;
    }
    let Some(slot) = s.threads.iter().position(|t| t.is_none()) else {
        return ERR;
    };

    let tid = s.next_tid;
    s.next_tid = s.next_tid.saturating_add(1);
    let trap = TrapFrame {
        rax: 0,
        rbx: 0,
        rcx: 0,
        rdx: 0,
        rsi: 0,
        rdi: arg,
        rbp: 0,
        r8: 0,
        r9: 0,
        r10: 0,
        r11: 0,
        r12: 0,
        r13: 0,
        r14: 0,
        r15: 0,
        rip: entry,
        cs: u64::from(crate::gdt::user_code().0),
        rflags: 0x202,
        rsp: stack,
        ss: u64::from(crate::gdt::user_data().0),
    };
    s.threads[slot] = Some(Thread {
        tid,
        proc: proc_idx,
        state: State::Runnable,
        trap,
        kcont_valid: false,
        user_rsp: 0,
        tls,
    });
    s.procs[proc_idx].as_mut().unwrap().live_threads += 1;
    unsafe {
        FXSAVES[cpu][slot] = FX_TEMPLATE;
        KCONTS[cpu][slot] = KernelCont::zero();
    }
    u64::from(tid)
}
