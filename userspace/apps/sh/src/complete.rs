use libcoeleo::{
    DIRENT_SIZE, ERR, OPEN_READ, close, dirent_is_dir, dirent_name, open, readdir, write,
};

use crate::cwd::Cwd;
use crate::line::LINE_CAP;

const MAX_CAND: usize = 32;
const NAME_CAP: usize = 64;

struct Cand {
    buf: [u8; NAME_CAP],
    len: usize,
}

impl Cand {
    fn as_bytes(&self) -> &[u8] {
        &self.buf[..self.len]
    }
}

pub fn apply(line: &mut [u8], n: &mut usize, cwd: &Cwd, prompt: &[u8]) {
    let (tok_at, tok_len) = token_span(line, *n);
    if tok_len == 0 {
        return;
    }
    let prefix = match core::str::from_utf8(&line[tok_at..tok_at + tok_len]) {
        Ok(s) => s,
        Err(_) => return,
    };
    let mut cands: [Cand; MAX_CAND] = [const { Cand { buf: [0; NAME_CAP], len: 0 } }; MAX_CAND];
    let mut nc = 0usize;
    collect_dir("/bin", prefix, &mut cands, &mut nc);
    collect_dir(cwd.as_str(), prefix, &mut cands, &mut nc);
    match nc {
        0 => {}
        1 => replace_token(line, n, tok_at, cands[0].as_bytes()),
        _ => {
            let _ = write(1, b"\n");
            for i in 0..nc {
                if i > 0 {
                    let _ = write(1, b" ");
                }
                let _ = write(1, cands[i].as_bytes());
            }
            let _ = write(1, b"\n");
            let _ = write(1, prompt);
            if *n > 0 {
                let _ = write(1, &line[..*n]);
            }
        }
    }
}

fn token_span(line: &[u8], n: usize) -> (usize, usize) {
    if n == 0 {
        return (0, 0);
    }
    let mut i = n;
    while i > 0 && line[i - 1] != b' ' {
        i -= 1;
    }
    (i, n - i)
}

fn replace_token(line: &mut [u8], n: &mut usize, tok_at: usize, name: &[u8]) {
    if tok_at + name.len() > line.len() || tok_at + name.len() > LINE_CAP {
        return;
    }
    let old = *n - tok_at;
    if name.len() < old {
        return;
    }
    line[tok_at..tok_at + name.len()].copy_from_slice(name);
    *n = tok_at + name.len();
    let _ = write(1, &name[old..]);
}

fn collect_dir(path: &str, prefix: &str, cands: &mut [Cand; MAX_CAND], nc: &mut usize) {
    let fd = open(path, OPEN_READ);
    if fd == ERR {
        return;
    }
    loop {
        if *nc >= MAX_CAND {
            break;
        }
        let mut ent = [0u8; DIRENT_SIZE];
        let r = readdir(fd, &mut ent);
        if r == 0 || r == ERR {
            break;
        }
        if dirent_is_dir(&ent) {
            continue;
        }
        let name = dirent_name(&ent);
        if name.is_empty() || name.len() > NAME_CAP || !name.starts_with(prefix) {
            continue;
        }
        if already(cands, *nc, name.as_bytes()) {
            continue;
        }
        let i = *nc;
        cands[i].buf[..name.len()].copy_from_slice(name.as_bytes());
        cands[i].len = name.len();
        *nc += 1;
    }
    let _ = close(fd);
}

fn already(cands: &[Cand; MAX_CAND], nc: usize, name: &[u8]) -> bool {
    cands[..nc].iter().any(|c| c.as_bytes() == name)
}
