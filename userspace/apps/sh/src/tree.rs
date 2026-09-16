use libcoeleo::{
    DIRENT_SIZE, ERR, OPEN_READ, close, dirent_is_dir, dirent_name, open, readdir, write,
};

use crate::cwd::{Cwd, resolve};
use crate::err;

const MAX_DEPTH: usize = 4;
const MAX_ENTRIES_PER_DIR: usize = 32;

pub fn cmd_tree(cwd: &Cwd, args: &str) {
    let path_arg = args.split_whitespace().next().unwrap_or("");
    let mut abs = [0u8; 256];
    let path = if path_arg.is_empty() {
        cwd.as_str()
    } else {
        resolve(cwd.as_str(), path_arg, &mut abs)
    };

    let fd = open(path, OPEN_READ);
    if fd == ERR {
        let _ = write(1, b"tree: not found\n");
        err::err_target("tree", path, "directory not found");
        return;
    }
    let _ = close(fd);

    let _ = write(1, b"\x1b[1;34m");
    let _ = write(1, path.as_bytes());
    let _ = write(1, b"\x1b[0m\n");

    let mut dir_count = 0usize;
    let mut file_count = 0usize;
    let mut prefix_mask = [false; MAX_DEPTH];

    walk_dir(
        path,
        0,
        &mut prefix_mask,
        &mut dir_count,
        &mut file_count,
    );

    let _ = write(1, b"\n");
    print_num(dir_count);
    let _ = write(1, b" dir(s), ");
    print_num(file_count);
    let _ = write(1, b" file(s)\n");
}

fn walk_dir(
    path: &str,
    depth: usize,
    mask: &mut [bool; MAX_DEPTH],
    dirs: &mut usize,
    files: &mut usize,
) {
    if depth >= MAX_DEPTH {
        return;
    }
    let fd = open(path, OPEN_READ);
    if fd == ERR {
        return;
    }

    // Collect directory entries (up to 32)
    struct Entry {
        name: [u8; 64],
        len: usize,
        is_dir: bool,
    }
    let mut entries: [Entry; MAX_ENTRIES_PER_DIR] = [const {
        Entry {
            name: [0; 64],
            len: 0,
            is_dir: false,
        }
    }; MAX_ENTRIES_PER_DIR];
    let mut count = 0usize;

    loop {
        if count >= MAX_ENTRIES_PER_DIR {
            break;
        }
        let mut ent = [0u8; DIRENT_SIZE];
        let r = readdir(fd, &mut ent);
        if r == 0 || r == ERR {
            break;
        }
        let name = dirent_name(&ent);
        if name.is_empty() || name == "." || name == ".." {
            continue;
        }
        let n = name.len().min(64);
        entries[count].name[..n].copy_from_slice(&name.as_bytes()[..n]);
        entries[count].len = n;
        entries[count].is_dir = dirent_is_dir(&ent);
        count += 1;
    }
    let _ = close(fd);

    for i in 0..count {
        let is_last = i + 1 == count;
        mask[depth] = !is_last;

        // Print indentation prefix
        for d in 0..depth {
            if mask[d] {
                let _ = write(1, b"\xe2\x94\x82   "); // "│   "
            } else {
                let _ = write(1, b"    ");
            }
        }

        let e = &entries[i];
        let name_str = core::str::from_utf8(&e.name[..e.len]).unwrap_or("");
        if is_last {
            let _ = write(1, b"\xe2\x94\x94\xe2\x94\x80\xe2\x94\x80 "); // "└── "
        } else {
            let _ = write(1, b"\xe2\x94\x9c\xe2\x94\x80\xe2\x94\x80 "); // "├── "
        }

        if e.is_dir {
            *dirs += 1;
            let _ = write(1, b"\x1b[1;34m");
            let _ = write(1, name_str.as_bytes());
            let _ = write(1, b"\x1b[0m\n");

            // Build child path
            let mut sub_buf = [0u8; 256];
            let sub_path = concat_path(path, name_str, &mut sub_buf);
            walk_dir(sub_path, depth + 1, mask, dirs, files);
        } else {
            *files += 1;
            let _ = write(1, name_str.as_bytes());
            let _ = write(1, b"\n");
        }
    }
}

fn concat_path<'a>(base: &str, child: &str, out: &'a mut [u8; 256]) -> &'a str {
    let mut i = 0usize;
    let b = base.as_bytes();
    let n = b.len().min(out.len());
    out[..n].copy_from_slice(&b[..n]);
    i += n;
    if !base.ends_with('/') && i < out.len() {
        out[i] = b'/';
        i += 1;
    }
    let cb = child.as_bytes();
    let cn = cb.len().min(out.len().saturating_sub(i));
    out[i..i + cn].copy_from_slice(&cb[..cn]);
    i += cn;
    core::str::from_utf8(&out[..i]).unwrap_or("/")
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
