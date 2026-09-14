use libcoeleo::{ERR, write};

use crate::complete;
use crate::cwd::Cwd;

pub const LINE_CAP: usize = 128;
pub const HIST_CAP: usize = 16;

pub struct History {
    lines: [[u8; LINE_CAP]; HIST_CAP],
    lens: [usize; HIST_CAP],
    start: usize,
    count: usize,
    view: Option<usize>,
    draft: [u8; LINE_CAP],
    draft_len: usize,
}

impl History {
    pub const fn new() -> Self {
        Self {
            lines: [[0; LINE_CAP]; HIST_CAP],
            lens: [0; HIST_CAP],
            start: 0,
            count: 0,
            view: None,
            draft: [0; LINE_CAP],
            draft_len: 0,
        }
    }

    fn push(&mut self, s: &[u8]) {
        let t = trim_bytes(s);
        if t.is_empty() || t.len() > LINE_CAP {
            self.view = None;
            return;
        }
        if self.count > 0 {
            let newest = (self.start + self.count - 1) % HIST_CAP;
            if &self.lines[newest][..self.lens[newest]] == t {
                self.view = None;
                return;
            }
        }
        if self.count < HIST_CAP {
            let i = (self.start + self.count) % HIST_CAP;
            self.lines[i][..t.len()].copy_from_slice(t);
            self.lens[i] = t.len();
            self.count += 1;
        } else {
            self.lines[self.start][..t.len()].copy_from_slice(t);
            self.lens[self.start] = t.len();
            self.start = (self.start + 1) % HIST_CAP;
        }
        self.view = None;
    }

    fn get(&self, age: usize, out: &mut [u8]) -> usize {
        if age >= self.count {
            return 0;
        }
        let i = (self.start + self.count - 1 - age) % HIST_CAP;
        let n = self.lens[i];
        out[..n].copy_from_slice(&self.lines[i][..n]);
        n
    }
}

enum Seq {
    Normal,
    Esc,
    Csi,
}

pub fn read_line(
    line: &mut [u8],
    hist: &mut History,
    cwd: &Cwd,
    prompt: &[u8],
    fancy: bool,
) -> usize {
    let mut n = 0usize;
    let mut cur = 0usize;
    let mut seq = Seq::Normal;
    loop {
        let mut c = [0u8; 1];
        let r = libcoeleo::read(0, &mut c);
        if r == 0 {
            return usize::MAX;
        }
        if r == ERR {
            continue;
        }
        match seq {
            Seq::Esc => {
                seq = if c[0] == b'[' { Seq::Csi } else { Seq::Normal };
                continue;
            }
            Seq::Csi => {
                seq = Seq::Normal;
                if fancy {
                    match c[0] {
                        b'A' => hist_up(hist, line, &mut n, &mut cur),
                        b'B' => hist_down(hist, line, &mut n, &mut cur),
                        b'C' => move_right(line, n, &mut cur),
                        b'D' => move_left(&mut cur),
                        _ => {}
                    }
                }
                continue;
            }
            Seq::Normal => {}
        }
        match c[0] {
            0x1B => seq = Seq::Esc,
            b'\n' | b'\r' => {
                go_end(line, n, cur);
                let _ = write(1, b"\n");
                if fancy {
                    hist.push(&line[..n]);
                }
                return n;
            }
            0x03 => {
                if fancy {
                    go_end(line, n, cur);
                    let _ = write(1, b"\n");
                    return 0;
                }
                hist.view = None;
                go_end(line, n, cur);
                erase_n(n);
                n = 0;
                cur = 0;
            }
            0x0C => {
                if fancy {
                    redraw(prompt, line, n, cur);
                }
            }
            0x15 => {
                if fancy {
                    kill_to_start(line, &mut n, &mut cur);
                }
            }
            0x17 => {
                if fancy {
                    kill_word(line, &mut n, &mut cur);
                }
            }
            0x08 | 0x7f => {
                hist.view = None;
                backspace(line, &mut n, &mut cur);
            }
            b'\t' => {
                if fancy {
                    hist.view = None;
                    complete::apply(line, &mut n, cwd, prompt);
                    cur = n;
                }
            }
            b if (b.is_ascii_graphic() || b == b' ') && n < line.len() => {
                hist.view = None;
                insert(line, &mut n, &mut cur, b);
            }
            _ => {}
        }
    }
}

fn hist_up(hist: &mut History, line: &mut [u8], n: &mut usize, cur: &mut usize) {
    if hist.count == 0 {
        return;
    }
    let age = match hist.view {
        None => {
            hist.draft[..*n].copy_from_slice(&line[..*n]);
            hist.draft_len = *n;
            hist.view = Some(0);
            0
        }
        Some(age) => {
            if age + 1 >= hist.count {
                return;
            }
            hist.view = Some(age + 1);
            age + 1
        }
    };
    let mut tmp = [0u8; LINE_CAP];
    let len = hist.get(age, &mut tmp);
    paint_replace(line, n, cur, &tmp[..len]);
}

