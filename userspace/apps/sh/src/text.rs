use libcoeleo::{DIRENT_SIZE, ERR, OPEN_READ, close, open, read, readdir, write};

use crate::cwd::{Cwd, resolve};
use crate::err;

pub fn cmd_head(cwd: &Cwd, args: &str) {
    let (lines_limit, path_arg) = parse_line_limit(args, 10);
    if path_arg.is_empty() {
        err::err("head", "missing file path");
        err::usage("head", "[-n lines] <file>");
        return;
    }
    let mut abs = [0u8; 256];
    let path = resolve(cwd.as_str(), path_arg, &mut abs);
    let fd = open(path, OPEN_READ);
    if fd == ERR {
        let _ = write(1, b"head: not found\n");
        err::err_target("head", path_arg, "file not found");
        return;
    }
    let mut probe = [0u8; DIRENT_SIZE];
    if readdir(fd, &mut probe) != ERR {
        let _ = close(fd);
        err::err_target("head", path_arg, "is a directory");
        return;
    }

    let mut printed_lines = 0usize;
    let mut buf = [0u8; 512];
    loop {
        let r = read(fd, &mut buf);
        if r == 0 || r == ERR {
            break;
        }
        let chunk = &buf[..r as usize];
        let mut start = 0usize;
        for (i, &b) in chunk.iter().enumerate() {
            if b == b'\n' {
                let _ = write(1, &chunk[start..=i]);
                start = i + 1;
                printed_lines += 1;
                if printed_lines >= lines_limit {
                    break;
                }
            }
        }
        if printed_lines >= lines_limit {
            break;
        }
        if start < chunk.len() {
            let _ = write(1, &chunk[start..]);
        }
    }
    let _ = close(fd);
}

pub fn cmd_tail(cwd: &Cwd, args: &str) {
    let (lines_limit, path_arg) = parse_line_limit(args, 10);
    if path_arg.is_empty() {
        err::err("tail", "missing file path");
        err::usage("tail", "[-n lines] <file>");
        return;
    }
    let mut abs = [0u8; 256];
    let path = resolve(cwd.as_str(), path_arg, &mut abs);
    let fd = open(path, OPEN_READ);
    if fd == ERR {
        let _ = write(1, b"tail: not found\n");
        err::err_target("tail", path_arg, "file not found");
        return;
    }
    let mut probe = [0u8; DIRENT_SIZE];
    if readdir(fd, &mut probe) != ERR {
        let _ = close(fd);
        err::err_target("tail", path_arg, "is a directory");
        return;
    }

    // Pass 1: count total lines
    let mut total_lines = 0usize;
    let mut buf = [0u8; 512];
    loop {
        let r = read(fd, &mut buf);
        if r == 0 || r == ERR {
            break;
        }
        for &b in &buf[..r as usize] {
            if b == b'\n' {
                total_lines += 1;
            }
        }
    }
    let _ = close(fd);

    let skip_lines = total_lines.saturating_sub(lines_limit);

    // Pass 2: print remaining lines
    let fd = open(path, OPEN_READ);
    if fd == ERR {
        return;
    }
    let mut cur_line = 0usize;
    loop {
        let r = read(fd, &mut buf);
        if r == 0 || r == ERR {
            break;
        }
        let chunk = &buf[..r as usize];
        let mut start = 0usize;
        for (i, &b) in chunk.iter().enumerate() {
            if b == b'\n' {
                if cur_line >= skip_lines {
                    let _ = write(1, &chunk[start..=i]);
                }
                cur_line += 1;
                start = i + 1;
            }
        }
        if cur_line >= skip_lines && start < chunk.len() {
            let _ = write(1, &chunk[start..]);
        }
    }
    let _ = close(fd);
}

