//! Four 4 KiB pipe rings. Do not lock `SCHED` from here (deadlock with `with_fds`).

use spin::Mutex;

pub const PIPE_MAX: usize = 4;
pub const PIPE_CAP: usize = 4096;

struct Pipe {
    used: bool,
    buf: [u8; PIPE_CAP],
    head: usize,
    len: usize,
    n_readers: u8,
    n_writers: u8,
}

const fn empty() -> Pipe {
    Pipe {
        used: false,
        buf: [0; PIPE_CAP],
        head: 0,
        len: 0,
        n_readers: 0,
        n_writers: 0,
    }
}

static PIPES: Mutex<[Pipe; PIPE_MAX]> = Mutex::new([empty(), empty(), empty(), empty()]);

pub fn alloc() -> Option<u8> {
    let mut pipes = PIPES.lock();
    for (i, p) in pipes.iter_mut().enumerate() {
        if !p.used {
            *p = Pipe {
                used: true,
                buf: [0; PIPE_CAP],
                head: 0,
                len: 0,
                n_readers: 1,
                n_writers: 1,
            };
            return Some(i as u8);
        }
    }
    None
}

pub fn clone_end(id: u8, write: bool) -> Result<(), ()> {
    let mut pipes = PIPES.lock();
    let p = pipes.get_mut(id as usize).ok_or(())?;
    if !p.used {
        return Err(());
    }
    if write {
        p.n_writers = p.n_writers.checked_add(1).ok_or(())?;
    } else {
        p.n_readers = p.n_readers.checked_add(1).ok_or(())?;
    }
    Ok(())
}

pub fn drop_end(id: u8, write: bool) {
    let mut pipes = PIPES.lock();
    let Some(p) = pipes.get_mut(id as usize) else {
        return;
    };
    if !p.used {
        return;
    }
    if write {
        p.n_writers = p.n_writers.saturating_sub(1);
    } else {
        p.n_readers = p.n_readers.saturating_sub(1);
    }
    if p.n_readers == 0 && p.n_writers == 0 {
        *p = empty();
    }
}

pub enum Read {
    Data(usize),
    Eof,
    WouldBlock,
}

pub enum Write {
    Data(usize),
    WouldBlock,
    Broken,
}

pub fn read(id: u8, dst: &mut [u8]) -> Read {
    if dst.is_empty() {
        return Read::Data(0);
    }
    let mut pipes = PIPES.lock();
    let Some(p) = pipes.get_mut(id as usize) else {
        return Read::Eof;
    };
    if !p.used {
        return Read::Eof;
    }
    if p.len == 0 {
        return if p.n_writers == 0 {
            Read::Eof
        } else {
            Read::WouldBlock
        };
    }
    let n = dst.len().min(p.len);
    for i in 0..n {
        dst[i] = p.buf[(p.head + i) % PIPE_CAP];
    }
    p.head = (p.head + n) % PIPE_CAP;
    p.len -= n;
    Read::Data(n)
}

pub fn write(id: u8, src: &[u8]) -> Write {
    if src.is_empty() {
        return Write::Data(0);
    }
    let mut pipes = PIPES.lock();
    let Some(p) = pipes.get_mut(id as usize) else {
        return Write::Broken;
    };
    if !p.used {
        return Write::Broken;
    }
    if p.n_readers == 0 {
        return Write::Broken;
    }
    if p.len == PIPE_CAP {
        return Write::WouldBlock;
    }
    let n = src.len().min(PIPE_CAP - p.len);
    let tail = (p.head + p.len) % PIPE_CAP;
    for i in 0..n {
        p.buf[(tail + i) % PIPE_CAP] = src[i];
    }
    p.len += n;
    Write::Data(n)
}