fn hist_down(hist: &mut History, line: &mut [u8], n: &mut usize, cur: &mut usize) {
    match hist.view {
        None => {}
        Some(0) => {
            hist.view = None;
            paint_replace(line, n, cur, &hist.draft[..hist.draft_len]);
        }
        Some(age) => {
            hist.view = Some(age - 1);
            let mut tmp = [0u8; LINE_CAP];
            let len = hist.get(age - 1, &mut tmp);
            paint_replace(line, n, cur, &tmp[..len]);
        }
    }
}

fn paint_replace(line: &mut [u8], n: &mut usize, cur: &mut usize, src: &[u8]) {
    go_end(line, *n, *cur);
    erase_n(*n);
    let len = src.len().min(line.len());
    line[..len].copy_from_slice(&src[..len]);
    *n = len;
    *cur = len;
    if len > 0 {
        let _ = write(1, &line[..len]);
    }
}

fn insert(line: &mut [u8], n: &mut usize, cur: &mut usize, b: u8) {
    for i in (*cur..*n).rev() {
        line[i + 1] = line[i];
    }
    line[*cur] = b;
    *n += 1;
    *cur += 1;
    let _ = write(1, &[b]);
    show_tail(line, *n, *cur);
}

fn backspace(line: &mut [u8], n: &mut usize, cur: &mut usize) {
    if *cur == 0 {
        return;
    }
    *cur -= 1;
    *n -= 1;
    for i in *cur..*n {
        line[i] = line[i + 1];
    }
    let _ = write(1, b"\x08");
    if *cur < *n {
        let _ = write(1, &line[*cur..*n]);
    }
    let _ = write(1, b" \x08");
    for _ in 0..(*n - *cur) {
        let _ = write(1, b"\x08");
    }
}

fn kill_to_start(line: &mut [u8], n: &mut usize, cur: &mut usize) {
    if *cur == 0 {
        return;
    }
    let old = *n;
    let rest = *n - *cur;
    go_end(line, *n, *cur);
    erase_n(old);
    for i in 0..rest {
        line[i] = line[i + *cur];
    }
    *n = rest;
    *cur = 0;
    if rest > 0 {
        let _ = write(1, &line[..rest]);
        for _ in 0..rest {
            let _ = write(1, b"\x08");
        }
    }
}

fn kill_word(line: &mut [u8], n: &mut usize, cur: &mut usize) {
    if *cur == 0 {
        return;
    }
    let mut i = *cur;
    while i > 0 && line[i - 1] == b' ' {
        i -= 1;
    }
    while i > 0 && line[i - 1] != b' ' {
        i -= 1;
    }
    let old = *n;
    let drop = *cur - i;
    let rest = *n - *cur;
    go_end(line, *n, *cur);
    erase_n(old);
    for k in 0..rest {
        line[i + k] = line[*cur + k];
    }
    *n -= drop;
    *cur = i;
    if *n > 0 {
        let _ = write(1, &line[..*n]);
    }
    for _ in 0..(*n - *cur) {
        let _ = write(1, b"\x08");
    }
}

fn move_left(cur: &mut usize) {
    if *cur == 0 {
        return;
    }
    *cur -= 1;
    let _ = write(1, b"\x08");
}

fn move_right(line: &[u8], n: usize, cur: &mut usize) {
    if *cur >= n {
        return;
    }
    let _ = write(1, &line[*cur..*cur + 1]);
    *cur += 1;
}

fn go_end(line: &[u8], n: usize, cur: usize) {
    if cur < n {
        let _ = write(1, &line[cur..n]);
    }
}

fn show_tail(line: &[u8], n: usize, cur: usize) {
    if cur >= n {
        return;
    }
    let _ = write(1, &line[cur..n]);
    for _ in 0..(n - cur) {
        let _ = write(1, b"\x08");
    }
}

fn erase_n(n: usize) {
    for _ in 0..n {
        let _ = write(1, b"\x08 \x08");
    }
}

fn redraw(prompt: &[u8], line: &[u8], n: usize, cur: usize) {
    let _ = write(1, b"\x1b[2J\x1b[H");
    let _ = write(1, prompt);
    if n > 0 {
        let _ = write(1, &line[..n]);
    }
    for _ in 0..(n - cur) {
        let _ = write(1, b"\x08");
    }
}

fn trim_bytes(s: &[u8]) -> &[u8] {
    let mut a = 0usize;
    let mut b = s.len();
    while a < b && s[a] == b' ' {
        a += 1;
    }
    while b > a && s[b - 1] == b' ' {
        b -= 1;
    }
    &s[a..b]
}