pub fn cmd_wc(cwd: &Cwd, args: &str) {
    let mut opt_l = false;
    let mut opt_w = false;
    let mut opt_c = false;
    let mut path_arg = "";

    for word in args.split_whitespace() {
        if word.starts_with('-') && word.len() > 1 {
            for b in word[1..].bytes() {
                match b {
                    b'l' => opt_l = true,
                    b'w' => opt_w = true,
                    b'c' => opt_c = true,
                    _ => {
                        err::err("wc", "invalid option");
                        err::usage("wc", "[-l] [-w] [-c] <file>");
                        return;
                    }
                }
            }
        } else {
            path_arg = word;
        }
    }

    if !opt_l && !opt_w && !opt_c {
        opt_l = true;
        opt_w = true;
        opt_c = true;
    }

    if path_arg.is_empty() {
        err::err("wc", "missing file path");
        err::usage("wc", "[-l] [-w] [-c] <file>");
        return;
    }

    let mut abs = [0u8; 256];
    let path = resolve(cwd.as_str(), path_arg, &mut abs);
    let fd = open(path, OPEN_READ);
    if fd == ERR {
        let _ = write(1, b"wc: not found\n");
        err::err_target("wc", path_arg, "file not found");
        return;
    }

    let mut lines = 0usize;
    let mut words = 0usize;
    let mut bytes = 0usize;
    let mut in_word = false;
    let mut buf = [0u8; 512];

    loop {
        let r = read(fd, &mut buf);
        if r == 0 || r == ERR {
            break;
        }
        bytes += r as usize;
        for &b in &buf[..r as usize] {
            if b == b'\n' {
                lines += 1;
            }
            if b == b' ' || b == b'\t' || b == b'\n' || b == b'\r' {
                in_word = false;
            } else if !in_word {
                in_word = true;
                words += 1;
            }
        }
    }
    let _ = close(fd);

    if opt_l {
        print_num(lines);
        let _ = write(1, b" ");
    }
    if opt_w {
        print_num(words);
        let _ = write(1, b" ");
    }
    if opt_c {
        print_num(bytes);
        let _ = write(1, b" ");
    }
    let _ = write(1, path_arg.as_bytes());
    let _ = write(1, b"\n");
}

pub fn cmd_grep(cwd: &Cwd, args: &str) {
    let mut opt_i = false;
    let mut opt_n = false;
    let mut opt_v = false;
    let mut pattern = "";
    let mut path_arg = "";

    for word in args.split_whitespace() {
        if word.starts_with('-') && word.len() > 1 {
            for b in word[1..].bytes() {
                match b {
                    b'i' => opt_i = true,
                    b'n' => opt_n = true,
                    b'v' => opt_v = true,
                    _ => {}
                }
            }
        } else if pattern.is_empty() {
            pattern = word;
        } else if path_arg.is_empty() {
            path_arg = word;
        }
    }

    if pattern.is_empty() {
        err::err("grep", "missing search term");
        err::usage("grep", "[-i] [-n] [-v] <term> [file]");
        return;
    }

    let fd = if path_arg.is_empty() {
        0 // stdin
    } else {
        let mut abs = [0u8; 256];
        let path = resolve(cwd.as_str(), path_arg, &mut abs);
        let f = open(path, OPEN_READ);
        if f == ERR {
            let _ = write(1, b"grep: not found\n");
            err::err_target("grep", path_arg, "file not found");
            return;
        }
        f
    };

    let mut line_buf = [0u8; 1024];
    let mut line_len = 0usize;
    let mut line_idx = 1usize;
    let mut buf = [0u8; 512];

    loop {
        let r = read(fd, &mut buf);
        if r == 0 || r == ERR {
            break;
        }
        for &b in &buf[..r as usize] {
            if b == b'\n' {
                check_and_print_grep(
                    &line_buf[..line_len],
                    pattern,
                    opt_i,
                    opt_n,
                    opt_v,
                    line_idx,
                );
                line_len = 0;
                line_idx += 1;
            } else if line_len < line_buf.len() {
                line_buf[line_len] = b;
                line_len += 1;
            }
        }
    }
    if line_len > 0 {
        check_and_print_grep(
            &line_buf[..line_len],
            pattern,
            opt_i,
            opt_n,
            opt_v,
            line_idx,
        );
    }

    if fd != 0 {
        let _ = close(fd);
    }
}

fn check_and_print_grep(
    line: &[u8],
    pat: &str,
    opt_i: bool,
    opt_n: bool,
    opt_v: bool,
    idx: usize,
) {
    let line_str = match core::str::from_utf8(line) {
        Ok(s) => s,
        Err(_) => return,
    };
    let found = if opt_i {
        contains_ignore_ascii_case(line_str, pat)
    } else {
        line_str.contains(pat)
    };

    let matched = (found && !opt_v) || (!found && opt_v);
    if matched {
        if opt_n {
            let _ = write(1, b"\x1b[32m");
            print_num(idx);
            let _ = write(1, b":\x1b[0m ");
        }
        if !opt_v && !pat.is_empty() {
            print_highlighted(line_str, pat, opt_i);
        } else {
            let _ = write(1, line);
            let _ = write(1, b"\n");
        }
    }
}

