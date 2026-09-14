//! TLS 1.2/1.3 client for HTTPS GET. Trust: baked test CA. RNG: RDRAND. Clock: RTC.

use alloc::string::String;
use alloc::sync::Arc;
use alloc::vec::Vec;
use core::fmt::Write;
use core::time::Duration;

use getrandom::Error;
use rustls::client::UnbufferedClientConnection;
use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
use rustls::time_provider::TimeProvider;
use rustls::unbuffered::ConnectionState;
use rustls::{ClientConfig, RootCertStore};
use smoltcp::socket::tcp;

use super::{abort_tcp, enable_irq, HttpError, Net, BODY_CAP, GET_TIMEOUT_TICKS};
use crate::clock;
use crate::http::{Host, Url};

static TEST_CA_DER: &[u8] = include_bytes!("ca.der");

fn rdrand_fill(dest: &mut [u8]) -> Result<(), Error> {
    fill_rdrand(dest).map_err(|_| Error::UNSUPPORTED)
}

getrandom::register_custom_getrandom!(rdrand_fill);

fn fill_rdrand(dest: &mut [u8]) -> Result<(), ()> {
    let mut i = 0usize;
    while i < dest.len() {
        let mut v: u64 = 0;
        let ok: u8;
        unsafe {
            core::arch::asm!(
                "rdrand {val}",
                "setc {ok}",
                val = out(reg) v,
                ok = out(reg_byte) ok,
                options(nomem, nostack),
            );
        }
        if ok == 0 {
            return Err(());
        }
        let n = core::cmp::min(8, dest.len() - i);
        dest[i..i + n].copy_from_slice(&v.to_le_bytes()[..n]);
        i += n;
    }
    Ok(())
}

#[derive(Debug)]
struct RtcTime;

impl TimeProvider for RtcTime {
    fn current_time(&self) -> Option<UnixTime> {
        let secs = crate::boot::rtc::unix_timestamp()?;
        Some(UnixTime::since_unix_epoch(Duration::from_secs(secs)))
    }
}

