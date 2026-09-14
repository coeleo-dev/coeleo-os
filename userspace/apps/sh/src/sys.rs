use libcoeleo::{
    ERR, INFO_DISK, INFO_MEM, clipboard_get, clipboard_set, clock_ms, kill, sysinfo, write,
};

use crate::err;

pub fn cmd_kill(args: &str) {
    let pid_str = args.split_whitespace().next().unwrap_or("");
    if pid_str.is_empty() {
        err::err("kill", "PID do processo ausente");
        err::usage("kill", "<pid>");
        return;
    }
    let Ok(pid) = parse_u64(pid_str) else {
        err::err_target("kill", pid_str, "PID inválido (esperado número)");
        return;
    };
    let r = kill(pid);
    if r == ERR {
        err::err_target("kill", pid_str, "processo não encontrado ou falha ao encerrar");
    } else {
        let _ = write(1, b"processo ");
        let _ = write(1, pid_str.as_bytes());
        let _ = write(1, b" encerrado com sucesso\n");
    }
}

pub fn cmd_disks() {
    let mut buf = [0u8; 1024];
    let r = sysinfo(INFO_DISK, &mut buf);
    if r == ERR || r == 0 {
        err::warn("nenhum dispositivo de armazenamento detectado");
        return;
    }
    let _ = write(1, &buf[..r as usize]);
}

pub fn cmd_uname(args: &str) {
    let all = args.split_whitespace().any(|a| a == "-a" || a == "--all");
    if all {
        let _ = write(1, b"Coeleo OS 0.2.0 x86_64 SMP APIC (Clang/Rust no_std)\n");
    } else {
        let _ = write(1, b"Coeleo\n");
    }
}

pub fn cmd_free(args: &str) {
    let mut buf = [0u8; 512];
    let r = sysinfo(INFO_MEM, &mut buf);
    if r == ERR || r == 0 {
        err::err("free", "não foi possível ler estatísticas de memória");
        return;
    }
    let text = core::str::from_utf8(&buf[..r as usize]).unwrap_or("");
    let human = args.split_whitespace().any(|a| a == "-h" || a == "--human");

    let _ = write(1, b"\x1b[1mMem\xc3\xb3ria do Sistema (PMM / Heap):\x1b[0m\n");
    if human {
        let _ = write(1, b"  ");
        let _ = write(1, text.as_bytes());
    } else {
        let _ = write(1, b"  ");
        let _ = write(1, text.as_bytes());
    }
}

pub fn cmd_df(_args: &str) {
    let mut buf = [0u8; 1024];
    let r = sysinfo(INFO_DISK, &mut buf);
    if r == ERR || r == 0 {
        err::err("df", "não foi possível inspecionar discos");
        return;
    }
    let _ = write(1, b"\x1b[1mSistemas de Arquivos Montados:\x1b[0m\n");
    let _ = write(1, &buf[..r as usize]);
}

pub fn cmd_sleep(args: &str) {
    let sec_str = args.split_whitespace().next().unwrap_or("");
    if sec_str.is_empty() {
        err::err("sleep", "tempo em segundos ausente");
        err::usage("sleep", "<segundos>");
        return;
    }
    let Ok(secs) = parse_u64(sec_str) else {
        err::err_target("sleep", sec_str, "tempo inválido (esperado número)");
        return;
    };
    if secs == 0 {
        return;
    }
    let ms = secs.saturating_mul(1000);
    let start = clock_ms();
    while clock_ms().saturating_sub(start) < ms {
        // Cooperative yield via tiny read or nop
        let mut dummy = [0u8; 1];
        let _ = libcoeleo::sysinfo(INFO_MEM, &mut dummy);
    }
}

pub fn cmd_clip(args: &str) {
    let mut parts = args.split_whitespace();
    let sub = parts.next().unwrap_or("");
    match sub {
        "get" => {
            let mut buf = [0u8; 4096];
            if let Some(len) = clipboard_get(&mut buf) {
                if len == 0 {
                    let _ = write(1, b"(\xc3\xa1rea de transfer\xc3\xaancia vazia)\n");
                } else {
                    let _ = write(1, &buf[..len.min(buf.len())]);
                    let _ = write(1, b"\n");
                }
            } else {
                let _ = write(1, b"(\xc3\xa1rea de transfer\xc3\xaancia vazia)\n");
            }
        }
        "set" => {
            let rest = args.strip_prefix("set").unwrap_or("").trim_start();
            if rest.is_empty() {
                err::err("clip set", "informe o texto a ser copiado");
                err::usage("clip", "set <texto>");
                return;
            }
            clipboard_set(rest.as_bytes());
            let _ = write(1, b"copiado para a \xc3\xa1rea de transfer\xc3\xaancia\n");
        }
        _ => {
            err::err("clip", "subcomando inválido");
            err::usage("clip", "get | set <texto>");
        }
    }
}

fn parse_u64(s: &str) -> Result<u64, ()> {
    let mut val = 0u64;
    for b in s.bytes() {
        if !b.is_ascii_digit() {
            return Err(());
        }
        val = val.saturating_mul(10).saturating_add((b - b'0') as u64);
    }
    Ok(val)
}