fn contains_ignore_ascii_case(haystack: &str, needle: &str) -> bool {
    let hb = haystack.as_bytes();
    let nb = needle.as_bytes();
    if nb.is_empty() {
        return true;
    }
    if hb.len() < nb.len() {
        return false;
    }
    for i in 0..=(hb.len() - nb.len()) {
        let slice = &hb[i..i + nb.len()];
        if slice.eq_ignore_ascii_case(nb) {
            return true;
        }
    }
    false
}

fn print_highlighted(haystack: &str, needle: &str, ignore_case: bool) {
    let hb = haystack.as_bytes();
    let nb = needle.as_bytes();
    let mut start = 0usize;
    while start < hb.len() {
        let mut match_pos = None;
        for i in start..=hb.len().saturating_sub(nb.len()) {
            let slice = &hb[i..i + nb.len()];
            let matches = if ignore_case {
                slice.eq_ignore_ascii_case(nb)
            } else {
                slice == nb
            };
            if matches {
                match_pos = Some(i);
                break;
            }
        }
        match match_pos {
            Some(pos) => {
                if pos > start {
                    let _ = write(1, &hb[start..pos]);
                }
                let _ = write(1, b"\x1b[1;31m");
                let _ = write(1, &hb[pos..pos + nb.len()]);
                let _ = write(1, b"\x1b[0m");
                start = pos + nb.len();
            }
            None => {
                let _ = write(1, &hb[start..]);
                break;
            }
        }
    }
    let _ = write(1, b"\n");
}

pub fn cmd_stat(cwd: &Cwd, args: &str) {
    let path_arg = args.split_whitespace().next().unwrap_or("");
    if path_arg.is_empty() {
        err::err("stat", "missing file path");
        err::usage("stat", "<path>");
        return;
    }
    let mut abs = [0u8; 256];
    let path = resolve(cwd.as_str(), path_arg, &mut abs);
    let fd = open(path, OPEN_READ);
    if fd == ERR {
        let _ = write(1, b"stat: not found\n");
        err::err_target("stat", path_arg, "file or directory not found");
        return;
    }
    let mut probe = [0u8; DIRENT_SIZE];
    let is_dir = readdir(fd, &mut probe) != ERR;

    let mut bytes = 0usize;
    if !is_dir {
        let mut buf = [0u8; 512];
        loop {
            let r = read(fd, &mut buf);
            if r == 0 || r == ERR {
                break;
            }
            bytes += r as usize;
        }
    }
    let _ = close(fd);

    let _ = write(1, b"     \x1b[1mFile:\x1b[0m ");
    let _ = write(1, path.as_bytes());
    let _ = write(1, b"\n");

    let _ = write(1, b"     \x1b[1mType:\x1b[0m ");
    if is_dir {
        let _ = write(1, b"\x1b[1;34mDirectory\x1b[0m\n");
    } else {
        let _ = write(1, b"Regular File\n");
        let _ = write(1, b"     \x1b[1mSize:\x1b[0m ");
        print_num(bytes);
        let _ = write(1, b" bytes");
        if bytes >= 1024 {
            let _ = write(1, b" (");
            print_num(bytes / 1024);
            let _ = write(1, b" KB)");
        }
        let _ = write(1, b"\n");
    }
}

fn parse_line_limit<'a>(args: &'a str, default_val: usize) -> (usize, &'a str) {
    let mut n = default_val;
    let mut path = "";
    let mut expect_n = false;

    for word in args.split_whitespace() {
        if expect_n {
            if let Ok(val) = parse_usize(word) {
                n = val.max(1);
            }
            expect_n = false;
        } else if word == "-n" {
            expect_n = true;
        } else if word.starts_with("-n") && word.len() > 2 {
            if let Ok(val) = parse_usize(&word[2..]) {
                n = val.max(1);
            }
        } else if word.starts_with('-') && word.len() > 1 && word.as_bytes()[1].is_ascii_digit() {
            if let Ok(val) = parse_usize(&word[1..]) {
                n = val.max(1);
            }
        } else if path.is_empty() {
            path = word;
        }
    }
    (n, path)
}

fn parse_usize(s: &str) -> Result<usize, ()> {
    let mut val = 0usize;
    for b in s.bytes() {
        if !b.is_ascii_digit() {
            return Err(());
        }
        val = val.saturating_mul(10).saturating_add((b - b'0') as usize);
    }
    Ok(val)
}

fn print_num(n: usize) {
    if n == 0 {
        let _ = write(1, b"0");
        return;
    }
    let mut buf = [0u8; 20];
    let mut x = n;
    let mut i = 20usize;
    while x > 0 {
        i -= 1;
        buf[i] = b'0' + (x % 10) as u8;
        x /= 10;
    }
    let _ = write(1, &buf[i..]);
}
