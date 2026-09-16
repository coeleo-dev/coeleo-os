use libcoeleo::{ERR, write};

use crate::complete;
use crate::cwd::Cwd;

pub const LINE_CAP: usize = 128;
pub const HIST_CAP: usize = 128;

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

    pub fn count(&self) -> usize {
        self.count
    }

    pub fn clear(&mut self) {
        self.count = 0;
        self.start = 0;
        self.view = None;
        self.draft_len = 0;
    }

    pub fn get_at(&self, idx: usize, out: &mut [u8]) -> usize {
        if idx >= self.count {
            return 0;
        }
        let i = (self.start + idx) % HIST_CAP;
        let n = self.lens[i];
        out[..n].copy_from_slice(&self.lines[i][..n]);
        n
    }

    pub fn peek_entry<'a>(&'a self, age: usize) -> Option<&'a [u8]> {
        if age >= self.count {
            return None;
        }
        let i = (self.start + self.count - 1 - age) % HIST_CAP;
        Some(&self.lines[i][..self.lens[i]])
    }

    pub fn push(&mut self, s: &[u8]) {
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
    Csi { buf: [u8; 8], len: usize },
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
                match c[0] {
                    b'[' => seq = Seq::Csi { buf: [0; 8], len: 0 },
                    b'b' => {
                        seq = Seq::Normal;
                        if fancy {
                            word_left(line, &mut cur);
                        }
                        continue;
                    }
                    b'f' => {
                        seq = Seq::Normal;
                        if fancy {
                            word_right(line, n, &mut cur);
                        }
                        continue;
                    }
                    _ => {
                        seq = Seq::Normal;
                        continue;
                    }
                }
                continue;
            }
            Seq::Csi { mut buf, mut len } => {
                let b = c[0];
                if (0x40..=0x7E).contains(&b) {
                    seq = Seq::Normal;
                    if fancy {
                        match b {
                            b'A' => hist_up(hist, line, &mut n, &mut cur),
                            b'B' => hist_down(hist, line, &mut n, &mut cur),
                            b'C' => move_right(line, n, &mut cur),
                            b'D' => move_left(&mut cur),
                            b'H' => go_home(&mut cur),
                            b'F' => go_to_end(line, n, &mut cur),
                            b'~' => {
                                match &buf[..len] {
                                    b"3" => forward_delete(line, &mut n, cur),
                                    b"1" => go_home(&mut cur),
                                    b"4" => go_to_end(line, n, &mut cur),
                                    _ => {}
                                }
                            }
                            _ => {
                                if &buf[..len] == b"1;5" {
                                    if b == b'D' {
                                        word_left(line, &mut cur);
                                    } else if b == b'C' {
                                        word_right(line, n, &mut cur);
                                    }
                                }
                            }
                        }
                    }
                    continue;
                } else if len < 8 {
                    buf[len] = b;
                    len += 1;
                    seq = Seq::Csi { buf, len };
                    continue;
                } else {
                    seq = Seq::Normal;
                    continue;
                }
            }
            Seq::Normal => {}
        }
        match c[0] {
            0x1B => seq = Seq::Esc,
            0x01 => {
                // Ctrl+A: Home
                if fancy {
                    go_home(&mut cur);
                }
            }
            0x05 => {
                // Ctrl+E: End
                if fancy {
                    go_to_end(line, n, &mut cur);
                }
            }
            0x04 => {
                // Ctrl+D: EOF if empty line, otherwise forward delete
                if n == 0 {
                    return usize::MAX;
                }
                if fancy {
                    forward_delete(line, &mut n, cur);
                }
            }
            0x0B => {
                // Ctrl+K: Kill to end
                if fancy {
                    kill_to_end(line, &mut n, cur);
                }
            }
            0x14 => {
                // Ctrl+T: Transpose characters
                if fancy {
                    transpose(line, n, &mut cur);
                }
            }
            0x12 => {
                // Ctrl+R: Reverse search
                if fancy {
                    reverse_search(hist, line, &mut n, &mut cur, prompt);
                }
            }
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
            0x16 => {
                if fancy {
                    let mut clip_buf = [0u8; 4096];
                    if let Some(clip_len) = libcoeleo::clipboard_get(&mut clip_buf) {
                        let mut added = false;
                        let len = clip_len.min(clip_buf.len());
                        for &b in &clip_buf[..len] {
                            if b >= 0x20 && b != 0x7F && n < line.len() {
                                for i in (cur..n).rev() {
                                    line[i + 1] = line[i];
                                }
                                line[cur] = b;
                                n += 1;
                                cur += 1;
                                added = true;
                            }
                        }
                        if added {
                            redraw(prompt, line, n, cur);
                        }
                    }
                }
            }
            0x17 => {
                if fancy {
                    kill_word(line, &mut n, &mut cur);
                }
            }
            0x08 | 0x7F => {
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
            b if b >= 0x20 && b != 0x7F && n < line.len() => {
                hist.view = None;
                insert(line, &mut n, &mut cur, b);
            }
            _ => {}
        }
    }
}

fn go_home(cur: &mut usize) {
    if *cur > 0 {
        for _ in 0..*cur {
            let _ = write(1, b"\x08");
        }
        *cur = 0;
    }
}

