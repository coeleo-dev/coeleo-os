use libcoeleo::{ERR, OPEN_READ, close, open, write};

use crate::err;
use crate::highlight::BUILTINS;
use crate::line::History;

const MAX_ALIASES: usize = 16;
const NAME_CAP: usize = 16;
const VALUE_CAP: usize = 64;

#[derive(Clone, Copy)]
struct Alias {
    name: [u8; NAME_CAP],
    name_len: usize,
    val: [u8; VALUE_CAP],
    val_len: usize,
}

impl Alias {
    const fn empty() -> Self {
        Self {
            name: [0; NAME_CAP],
            name_len: 0,
            val: [0; VALUE_CAP],
            val_len: 0,
        }
    }
}

pub struct AliasTable {
    items: [Alias; MAX_ALIASES],
    count: usize,
}

impl AliasTable {
    pub const fn new() -> Self {
        Self {
            items: [Alias::empty(); MAX_ALIASES],
            count: 0,
        }
    }

    pub fn set(&mut self, name: &str, val: &str) -> bool {
        let name_bytes = name.as_bytes();
        let val_bytes = val.as_bytes();
        if name_bytes.is_empty() || name_bytes.len() > NAME_CAP || val_bytes.len() > VALUE_CAP {
            return false;
        }
        for i in 0..self.count {
            if &self.items[i].name[..self.items[i].name_len] == name_bytes {
                self.items[i].val[..val_bytes.len()].copy_from_slice(val_bytes);
                self.items[i].val_len = val_bytes.len();
                return true;
            }
        }
        if self.count < MAX_ALIASES {
            let i = self.count;
            self.items[i].name[..name_bytes.len()].copy_from_slice(name_bytes);
            self.items[i].name_len = name_bytes.len();
            self.items[i].val[..val_bytes.len()].copy_from_slice(val_bytes);
            self.items[i].val_len = val_bytes.len();
            self.count += 1;
            return true;
        }
        false
    }

    pub fn get<'a>(&'a self, name: &str) -> Option<&'a str> {
        let nb = name.as_bytes();
        for i in 0..self.count {
            if &self.items[i].name[..self.items[i].name_len] == nb {
                return core::str::from_utf8(&self.items[i].val[..self.items[i].val_len]).ok();
            }
        }
        None
    }

    pub fn remove(&mut self, name: &str) -> bool {
        let nb = name.as_bytes();
        for i in 0..self.count {
            if &self.items[i].name[..self.items[i].name_len] == nb {
                for k in i..self.count - 1 {
                    self.items[k] = self.items[k + 1];
                }
                self.count -= 1;
                return true;
            }
        }
        false
    }

    pub fn list(&self) {
        if self.count == 0 {
            let _ = write(1, b"(no aliases defined)\n");
            return;
        }
        for i in 0..self.count {
            let name = core::str::from_utf8(&self.items[i].name[..self.items[i].name_len]).unwrap_or("");
            let val = core::str::from_utf8(&self.items[i].val[..self.items[i].val_len]).unwrap_or("");
            let _ = write(1, b"alias ");
            let _ = write(1, name.as_bytes());
            let _ = write(1, b"='");
            let _ = write(1, val.as_bytes());
            let _ = write(1, b"'\n");
        }
    }
}

pub fn cmd_alias(table: &mut AliasTable, args: &str) {
    let args = args.trim();
    if args.is_empty() {
        table.list();
        return;
    }
    if let Some((name, val)) = args.split_once('=') {
        let name = name.trim();
        let val = val.trim().trim_matches('\'').trim_matches('"');
        if table.set(name, val) {
            let _ = write(1, b"alias set: ");
            let _ = write(1, name.as_bytes());
            let _ = write(1, b" -> '");
            let _ = write(1, val.as_bytes());
            let _ = write(1, b"'\n");
        } else {
            err::err("alias", "alias table full or identifier too long");
        }
    } else {
        err::err("alias", "invalid syntax");
        err::usage("alias", "[name='command']");
    }
}

pub fn cmd_unalias(table: &mut AliasTable, args: &str) {
    let name = args.split_whitespace().next().unwrap_or("");
    if name.is_empty() {
        err::err("unalias", "missing alias name to remove");
        err::usage("unalias", "<name>");
        return;
    }
    if table.remove(name) {
        let _ = write(1, b"alias removed: ");
        let _ = write(1, name.as_bytes());
        let _ = write(1, b"\n");
    } else {
        err::err_target("unalias", name, "alias not found");
    }
}

pub fn cmd_which(table: &AliasTable, args: &str) {
    let cmd = args.split_whitespace().next().unwrap_or("");
    if cmd.is_empty() {
        err::err("which", "missing command name");
        err::usage("which", "<command>");
        return;
    }
    if let Some(alias_val) = table.get(cmd) {
        let _ = write(1, cmd.as_bytes());
        let _ = write(1, b": aliased to '");
        let _ = write(1, alias_val.as_bytes());
        let _ = write(1, b"'\n");
        return;
    }
    if BUILTINS.iter().any(|&b| b == cmd) {
        let _ = write(1, cmd.as_bytes());
        let _ = write(1, b": shell built-in command\n");
        return;
    }

    // Check /bin/<cmd>
    let mut bin_path = [0u8; 32];
    if cmd.len() + 5 < bin_path.len() {
        bin_path[..5].copy_from_slice(b"/bin/");
        bin_path[5..5 + cmd.len()].copy_from_slice(cmd.as_bytes());
        let bin_str = core::str::from_utf8(&bin_path[..5 + cmd.len()]).unwrap_or("");
        let fd = open(bin_str, OPEN_READ);
        if fd != ERR {
            let _ = close(fd);
            let _ = write(1, bin_str.as_bytes());
            let _ = write(1, b"\n");
            return;
        }
    }

    let _ = write(1, cmd.as_bytes());
    let _ = write(1, b" not found\n");
}

pub fn cmd_history(hist: &mut History, args: &str) {
    if args.split_whitespace().any(|a| a == "-c" || a == "--clear") {
        hist.clear();
        let _ = write(1, b"history cleared\n");
        return;
    }
    let count = hist.count();
    if count == 0 {
        let _ = write(1, b"(history empty)\n");
        return;
    }
    let mut tmp = [0u8; 128];
    for i in 0..count {
        let len = hist.get_at(i, &mut tmp);
        let _ = write(1, b"  ");
        print_num(i + 1);
        let _ = write(1, b"  ");
        let _ = write(1, &tmp[..len]);
        let _ = write(1, b"\n");
    }
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
