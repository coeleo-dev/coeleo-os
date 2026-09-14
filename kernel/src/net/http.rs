//! HTTP/1.0 URL and header split. No TLS.

use alloc::string::String;

pub enum Host {
    V4([u8; 4]),
    Name(String),
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Scheme {
    Http,
    Https,
}

pub struct Url {
    pub scheme: Scheme,
    pub host: Host,
    pub port: u16,
    pub path: String,
}

pub enum UrlError {
    Bad,
}

pub struct HttpHead {
    pub content_length: Option<usize>,
}

pub fn parse_url(s: &str) -> Result<Url, UrlError> {
    let (scheme, default_port, rest) = if let Some(r) = s.strip_prefix("http://") {
        (Scheme::Http, 80, r)
    } else if let Some(r) = s.strip_prefix("https://") {
        (Scheme::Https, 443, r)
    } else {
        return Err(UrlError::Bad);
    };
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
        None => (authority, default_port),
    };
    let host = if let Some(addr) = crate::net::parse_ipv4(host) {
        Host::V4(addr.octets())
    } else if crate::net::is_hostname(host) {
        Host::Name(String::from(host))
    } else {
        return Err(UrlError::Bad);
    };
    let path = if path.is_empty() {
        String::from("/")
    } else {
        String::from(path)
    };
    Ok(Url {
        scheme,
        host,
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
