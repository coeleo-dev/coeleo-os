pub struct Cwd {
    buf: [u8; 256],
    len: usize,
}

impl Cwd {
    pub fn new() -> Self {
        let mut buf = [0u8; 256];
        buf[0] = b'/';
        Self { buf, len: 1 }
    }

    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.buf[..self.len]).unwrap_or("/")
    }

    pub fn set(&mut self, s: &str) {
        let n = s.len().min(self.buf.len());
        self.buf[..n].copy_from_slice(&s.as_bytes()[..n]);
        self.len = n;
    }
}

pub fn resolve<'a>(cwd: &str, arg: &str, out: &'a mut [u8; 256]) -> &'a str {
    let mut parts: [&str; 16] = [""; 16];
    let mut n = 0usize;
    let first = if arg.starts_with('/') { "" } else { cwd };
    for src in [first, arg] {
        for c in src.split('/') {
            match c {
                "" | "." => {}
                ".." => {
                    if n > 0 {
                        n -= 1;
                    }
                }
                other if n < 16 => {
                    parts[n] = other;
                    n += 1;
                }
                _ => {}
            }
        }
    }
    if n == 0 {
        out[0] = b'/';
        return core::str::from_utf8(&out[..1]).unwrap_or("/");
    }
    let mut i = 0usize;
    for p in 0..n {
        if i + 1 + parts[p].len() >= out.len() {
            break;
        }
        out[i] = b'/';
        i += 1;
        let b = parts[p].as_bytes();
        out[i..i + b.len()].copy_from_slice(b);
        i += b.len();
    }
    core::str::from_utf8(&out[..i]).unwrap_or("/")
}
