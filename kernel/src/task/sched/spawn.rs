//! Spawn, wait, kill, and the init/exclusive run loops.

use core::sync::atomic::Ordering;

use crate::elfload::{self, Image};
use crate::fd::FdTable;
use crate::process::Outcome;

use super::pcb::{ERR, KernelCont, MAX_PROC, PS_REC, Pcb, SCHED, Sched, State, TrapFrame};
use super::switch::{
    FX_TEMPLATE, FXSAVES, KCONTS, OUTCOME, apply_hw, drop_pcb, enter_scheduler, fxsave_current,
    leave_scheduler, schedule_next, set_current_slot, yield_block,
};

pub fn with_current_fds<T>(f: impl FnOnce(&mut FdTable) -> T) -> Option<T> {
    let mut s = SCHED.lock();
    let i = s.current?;
    Some(f(&mut s.procs[i].as_mut()?.fds))
}

pub fn current_pid() -> Option<u32> {
    let s = SCHED.lock();
    let i = s.current?;
    s.procs[i].as_ref().map(|p| p.pid)
}

pub fn has_runnable_other() -> bool {
    let s = SCHED.lock();
    let cur = s.current;
    s.procs.iter().enumerate().any(|(i, p)| {
        Some(i) != cur
            && p.as_ref()
                .is_some_and(|p| p.state == State::Runnable || p.state == State::Running)
    })
}

pub fn current_is_zombie() -> bool {
    let s = SCHED.lock();
    s.current
        .and_then(|i| s.procs[i].as_ref())
        .is_some_and(|p| p.state == State::Zombie)
}

pub fn wake_stdin() {
    let mut s = SCHED.lock();
    for p in s.procs.iter_mut().flatten() {
        if p.state == State::BlockedStdin {
            p.state = State::Runnable;
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
    for p in s.procs.iter_mut().flatten() {
        if p.state == State::BlockedPipeRead || p.state == State::BlockedPipeWrite {
            p.state = State::Runnable;
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
    {
        let mut s = SCHED.lock();
        s.exclusive = exclusive;
        s.next_pid = 1;
        s.init_pid = 0;
        s.current = None;
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
        let pid = s.procs[slot].as_ref().unwrap().pid;
        s.init_pid = pid;
        s.current = Some(slot);
        s.procs[slot].as_mut().unwrap().state = State::Running;
    }
    set_current_slot(slot);
    apply_hw(slot);
    unsafe {
        enter_scheduler();
    }
    match OUTCOME.load(Ordering::SeqCst) {
        1 => Outcome::Fault,
        _ => Outcome::Exited,
    }
}

pub(super) fn insert_pcb(
    image: Image,
    name: &str,
    parent: u32,
    argv: elfload::ArgvSetup,
    stdin_fd: u64,
    stdout_fd: u64,
) -> Option<usize> {
    let mut s = SCHED.lock();
    let Some(slot) = s.procs.iter().position(|p| p.is_none()) else {
        drop(s);
        elfload::unload(image);
        return None;
    };
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
    s.procs[slot] = Some(Pcb {
        pid,
        parent,
        state: State::Runnable,
        name: name_from_path(name),
        l4,
        trap,
        kcont_valid: false,
        user_rsp: 0,
        fds,
        image: Some(image),
        spawned_child: false,
        last_spawned: 0,
        fault: false,
    });
    unsafe {
        FXSAVES[slot] = FX_TEMPLATE;
        KCONTS[slot] = KernelCont {
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
    }
    if parent != 0 {
        if let Some(p) = s.procs.iter_mut().flatten().find(|p| p.pid == parent) {
            p.spawned_child = true;
            p.last_spawned = pid;
        }
    }
    Some(slot)
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
    if let Some(pid) = current_pid() {
        crate::win::drop_pid(pid);
    }
    let (is_init, exclusive, slot) = {
        let mut s = SCHED.lock();
        let slot = s.current.expect("user_exit");
        make_zombie_locked(&mut s, slot, outcome == Outcome::Fault);
        let is_init = s.procs[slot].as_ref().unwrap().pid == s.init_pid;
        (is_init, s.exclusive, slot)
    };
    if exclusive || is_init {
        leave_scheduler(outcome);
    }
    wake_parent_of_slot(slot);
    schedule_next();
}

pub(super) fn make_zombie_locked(s: &mut Sched, slot: usize, fault: bool) {
    if let Some(p) = s.procs[slot].as_mut() {
        p.fds.close_all();
        p.state = State::Zombie;
        p.fault = fault;
        p.kcont_valid = false;
    }
    wake_pipe_locked(s);
}

pub(super) fn wake_parent_of_slot(slot: usize) {
    let mut s = SCHED.lock();
    let parent = s.procs[slot].as_ref().map(|p| p.parent).unwrap_or(0);
    if parent == 0 {
        return;
    }
    for p in s.procs.iter_mut().flatten() {
        if p.pid == parent && p.state == State::BlockedWait {
            p.state = State::Runnable;
        }
    }
}

pub fn kill_foreground() -> bool {
    let mut s = SCHED.lock();
    let waiter = s
        .procs
        .iter()
        .position(|p| p.as_ref().is_some_and(|p| p.state == State::BlockedWait));
    let Some(w) = waiter else {
        return false;
    };
    let last = s.procs[w].as_ref().unwrap().last_spawned;
    let init = s.init_pid;
    if last == 0 || last == init {
        return false;
    }
    let Some(v) = s.procs.iter().position(|p| {
        p.as_ref()
            .is_some_and(|p| p.pid == last && p.state != State::Zombie)
    }) else {
        return false;
    };
    make_zombie_locked(&mut s, v, false);
    let parent = s.procs[v].as_ref().unwrap().parent;
    for p in s.procs.iter_mut().flatten() {
        if p.pid == parent && p.state == State::BlockedWait {
            p.state = State::Runnable;
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
        s.current
            .and_then(|i| s.procs[i].as_ref())
            .map(|p| p.pid)
            .unwrap_or(0)
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
    Some(SCHED.lock().procs[slot].as_ref().unwrap().pid)
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
        let z = s.procs.iter().position(|p| {
            p.as_ref()
                .is_some_and(|p| p.parent == 0 && p.state == State::Zombie)
        });
        let Some(i) = z else {
            return;
        };
        let pcb = s.procs[i].take().unwrap();
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
    let cur = match s.current {
        Some(i) => i,
        None => return WaitResult::NeverHadChildren,
    };
    let pid = s.procs[cur].as_ref().unwrap().pid;
    let spawned = s.procs[cur].as_ref().unwrap().spawned_child;
    let mut zombie = None;
    for (i, p) in s.procs.iter().enumerate() {
        if let Some(pcb) = p {
            if pcb.parent == pid && pcb.state == State::Zombie {
                zombie = Some(i);
                break;
            }
        }
    }
    if let Some(i) = zombie {
        let pcb = s.procs[i].take().unwrap();
        let cpid = pcb.pid;
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
    let cur = match s.current {
        Some(i) => i,
        None => return ERR,
    };
    let parent_pid = s.procs[cur].as_ref().unwrap().pid;
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
    if s.procs[v].as_ref().unwrap().state == State::Zombie {
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
