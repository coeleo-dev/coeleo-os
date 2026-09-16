use libcoeleo::{
    DIRENT_SIZE, ERR, OPEN_READ, close, dirent_is_dir, dirent_name, open, readdir, write,
};

use crate::cwd::{Cwd, resolve};
use crate::highlight::BUILTINS;
use crate::line::LINE_CAP;

const MAX_CAND: usize = 32;
const NAME_CAP: usize = 64;

#[derive(Clone, Copy)]
struct Cand {
    buf: [u8; NAME_CAP],
    len: usize,
    is_dir: bool,
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
    let raw_token = match core::str::from_utf8(&line[tok_at..tok_at + tok_len]) {
        Ok(s) => s,
        Err(_) => return,
    };

    let mut cands: [Cand; MAX_CAND] = [const {
        Cand {
            buf: [0; NAME_CAP],
            len: 0,
            is_dir: false,
        }
    }; MAX_CAND];
    let mut nc = 0usize;

    // Check if path has a directory component (e.g. "docs/he" or "/bin/he")
    let (search_dir, prefix, path_prefix) = if let Some(idx) = raw_token.rfind('/') {
        let (dir_part, file_part) = raw_token.split_at(idx + 1);
        (dir_part, file_part, dir_part)
    } else {
        ("", raw_token, "")
    };

    if path_prefix.is_empty() && tok_at == 0 {
        // Complete builtins if first word
        for &b in BUILTINS {
            if b.starts_with(prefix) && !already(&cands, nc, b.as_bytes()) && nc < MAX_CAND {
                let bn = b.len().min(NAME_CAP);
                cands[nc].buf[..bn].copy_from_slice(&b.as_bytes()[..bn]);
                cands[nc].len = bn;
                cands[nc].is_dir = false;
                nc += 1;
            }
        }
        collect_dir("/bin", prefix, "", &mut cands, &mut nc);
        collect_dir(cwd.as_str(), prefix, "", &mut cands, &mut nc);
    } else if path_prefix.is_empty() {
        collect_dir(cwd.as_str(), prefix, "", &mut cands, &mut nc);
        collect_dir("/bin", prefix, "", &mut cands, &mut nc);
    } else {
        let mut abs = [0u8; 256];
        let target_dir = resolve(cwd.as_str(), search_dir, &mut abs);
        collect_dir(target_dir, prefix, path_prefix, &mut cands, &mut nc);
    }

    match nc {
        0 => {}
        1 => replace_token(line, n, tok_at, cands[0].as_bytes()),
        _ => {
            // Find Longest Common Prefix (LCP)
            let lcp = common_prefix_len(&cands[..nc]);
            if lcp > tok_len && lcp <= NAME_CAP {
                let fill = &cands[0].buf[..lcp];
                replace_token(line, n, tok_at, fill);
            }
            let _ = write(1, b"\n");
            for i in 0..nc {
                if i > 0 {
                    let _ = write(1, b"  ");
                }
                if cands[i].is_dir {
                    let _ = write(1, b"\x1b[1;34m");
                    let _ = write(1, cands[i].as_bytes());
                    let _ = write(1, b"\x1b[0m");
                } else {
                    let _ = write(1, cands[i].as_bytes());
                }
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

fn collect_dir(
    dir_path: &str,
    prefix: &str,
    path_prefix: &str,
    cands: &mut [Cand; MAX_CAND],
    nc: &mut usize,
) {
    let fd = open(dir_path, OPEN_READ);
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
        let name = dirent_name(&ent);
        if name.is_empty() || name == "." || name == ".." || !name.starts_with(prefix) {
            continue;
        }
        let is_dir = dirent_is_dir(&ent);
        let needed_len = path_prefix.len() + name.len() + if is_dir { 1 } else { 0 };
        if needed_len > NAME_CAP {
            continue;
        }

        let i = *nc;
        let mut p = 0usize;
        if !path_prefix.is_empty() {
            cands[i].buf[p..p + path_prefix.len()].copy_from_slice(path_prefix.as_bytes());
            p += path_prefix.len();
        }
        cands[i].buf[p..p + name.len()].copy_from_slice(name.as_bytes());
        p += name.len();
        if is_dir {
            cands[i].buf[p] = b'/';
            p += 1;
        }
        cands[i].len = p;
        cands[i].is_dir = is_dir;

        if already(cands, *nc, &cands[i].buf[..p]) {
            continue;
        }
        *nc += 1;
    }
    let _ = close(fd);
}

fn already(cands: &[Cand; MAX_CAND], nc: usize, name: &[u8]) -> bool {
    cands[..nc].iter().any(|c| c.as_bytes() == name)
}

fn common_prefix_len(cands: &[Cand]) -> usize {
    if cands.is_empty() {
        return 0;
    }
    let first = cands[0].as_bytes();
    let mut len = first.len();
    for c in &cands[1..] {
        let b = c.as_bytes();
        let limit = len.min(b.len());
        let mut same = 0usize;
        while same < limit && first[same] == b[same] {
            same += 1;
        }
        len = same;
        if len == 0 {
            break;
        }
    }
    len
}
