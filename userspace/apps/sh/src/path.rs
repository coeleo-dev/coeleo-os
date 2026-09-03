use libcoeleo::{ERR, spawn, wait, write};

use crate::cwd::{resolve, Cwd};

pub fn cmd_path(cmd: &str) {
    if cmd.is_empty() || cmd.contains('/') || cmd.len() > ::pkg::NAME_MAX {
        let _ = write(1, b"not found\n");
        return;
    }
    let mut bin = [0u8; 16];
    bin[..5].copy_from_slice(b"/bin/");
    bin[5..5 + cmd.len()].copy_from_slice(cmd.as_bytes());
    let bin = core::str::from_utf8(&bin[..5 + cmd.len()]).unwrap_or("");
    if spawn(bin) != ERR {
        let _ = wait();
        return;
    }
    // Kernel cwd is `/`; phase 10 still types `hello` with the ELF at `/hello`.
    if spawn(cmd) != ERR {
        let _ = wait();
        return;
    }
    let _ = write(1, b"not found\n");
}

pub fn cmd_run(cwd: &Cwd, args: &str) {
    let path_arg = args.split_whitespace().next().unwrap_or("");
    if path_arg.is_empty() {
        let _ = write(1, b"run: missing path\n");
        return;
    }
    let mut abs = [0u8; 256];
    let path = resolve(cwd.as_str(), path_arg, &mut abs);
    if spawn(path) == ERR {
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
