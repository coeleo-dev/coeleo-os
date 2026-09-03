//! HTTP/1.0 URL and header split. No TLS, no DNS.

use alloc::string::String;

pub struct Url {
    pub host: [u8; 4],
    pub port: u16,
    pub path: String,
}

pub enum UrlError {
    Bad,
    Https,
}

pub struct HttpHead {
    pub content_length: Option<usize>,
}

pub fn parse_url(s: &str) -> Result<Url, UrlError> {
    if s.starts_with("https://") {
        return Err(UrlError::Https);
    }
    let rest = s.strip_prefix("http://").ok_or(UrlError::Bad)?;
    if rest.is_empty() {
        return Err(UrlError::Bad);
    }
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    if authority.is_empty() {
        return Err(UrlError::Bad);
    }
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) => {
            let port: u16 = p.parse().map_err(|_| UrlError::Bad)?;
            if port == 0 {
                return Err(UrlError::Bad);
            }
            (h, port)
        }
        None => (authority, 80),
    };
    let addr = crate::net::parse_ipv4(host).ok_or(UrlError::Bad)?;
    let path = if path.is_empty() {
        String::from("/")
    } else {
        String::from(path)
    };
    Ok(Url {
        host: addr.octets(),
        port,
        path,
    })
}

pub fn split_head(buf: &[u8]) -> Option<(HttpHead, usize)> {
    let sep = buf.windows(4).position(|w| w == b"\r\n\r\n")?;
    let body_start = sep + 4;
    let headers = core::str::from_utf8(&buf[..sep]).ok()?;
    let mut content_length = None;
    for line in headers.split("\r\n").skip(1) {
        let Some((name, val)) = line.split_once(':') else {
            continue;
        };
        if name.eq_ignore_ascii_case("content-length") {
            content_length = val.trim().parse().ok();
        }
    }
    Some((HttpHead { content_length }, body_start))
}
