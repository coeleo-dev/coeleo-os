use libcoeleo::{
    ERR, OPEN_CREATE, OPEN_READ, OPEN_TRUNC, OPEN_WRITE, SPAWN_FD_DEFAULT, close, kill, open, pipe,
    spawn_ex, wait, write,
};

use crate::cwd::{Cwd, resolve};

const MAX_STAGES: usize = 4;
const MAX_WORDS: usize = 8;

#[derive(Clone, Copy)]
struct Stage<'a> {
    words: [&'a str; MAX_WORDS],
    nwords: usize,
    stdin_path: Option<&'a str>,
    stdout_path: Option<&'a str>,
}

impl<'a> Stage<'a> {
    fn empty() -> Self {
        Self {
            words: [""; MAX_WORDS],
            nwords: 0,
            stdin_path: None,
            stdout_path: None,
        }
    }
}

fn is_meta(t: &str) -> bool {
    t == "|" || t == ">" || t == "<"
}

pub fn has_meta(line: &str) -> bool {
    line.split_whitespace().any(is_meta)
}

pub fn run_line(cwd: &Cwd, line: &str) {
    let (stages, ns) = match parse(line) {
        Ok(v) => v,
        Err(msg) => {
            let _ = write(1, msg);
            return;
        }
    };
    let npipes = ns.saturating_sub(1);
    let mut pipes = [[ERR; 2]; MAX_STAGES - 1];
    for i in 0..npipes {
        let mut fds = [0u32; 2];
        if pipe(&mut fds) == ERR {
            close_pipes(&pipes, i);
            let _ = write(1, b"pipe: failed\n");
            return;
        }
        pipes[i][0] = fds[0] as u64;
        pipes[i][1] = fds[1] as u64;
    }
    let mut extra = [ERR; 8];
    let mut nextra = 0usize;
    let mut pids = [ERR; MAX_STAGES];
    let mut npids = 0usize;
    for i in 0..ns {
        let st = &stages[i];
        let stdin = if let Some(p) = st.stdin_path {
            match open_redir(cwd, p, OPEN_READ, &mut extra, &mut nextra) {
                Some(fd) => fd,
                None => {
                    abort_pipeline(&pids, npids, &pipes, npipes, &extra, nextra);
                    return;
                }
            }
        } else if i > 0 {
            pipes[i - 1][0]
        } else {
            SPAWN_FD_DEFAULT
        };
        let stdout = if let Some(p) = st.stdout_path {
            match open_redir(
                cwd,
                p,
                OPEN_WRITE | OPEN_CREATE | OPEN_TRUNC,
                &mut extra,
                &mut nextra,
            ) {
                Some(fd) => fd,
                None => {
                    abort_pipeline(&pids, npids, &pipes, npipes, &extra, nextra);
                    return;
                }
            }
        } else if i + 1 < ns {
            pipes[i][1]
        } else {
            SPAWN_FD_DEFAULT
        };
        let mut blob = [0u8; 1024];
        let Some(blen) = pack_words(cwd.as_str(), &st.words[..st.nwords], &mut blob) else {
            abort_pipeline(&pids, npids, &pipes, npipes, &extra, nextra);
            let _ = write(1, b"spawn: failed\n");
            return;
        };
        let pid = spawn_elf(cwd, st.words[0], &blob[..blen], stdin, stdout);
        if pid == ERR {
            abort_pipeline(&pids, npids, &pipes, npipes, &extra, nextra);
            let _ = write(1, b"spawn: failed\n");
            return;
        }
        pids[npids] = pid;
        npids += 1;
    }
    close_pipes(&pipes, npipes);
    close_fds(&extra, nextra);
    for _ in 0..npids {
        let _ = wait();
    }
}

fn open_redir(
    cwd: &Cwd,
    arg: &str,
    flags: u64,
    extra: &mut [u64; 8],
    nextra: &mut usize,
) -> Option<u64> {
    let mut abs = [0u8; 256];
    let path = resolve(cwd.as_str(), arg, &mut abs);
    let fd = open(path, flags);
    if fd == ERR {
        let _ = write(1, b"open: failed\n");
        return None;
    }
    if *nextra >= extra.len() {
        let _ = close(fd);
        return None;
    }
    extra[*nextra] = fd;
    *nextra += 1;
    Some(fd)
}

