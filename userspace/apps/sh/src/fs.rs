use libcoeleo::{
    ERR, OPEN_CREATE, OPEN_READ, OPEN_TRUNC, OPEN_WRITE, close, dirent_is_dir, dirent_name, open,
    read, readdir, sync, unlink, write,
};

use crate::cwd::{resolve, Cwd};

const WRITE_PROMPT: &[u8] = b"write>";
const WRITE_CAP: usize = 4096;

pub struct WriteJob {
    path: [u8; 256],
    path_len: usize,
    body: [u8; WRITE_CAP],
    body_len: usize,
}

impl WriteJob {
    fn path_str(&self) -> &str {
        core::str::from_utf8(&self.path[..self.path_len]).unwrap_or("")
    }
}

pub enum WriteNext {
    Stay,
    Done,
}

pub fn submit_write(job: &mut WriteJob, line: &str) -> WriteNext {
    if line == "." {
        let path = job.path_str();
        let fd = open(path, OPEN_CREATE | OPEN_WRITE | OPEN_TRUNC);
        if fd == ERR {
            let _ = write(1, b"write: failed\n");
            return WriteNext::Done;
        }
        let mut off = 0usize;
        while off < job.body_len {
            let chunk = &job.body[off..job.body_len];
            let r = write(fd, chunk);
            if r == ERR {
                let _ = close(fd);
                let _ = write(1, b"write: failed\n");
                return WriteNext::Done;
            }
            off += r as usize;
        }
        let _ = close(fd);
        return WriteNext::Done;
    }
    if job.body_len + line.len() + 1 > WRITE_CAP {
        let _ = write(1, b"write: too large\n");
        return WriteNext::Done;
    }
    job.body[job.body_len..job.body_len + line.len()].copy_from_slice(line.as_bytes());
    job.body_len += line.len();
    job.body[job.body_len] = b'\n';
    job.body_len += 1;
    let _ = write(1, WRITE_PROMPT);
    WriteNext::Stay
}

pub fn cmd_touch(cwd: &Cwd, args: &str) {
    if args.is_empty() {
        let _ = write(1, b"touch: missing path\n");
        return;
    }
    let mut abs = [0u8; 256];
    let path = resolve(cwd.as_str(), args, &mut abs);
    let fd = open(path, OPEN_CREATE | OPEN_WRITE);
    if fd == ERR {
        let _ = write(1, b"touch: failed\n");
        return;
    }
    let _ = close(fd);
}

pub fn cmd_write_enter(cwd: &Cwd, args: &str, writing: &mut Option<WriteJob>) {
    let path_arg = args.split_whitespace().next().unwrap_or("");
    if path_arg.is_empty() {
        let _ = write(1, b"write: missing path\n");
        return;
    }
    let mut abs = [0u8; 256];
    let path = resolve(cwd.as_str(), path_arg, &mut abs);
    let mut job = WriteJob {
        path: [0; 256],
        path_len: 0,
        body: [0; WRITE_CAP],
        body_len: 0,
    };
    let n = path.len().min(job.path.len());
    job.path[..n].copy_from_slice(&path.as_bytes()[..n]);
    job.path_len = n;
    *writing = Some(job);
    let _ = write(1, WRITE_PROMPT);
}

pub fn cmd_rm(cwd: &Cwd, args: &str) {
    if args.is_empty() {
        let _ = write(1, b"rm: missing path\n");
        return;
    }
    let mut abs = [0u8; 256];
    let path = resolve(cwd.as_str(), args, &mut abs);
    if unlink(path) == ERR {
        let _ = write(1, b"rm: failed\n");
    }
}

pub fn cmd_sync() {
    if sync() == ERR {
        let _ = write(1, b"sync: failed\n");
    }
}

pub fn cmd_ls(cwd: &Cwd, args: &str) {
    let mut abs = [0u8; 256];
    let path = if args.is_empty() {
        cwd.as_str()
    } else {
        resolve(cwd.as_str(), args, &mut abs)
    };
    let fd = open(path, OPEN_READ);
    if fd == ERR {
        let _ = write(1, b"ls: not found\n");
        return;
    }
    loop {
        let mut ent = [0u8; 64];
        let r = readdir(fd, &mut ent);
        if r == 0 || r == ERR {
            break;
        }
        let name = dirent_name(&ent);
        let _ = write(1, name.as_bytes());
        if dirent_is_dir(&ent) {
            let _ = write(1, b"/");
        }
        let _ = write(1, b"\n");
    }
    let _ = close(fd);
}

pub fn cmd_cat(cwd: &Cwd, args: &str) {
    if args.is_empty() {
        let _ = write(1, b"cat: missing path\n");
        return;
    }
    let mut abs = [0u8; 256];
    let path = resolve(cwd.as_str(), args, &mut abs);
    let fd = open(path, OPEN_READ);
    if fd == ERR {
        let _ = write(1, b"cat: not found\n");
        return;
    }
    let mut probe = [0u8; 64];
    if readdir(fd, &mut probe) != ERR {
        let _ = close(fd);
        let _ = write(1, b"cat: is a directory\n");
        return;
    }
    loop {
        let mut buf = [0u8; 512];
        let r = read(fd, &mut buf);
        if r == 0 {
            break;
        }
        if r == ERR {
            let _ = write(1, b"cat: read failed\n");
            break;
        }
        let _ = write(1, &buf[..r as usize]);
    }
    let _ = close(fd);
}

pub fn cmd_cd(cwd: &mut Cwd, args: &str) {
    if args.is_empty() {
        let _ = write(1, b"cd: missing path\n");
        return;
    }
    let mut abs = [0u8; 256];
    let path = resolve(cwd.as_str(), args, &mut abs);
    let fd = open(path, OPEN_READ);
    if fd == ERR {
        let _ = write(1, b"cd: not found\n");
        return;
    }
    let mut ent = [0u8; 64];
    let r = readdir(fd, &mut ent);
    let _ = close(fd);
    if r == ERR {
        let _ = write(1, b"cd: not a directory\n");
        return;
    }
    cwd.set(path);
}
