//! Serial stays plain for tests; Flanterm can take ANSI and the boot hero.

/// Flanterm treats `\n` as LF (next row, same column). `\r` returns to column 0.
/// Without CR, each command walks one cell to the right (the framebuffer "staircase").
pub fn write(s: &str) {
    emit(s, true, true);
}

/// Framebuffer only. Serial tests keep grepping a one-line `Coeleo OS`.
pub fn write_screen(s: &str) {
    emit(s, false, true);
}

pub fn boot_banner() {
    crate::serial::write_str("Coeleo OS\n");
}

pub fn boot_hero() {
    write_screen(HERO);
}

const HERO: &str = "\x1b[96m\
   ____            _\n\
  / ___|___   ___ | | ___  ___\n\
 | |   / _ \\ / _ \\| |/ _ \\/ _ \\\n\
 | |__| (_) |  __/| |  __/ (_) |\n\
  \\____\\___/ \\___||_|\\___|\\___/\n\
\x1b[0m\x1b[1;94m  Coeleo OS\x1b[0m\n\
\x1b[90m  shell + files\x1b[0m\n\
";

fn emit(s: &str, serial: bool, screen: bool) {
    let mut rest = s;
    loop {
        match rest.find('\n') {
            None => {
                if !rest.is_empty() {
                    if serial {
                        crate::serial::write_str(rest);
                    }
                    if screen {
                        crate::fbterm::write(rest);
                    }
                }
                return;
            }
            Some(i) => {
                let (head, tail) = rest.split_at(i);
                if !head.is_empty() {
                    if serial {
                        crate::serial::write_str(head);
                    }
                    if screen {
                        crate::fbterm::write(head);
                    }
                }
                if serial {
                    crate::serial::write_str("\r\n");
                }
                if screen {
                    crate::fbterm::write("\r\n");
                }
                rest = &tail[1..];
            }
        }
    }
}