fn abort_pipeline(
    pids: &[u64; MAX_STAGES],
    npids: usize,
    pipes: &[[u64; 2]; MAX_STAGES - 1],
    npipes: usize,
    extra: &[u64; 8],
    nextra: usize,
) {
    close_pipes(pipes, npipes);
    close_fds(extra, nextra);
    for i in 0..npids {
        let _ = kill(pids[i]);
    }
    for _ in 0..npids {
        let _ = wait();
    }
}

fn close_pipes(pipes: &[[u64; 2]; MAX_STAGES - 1], n: usize) {
    for i in 0..n {
        if pipes[i][0] != ERR {
            let _ = close(pipes[i][0]);
        }
        if pipes[i][1] != ERR {
            let _ = close(pipes[i][1]);
        }
    }
}

fn close_fds(fds: &[u64; 8], n: usize) {
    for i in 0..n {
        if fds[i] != ERR {
            let _ = close(fds[i]);
        }
    }
}

fn pack_words(cwd: &str, words: &[&str], out: &mut [u8; 1024]) -> Option<usize> {
    let mut n = 0usize;
    for (i, w) in words.iter().enumerate() {
        let mut abs = [0u8; 256];
        let s = if i == 0 {
            *w
        } else {
            resolve(cwd, w, &mut abs)
        };
        if n + s.len() + 1 > out.len() {
            return None;
        }
        out[n..n + s.len()].copy_from_slice(s.as_bytes());
        n += s.len();
        out[n] = 0;
        n += 1;
    }
    if n == 0 { None } else { Some(n) }
}

fn spawn_elf(cwd: &Cwd, name: &str, blob: &[u8], stdin: u64, stdout: u64) -> u64 {
    if name.contains('/') {
        let mut abs = [0u8; 256];
        let path = resolve(cwd.as_str(), name, &mut abs);
        return spawn_ex(path, blob, stdin, stdout);
    }
    if name.is_empty() || name.len() > ::pkg::NAME_MAX {
        return ERR;
    }
    let mut bin = [0u8; 16];
    bin[..5].copy_from_slice(b"/bin/");
    bin[5..5 + name.len()].copy_from_slice(name.as_bytes());
    let bin = core::str::from_utf8(&bin[..5 + name.len()]).unwrap_or("");
    let r = spawn_ex(bin, blob, stdin, stdout);
    if r != ERR {
        return r;
    }
    spawn_ex(name, blob, stdin, stdout)
}

fn parse(line: &str) -> Result<([Stage<'_>; MAX_STAGES], usize), &'static [u8]> {
    let mut stages = [Stage::empty(); MAX_STAGES];
    let mut ns = 1usize;
    let mut expect_in = false;
    let mut expect_out = false;
    for tok in line.split_whitespace() {
        if expect_in {
            if is_meta(tok) {
                return Err(b"sh: parse error\n");
            }
            if stages[ns - 1].stdin_path.is_some() {
                return Err(b"sh: parse error\n");
            }
            stages[ns - 1].stdin_path = Some(tok);
            expect_in = false;
            continue;
        }
        if expect_out {
            if is_meta(tok) {
                return Err(b"sh: parse error\n");
            }
            if stages[ns - 1].stdout_path.is_some() {
                return Err(b"sh: parse error\n");
            }
            stages[ns - 1].stdout_path = Some(tok);
            expect_out = false;
            continue;
        }
        match tok {
            "|" => {
                if stages[ns - 1].nwords == 0 {
                    return Err(b"sh: parse error\n");
                }
                if ns >= MAX_STAGES {
                    return Err(b"sh: parse error\n");
                }
                ns += 1;
            }
            "<" => expect_in = true,
            ">" => expect_out = true,
            _ => {
                let st = &mut stages[ns - 1];
                if st.nwords >= MAX_WORDS {
                    return Err(b"sh: parse error\n");
                }
                st.words[st.nwords] = tok;
                st.nwords += 1;
            }
        }
    }
    if expect_in || expect_out || stages[ns - 1].nwords == 0 {
        return Err(b"sh: parse error\n");
    }
    for i in 0..ns {
        if i > 0 && stages[i].stdin_path.is_some() {
            return Err(b"sh: parse error\n");
        }
        if i + 1 < ns && stages[i].stdout_path.is_some() {
            return Err(b"sh: parse error\n");
        }
    }
    Ok((stages, ns))
}
