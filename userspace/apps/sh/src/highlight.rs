use crate::line::History;

pub const BUILTINS: &[&str] = &[
    "alias", "cat", "cd", "clear", "clip", "clock", "cp", "date", "df", "disk", "disks", "echo",
    "edit", "exit", "fault", "free", "get", "grep", "halt", "head", "help", "history", "install",
    "kill", "ls", "mem", "mkdir", "mv", "ping", "pkg", "poweroff", "ps", "pwd", "reboot", "rm",
    "run", "sleep", "source", "spin", "stat", "sync", "tail", "touch", "tree", "uname", "unalias",
    "uptime", "version", "wc", "which", "write",
];

pub fn is_known_command(cmd: &str) -> bool {
    if BUILTINS.iter().any(|&b| b == cmd) {
        return true;
    }
    // Also common binaries
    if cmd == "hello" || cmd == "widgets" || cmd == "winprobe" || cmd == "sh" {
        return true;
    }
    if cmd.starts_with('/') || cmd.starts_with("./") {
        return true;
    }
    false
}

/// Look up the most recent history entry that starts with `prefix`.
pub fn find_autosuggestion<'a>(prefix: &[u8], hist: &'a History) -> Option<&'a [u8]> {
    if prefix.is_empty() {
        return None;
    }
    let count = hist.count();
    for i in 0..count {
        let age = count - 1 - i;
        if let Some(entry) = hist.peek_entry(age) {
            if entry.len() > prefix.len() && entry.starts_with(prefix) {
                return Some(&entry[prefix.len()..]);
            }
        }
    }
    None
}
