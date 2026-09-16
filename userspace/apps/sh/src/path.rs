use libcoeleo::{ERR, SPAWN_FD_DEFAULT, spawn, spawn_ex, wait, write};

use crate::cwd::{Cwd, resolve};

pub fn cmd_path(cmd: &str, args: &str) {
    if cmd.is_empty() || cmd.contains('/') || cmd.len() > ::pkg::NAME_MAX {
        let _ = write(1, b"not found\n");
        return;
    }
    let mut blob = [0u8; 1024];
    let blen = pack_argv(cmd, args, &mut blob);
    let mut bin = [0u8; 16];
    bin[..5].copy_from_slice(b"/bin/");
    bin[5..5 + cmd.len()].copy_from_slice(cmd.as_bytes());
    let bin = core::str::from_utf8(&bin[..5 + cmd.len()]).unwrap_or("");
    if spawn_blob(bin, &blob[..blen]) != ERR {
        let _ = wait();
        return;
    }
    // Kernel cwd is `/`; phase 10 still types `hello` with the ELF at `/hello`.
    if spawn_blob(cmd, &blob[..blen]) != ERR {
        let _ = wait();
        return;
    }
    let _ = write(1, b"not found\n");
    if let Some(sug) = crate::err::suggest_command(cmd, crate::highlight::BUILTINS) {
        let _ = write(1, b"\x1b[1;36mdica:\x1b[0m voc\xc3\xaa quis dizer '");
        let _ = write(1, sug.as_bytes());
        let _ = write(1, b"'?\n");
    }
}

pub fn cmd_run(cwd: &Cwd, args: &str) {
    let mut it = args.split_whitespace();
    let path_arg = it.next().unwrap_or("");
    if path_arg.is_empty() {
        let _ = write(1, b"run: missing path\n");
        return;
    }
    let rest = skip_first_word(args);
    let mut abs = [0u8; 256];
    let path = resolve(cwd.as_str(), path_arg, &mut abs);
    let mut blob = [0u8; 1024];
    let blen = pack_argv(path_arg, rest, &mut blob);
    if spawn_blob(path, &blob[..blen]) == ERR {
        let _ = write(1, b"run: not found\n");
        return;
    }
    let _ = wait();
}

pub fn cmd_spawn_wait(path: &str) {
    if spawn(path) == ERR {
        return;
    }
    let _ = wait();
}

fn spawn_blob(path: &str, blob: &[u8]) -> u64 {
    if blob.is_empty() {
        spawn(path)
    } else {
        spawn_ex(path, blob, SPAWN_FD_DEFAULT, SPAWN_FD_DEFAULT)
    }
}

fn pack_argv(cmd: &str, args: &str, out: &mut [u8; 1024]) -> usize {
    if args.is_empty() {
        return 0;
    }
    let mut n = 0usize;
    for w in core::iter::once(cmd).chain(args.split_whitespace()) {
        if n + w.len() + 1 > out.len() {
            return 0;
        }
        out[n..n + w.len()].copy_from_slice(w.as_bytes());
        n += w.len();
        out[n] = 0;
        n += 1;
    }
    n
}

fn skip_first_word(s: &str) -> &str {
    let s = s.trim_start();
    match s.split_once(char::is_whitespace) {
        Some((_, rest)) => rest.trim_start(),
        None => "",
    }
}