pub fn tls_get(net: &mut Net, url: &Url) -> Result<Vec<u8>, HttpError> {
    let mut root_store = RootCertStore::empty();
    root_store
        .add(CertificateDer::from(TEST_CA_DER))
        .map_err(|_| HttpError::Https)?;

    let config = ClientConfig::builder_with_details(
        Arc::new(rustls_rustcrypto::provider()),
        Arc::new(RtcTime),
    )
    .with_safe_default_protocol_versions()
    .map_err(|_| HttpError::Https)?
    .with_root_certificates(root_store)
    .with_no_client_auth();

    let server_name = match &url.host {
        Host::V4(o) => {
            let mut ip_str = String::new();
            let _ = write!(ip_str, "{}.{}.{}.{}", o[0], o[1], o[2], o[3]);
            ServerName::try_from(ip_str.as_str())
                .map_err(|_| HttpError::Https)?
                .to_owned()
        }
        Host::Name(n) => ServerName::try_from(n.as_str())
            .map_err(|_| HttpError::Https)?
            .to_owned(),
    };

    let mut conn = UnbufferedClientConnection::new(Arc::new(config), server_name)
        .map_err(|_| HttpError::Https)?;

    let mut req = String::new();
    match &url.host {
        Host::V4(o) => {
            let _ = write!(
                req,
                "GET {} HTTP/1.0\r\nHost: {}.{}.{}.{}\r\n\r\n",
                url.path, o[0], o[1], o[2], o[3]
            );
        }
        Host::Name(n) => {
            let _ = write!(req, "GET {} HTTP/1.0\r\nHost: {}\r\n\r\n", url.path, n);
        }
    }

    let t0 = clock::ticks();
    let mut incoming = [0u8; 8192];
    let mut incoming_len = 0usize;
    let mut outgoing = [0u8; 8192];
    let mut outgoing_len = 0usize;
    let mut outgoing_sent = 0usize;
    let mut req_queued = false;
    let mut rx_body = Vec::new();

    loop {
        enable_irq();
        if clock::ticks().saturating_sub(t0) >= GET_TIMEOUT_TICKS {
            abort_tcp(net);
            return Err(HttpError::Timeout);
        }
        net.poll_inner();

        // 1. Send outgoing bytes over TCP if pending
        if outgoing_sent < outgoing_len {
            let sock = net.sockets.get_mut::<tcp::Socket>(net.tcp);
            if !sock.is_open() {
                abort_tcp(net);
                return Err(HttpError::Failed);
            }
            if sock.may_send() {
                match sock.send_slice(&outgoing[outgoing_sent..outgoing_len]) {
                    Ok(n) => outgoing_sent += n,
                    Err(_) => {
                        abort_tcp(net);
                        return Err(HttpError::Failed);
                    }
                }
            }
        }

        // 2. Receive bytes from TCP into incoming buffer
        {
            let sock = net.sockets.get_mut::<tcp::Socket>(net.tcp);
            if sock.can_recv() && incoming_len < incoming.len() {
                match sock.recv_slice(&mut incoming[incoming_len..]) {
                    Ok(n) if n > 0 => incoming_len += n,
                    Ok(_) => {}
                    Err(tcp::RecvError::Finished) => {}
                    Err(_) => {
                        abort_tcp(net);
                        return Err(HttpError::Failed);
                    }
                }
            }
        }

        // 3. Drive TLS record processing only when not blocked on sending outgoing TLS records
        if outgoing_sent >= outgoing_len {
            let status = conn.process_tls_records(&mut incoming[..incoming_len]);
            let mut total_discard = status.discard;

            let state = match status.state {
                Ok(s) => s,
                Err(_) => {
                    abort_tcp(net);
                    return Err(HttpError::Https);
                }
            };

            let mut should_break = false;
            match state {
                ConnectionState::EncodeTlsData(mut enc) => {
                    match enc.encode(&mut outgoing) {
                        Ok(n) => {
                            outgoing_len = n;
                            outgoing_sent = 0;
                        }
                        Err(_) => {
                            abort_tcp(net);
                            return Err(HttpError::Https);
                        }
                    }
                }
                ConnectionState::TransmitTlsData(trans) => {
                    trans.done();
                }
                ConnectionState::BlockedHandshake => {
                    // Need more data from TCP socket
                }
                ConnectionState::WriteTraffic(mut wt) => {
                    if !req_queued {
                        match wt.encrypt(req.as_bytes(), &mut outgoing) {
                            Ok(n) => {
                                outgoing_len = n;
                                outgoing_sent = 0;
                                req_queued = true;
                            }
                            Err(_) => {
                                abort_tcp(net);
                                return Err(HttpError::Https);
                            }
                        }
                    }
                }
                ConnectionState::ReadTraffic(mut rt) => {
                    while let Some(res) = rt.next_record() {
                        match res {
                            Ok(rec) => {
                                total_discard += rec.discard;
                                let cap = BODY_CAP + 2048;
                                if rx_body.len() < cap {
                                    let take = rec.payload.len().min(cap - rx_body.len());
                                    rx_body.extend_from_slice(&rec.payload[..take]);
                                }
                            }
                            Err(_) => {
                                abort_tcp(net);
                                return Err(HttpError::Https);
                            }
                        }
                    }
                }
                ConnectionState::ReadEarlyData(_) => {}
                ConnectionState::PeerClosed | ConnectionState::Closed => {
                    abort_tcp(net);
                    should_break = true;
                }
                _ => {}
            }

            if total_discard > 0 && total_discard <= incoming_len {
                incoming.copy_within(total_discard..incoming_len, 0);
                incoming_len -= total_discard;
            }

            if should_break {
                break;
            }
        }

        // Check if we have received a full HTTP response body
        if req_queued {
            if let Some((head, body_start)) = crate::http::split_head(&rx_body) {
                let body = if body_start > rx_body.len() {
                    &[][..]
                } else {
                    &rx_body[body_start..]
                };
                if let Some(len) = head.content_length {
                    let want = len.min(BODY_CAP);
                    if body.len() >= want {
                        abort_tcp(net);
                        return Ok(body[..want].to_vec());
                    }
                }
            }
        }
    }

    if let Some((head, body_start)) = crate::http::split_head(&rx_body) {
        let body = if body_start > rx_body.len() {
            &[][..]
        } else {
            &rx_body[body_start..]
        };
        let want = head.content_length.unwrap_or(body.len()).min(BODY_CAP);
        return Ok(body[..body.len().min(want)].to_vec());
    }

    Err(HttpError::Failed)
}