fn go_to_end(line: &[u8], n: usize, cur: &mut usize) {
    if *cur < n {
        let _ = write(1, &line[*cur..n]);
        *cur = n;
    }
}

fn forward_delete(line: &mut [u8], n: &mut usize, cur: usize) {
    if cur < *n {
        *n -= 1;
        for i in cur..*n {
            line[i] = line[i + 1];
        }
        if cur < *n {
            let _ = write(1, &line[cur..*n]);
        }
        let _ = write(1, b" \x08");
        for _ in 0..(*n - cur) {
            let _ = write(1, b"\x08");
        }
    }
}

fn kill_to_end(_line: &mut [u8], n: &mut usize, cur: usize) {
    if cur < *n {
        let drop = *n - cur;
        for _ in 0..drop {
            let _ = write(1, b" ");
        }
        for _ in 0..drop {
            let _ = write(1, b"\x08");
        }
        *n = cur;
    }
}

fn transpose(line: &mut [u8], n: usize, cur: &mut usize) {
    if *cur >= 2 && *cur <= n {
        let a = line[*cur - 2];
        let b = line[*cur - 1];
        line[*cur - 2] = b;
        line[*cur - 1] = a;
        let _ = write(1, b"\x08\x08");
        let _ = write(1, &[b, a]);
    }
}

fn word_left(line: &[u8], cur: &mut usize) {
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
    for _ in 0..(*cur - i) {
        let _ = write(1, b"\x08");
    }
    *cur = i;
}

fn word_right(line: &[u8], n: usize, cur: &mut usize) {
    if *cur >= n {
        return;
    }
    let mut i = *cur;
    while i < n && line[i] == b' ' {
        i += 1;
    }
    while i < n && line[i] != b' ' {
        i += 1;
    }
    let _ = write(1, &line[*cur..i]);
    *cur = i;
}

fn reverse_search(
    hist: &History,
    line: &mut [u8],
    n: &mut usize,
    cur: &mut usize,
    prompt: &[u8],
) {
    let mut query = [0u8; 32];
    let mut qlen = 0usize;
    let mut match_idx: Option<usize> = None;
    let mut original = [0u8; LINE_CAP];
    original[..*n].copy_from_slice(&line[..*n]);
    let orig_n = *n;

    loop {
        // Redraw reverse search prompt
        let _ = write(1, b"\r\x1b[K(reverse-i-search)`");
        if qlen > 0 {
            let _ = write(1, &query[..qlen]);
        }
        let _ = write(1, b"': ");
        if let Some(idx) = match_idx {
            let mut tmp = [0u8; LINE_CAP];
            let len = hist.get_at(idx, &mut tmp);
            let _ = write(1, &tmp[..len]);
        }

        let mut c = [0u8; 1];
        let r = libcoeleo::read(0, &mut c);
        if r == 0 || r == ERR {
            break;
        }
        match c[0] {
            0x12 => {
                // Ctrl+R: search earlier match
                if let Some(cur_idx) = match_idx {
                    if cur_idx > 0 {
                        match_idx = find_match(hist, &query[..qlen], cur_idx - 1);
                    }
                }
            }
            0x08 | 0x7F => {
                if qlen > 0 {
                    qlen -= 1;
                    match_idx = if qlen > 0 {
                        find_match(hist, &query[..qlen], hist.count().saturating_sub(1))
                    } else {
                        None
                    };
                }
            }
            0x1B | 0x03 | 0x07 => {
                // Cancel
                line[..orig_n].copy_from_slice(&original[..orig_n]);
                *n = orig_n;
                *cur = orig_n;
                redraw(prompt, line, *n, *cur);
                return;
            }
            b'\n' | b'\r' => {
                if let Some(idx) = match_idx {
                    let mut tmp = [0u8; LINE_CAP];
                    let len = hist.get_at(idx, &mut tmp);
                    line[..len].copy_from_slice(&tmp[..len]);
                    *n = len;
                    *cur = len;
                }
                redraw(prompt, line, *n, *cur);
                return;
            }
            b if b >= 0x20 && b != 0x7F && qlen < query.len() => {
                query[qlen] = b;
                qlen += 1;
                match_idx = find_match(hist, &query[..qlen], hist.count().saturating_sub(1));
            }
            _ => {}
        }
    }
}

fn find_match(hist: &History, query: &[u8], start_idx: usize) -> Option<usize> {
    if query.is_empty() || hist.count() == 0 {
        return None;
    }
    let mut tmp = [0u8; LINE_CAP];
    let limit = start_idx.min(hist.count().saturating_sub(1));
    for i in (0..=limit).rev() {
        let len = hist.get_at(i, &mut tmp);
        let entry = &tmp[..len];
        if contains_subslice(entry, query) {
            return Some(i);
        }
    }
    None
}

fn contains_subslice(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() {
        return true;
    }
    if haystack.len() < needle.len() {
        return false;
    }
    for i in 0..=(haystack.len() - needle.len()) {
        if &haystack[i..i + needle.len()] == needle {
            return true;
        }
    }
    false
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
