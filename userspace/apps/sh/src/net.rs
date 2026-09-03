use libcoeleo::{ERR, ERR_BAD_URL, ERR_HTTPS, ERR_NO_NET, ERR_TIMEOUT, http_get, net_ping, write};

use crate::write_u32;

const GET_CAP: usize = 32 * 1024;

static mut GET_BUF: [u8; GET_CAP] = [0; GET_CAP];

pub fn cmd_ping(args: &str) {
    let tok = args.split_whitespace().next().unwrap_or("");
    if tok.is_empty() {
        let _ = write(1, b"ping: missing address\n");
        return;
    }
    let Some(oct) = parse_ipv4(tok) else {
        let _ = write(1, b"ping: bad address\n");
        return;
    };
    let mut buf = [0u8; 32];
    let r = net_ping(oct, &mut buf);
    if r == ERR_NO_NET {
        let _ = write(1, b"ping: no network\n");
        return;
    }
    if r == ERR {
        let _ = write(1, b"ping: failed\n");
        return;
    }
    if r == 0 {
        let _ = write(1, b"ping: timeout\n");
        return;
    }
    let n = r as usize;
    let mut i = 0usize;
    while i < n {
        let off = i * 8;
        if off + 8 > buf.len() {
            break;
        }
        let from = [buf[off], buf[off + 1], buf[off + 2], buf[off + 3]];
        let ms = u32::from_le_bytes(buf[off + 4..off + 8].try_into().unwrap_or([0; 4]));
        let _ = write(1, b"reply from ");
        write_u32(from[0] as u32);
        let _ = write(1, b".");
        write_u32(from[1] as u32);
        let _ = write(1, b".");
        write_u32(from[2] as u32);
        let _ = write(1, b".");
        write_u32(from[3] as u32);
        let _ = write(1, b" time=");
        write_u32(ms);
        let _ = write(1, b"ms\n");
        i += 1;
    }
}

fn parse_ipv4(s: &str) -> Option<[u8; 4]> {
    let mut oct = [0u8; 4];
    let mut n = 0usize;
    for part in s.split('.') {
        if n == 4 || part.is_empty() {
            return None;
        }
        if !part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let v = parse_u8(part)?;
        oct[n] = v;
        n += 1;
    }
    if n != 4 {
        return None;
    }
    Some(oct)
}

fn parse_u8(s: &str) -> Option<u8> {
    let mut v: u16 = 0;
    for b in s.bytes() {
        v = v.checked_mul(10)?.checked_add((b - b'0') as u16)?;
        if v > 255 {
            return None;
        }
    }
    if s.is_empty() {
        return None;
    }
    Some(v as u8)
}

pub fn cmd_get(args: &str) {
    let tok = args.split_whitespace().next().unwrap_or("");
    if tok.is_empty() {
        let _ = write(1, b"get: missing url\n");
        return;
    }
    let buf = unsafe { &mut *core::ptr::addr_of_mut!(GET_BUF) };
    let r = http_get(tok, buf);
    if r == ERR_HTTPS {
        let _ = write(1, b"get: https not supported\n");
        return;
    }
    if r == ERR_BAD_URL {
        let _ = write(1, b"get: bad url\n");
        return;
    }
    if r == ERR_NO_NET {
        let _ = write(1, b"get: no network\n");
        return;
    }
    if r == ERR_TIMEOUT {
        let _ = write(1, b"get: timeout\n");
        return;
    }
    if r == ERR {
        let _ = write(1, b"get: failed\n");
        return;
    }
    let n = r as usize;
    match core::str::from_utf8(&buf[..n]) {
        Ok(_) => {
            let mut off = 0usize;
            while off < n {
                let take = (n - off).min(512);
                let w = write(1, &buf[off..off + take]);
                if w == ERR {
                    break;
                }
                off += w as usize;
            }
        }
        Err(_) => {
            let _ = write(1, b"get: not text\n");
        }
    }
}
