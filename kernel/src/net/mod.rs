//! In-kernel IPv4: DHCP, DNS A, ICMP echo, HTTP/1.0 GET.

pub mod dhcp;
pub mod dns;
pub mod e1000e;
pub mod http;
pub mod tls;
pub mod virtio_hal;
pub mod virtio_net;
pub mod virtio_pci;

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;
use core::fmt::Write;

use smoltcp::iface::{Config, Interface, SocketHandle, SocketSet};
use smoltcp::phy::{Device, DeviceCapabilities, RxToken, TxToken};
use smoltcp::socket::{dhcpv4, dns as dns_sock, icmp, tcp};
use smoltcp::time::Instant;
use smoltcp::wire::{EthernetAddress, Icmpv4Packet, Icmpv4Repr, IpAddress, Ipv4Address};
use spin::Mutex;

use crate::clock;
use crate::e1000e::E1000e;
use crate::http::{Host, Url};
use crate::virtio_net::VirtioNetDev;

pub use dns::is_hostname;

const OUR_IP: Ipv4Address = Ipv4Address::new(10, 0, 2, 15);
const GATEWAY: Ipv4Address = Ipv4Address::new(10, 0, 2, 2);
const PREFIX: u8 = 24;
const ICMP_IDENT: u16 = 0x22b;
const PING_COUNT: u16 = 4;
const TIMEOUT_TICKS: u64 = 200;
pub(super) const GET_TIMEOUT_TICKS: u64 = 500;
const CLOSE_TIMEOUT_TICKS: u64 = 100;
const ECHO_PAYLOAD: [u8; 16] = [0u8; 16];
pub(super) const BODY_CAP: usize = 32 * 1024;
const FIRST_LOCAL_PORT: u16 = 49152;

pub(super) struct Net {
    device: Nic,
    iface: Interface,
    pub(super) sockets: SocketSet<'static>,
    icmp: SocketHandle,
    pub(super) tcp: SocketHandle,
    dhcp: SocketHandle,
    dns: SocketHandle,
    next_port: u16,
    lease: dhcp::Lease,
    announced: bool,
    has_dns: bool,
    dns_cache: dns::Cache,
}

enum Nic {
    Virtio(VirtioNetDev),
    E1000e(E1000e),
}

impl Nic {
    fn probe() -> Option<Self> {
        if let Some(v) = VirtioNetDev::probe() {
            return Some(Nic::Virtio(v));
        }
        E1000e::probe().map(Nic::E1000e)
    }

    fn mac(&self) -> [u8; 6] {
        match self {
            Nic::Virtio(d) => d.mac(),
            Nic::E1000e(d) => d.mac(),
        }
    }
}

enum RxTok {
    Virtio(virtio_net::RxTok),
    E1000e(e1000e::RxTok),
}

enum TxTok<'a> {
    Virtio(virtio_net::TxTok<'a>),
    E1000e(e1000e::TxTok<'a>),
}

impl Device for Nic {
    type RxToken<'a> = RxTok;
    type TxToken<'a> = TxTok<'a>;

    fn receive(&mut self, timestamp: Instant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        match self {
            Nic::Virtio(d) => {
                let (rx, tx) = d.receive(timestamp)?;
                Some((RxTok::Virtio(rx), TxTok::Virtio(tx)))
            }
            Nic::E1000e(d) => {
                let (rx, tx) = d.receive(timestamp)?;
                Some((RxTok::E1000e(rx), TxTok::E1000e(tx)))
            }
        }
    }

    fn transmit(&mut self, timestamp: Instant) -> Option<Self::TxToken<'_>> {
        match self {
            Nic::Virtio(d) => Some(TxTok::Virtio(d.transmit(timestamp)?)),
            Nic::E1000e(d) => Some(TxTok::E1000e(d.transmit(timestamp)?)),
        }
    }

    fn capabilities(&self) -> DeviceCapabilities {
        match self {
            Nic::Virtio(d) => d.capabilities(),
            Nic::E1000e(d) => d.capabilities(),
        }
    }
}

impl RxToken for RxTok {
    fn consume<R, F>(self, f: F) -> R
    where
        F: FnOnce(&[u8]) -> R,
    {
        match self {
            RxTok::Virtio(t) => t.consume(f),
            RxTok::E1000e(t) => t.consume(f),
        }
    }
}

impl TxToken for TxTok<'_> {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        match self {
            TxTok::Virtio(t) => t.consume(len, f),
            TxTok::E1000e(t) => t.consume(len, f),
        }
    }
}

static NET: Mutex<Option<Net>> = Mutex::new(None);

pub struct PingReply {
    pub from: Ipv4Address,
    pub time_ms: u64,
}

pub enum PingError {
    NoNet,
    Failed,
}

pub enum HttpError {
    NoNet,
    Timeout,
    Failed,
    Https,
}

pub const ERR: u64 = u64::MAX;
pub const ERR_NO_NET: u64 = u64::MAX - 1;
pub const ERR_TIMEOUT: u64 = u64::MAX - 2;
pub const ERR_HTTPS: u64 = u64::MAX - 3;
pub const ERR_BAD_URL: u64 = u64::MAX - 4;

const URL_MAX: usize = 256;
const HTTP_GET_ARGS: usize = 32;

pub fn init() {
    let Some(mut device) = Nic::probe() else {
        return;
    };
    let mut config = Config::new(EthernetAddress(device.mac()).into());
    config.random_seed = clock::ticks();
    let now = Instant::from_millis(clock::millis() as i64);
    let iface = Interface::new(config, &mut device, now);

    let mut sockets = SocketSet::new(Vec::new());
    let icmp = sockets.add(icmp_socket());
    {
        let sock = sockets.get_mut::<icmp::Socket>(icmp);
        if sock.bind(icmp::Endpoint::Ident(ICMP_IDENT)).is_err() {
            return;
        }
    }
    let tcp = sockets.add(tcp_socket());
    let dhcp = sockets.add(dhcpv4::Socket::new());
    let dns_h = sockets.add(dns_sock::Socket::new(&[], vec![None]));

    *NET.lock() = Some(Net {
        device,
        iface,
        sockets,
        icmp,
        tcp,
        dhcp,
        dns: dns_h,
        next_port: FIRST_LOCAL_PORT,
        lease: dhcp::Lease::Discovering,
        announced: false,
        has_dns: false,
        dns_cache: dns::Cache::new(),
    });
}

pub fn poll() {
    let mut net = NET.lock();
    let Some(net) = net.as_mut() else {
        return;
    };
    net.poll_inner();
}

pub fn parse_ipv4(s: &str) -> Option<Ipv4Address> {
    let mut oct = [0u8; 4];
    let mut n = 0usize;
    for part in s.split('.') {
        if n == 4 || part.is_empty() {
            return None;
        }
        if !part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        oct[n] = part.parse().ok()?;
        n += 1;
    }
    if n != 4 {
        return None;
    }
    Some(Ipv4Address::new(oct[0], oct[1], oct[2], oct[3]))
}

pub fn ping_target(name: &str) -> Result<Vec<PingReply>, PingError> {
    let mut net = NET.lock();
    let net = net.as_mut().ok_or(PingError::NoNet)?;
    dhcp::ensure_lease(net);
    let dest = if let Some(ip) = parse_ipv4(name) {
        ip
    } else if dns::is_hostname(name) {
        dns::resolve(net, name).ok_or(PingError::Failed)?
    } else {
        return Err(PingError::Failed);
    };
    ping_locked(net, dest)
}

fn ping_locked(net: &mut Net, dest: Ipv4Address) -> Result<Vec<PingReply>, PingError> {
    let remote = IpAddress::Ipv4(dest);
    let mut replies = Vec::new();

    for seq in 0..PING_COUNT {
        enable_irq();
        net.poll_inner();
        send_echo(net, remote, seq)?;
        let t0 = clock::ticks();
        loop {
            enable_irq();
            net.poll_inner();
            if let Some(from) = take_echo_reply(net, seq) {
                let time_ms = clock::ticks().saturating_sub(t0) * (1000 / clock::HZ);
                let IpAddress::Ipv4(from) = from;
                replies.push(PingReply { from, time_ms });
                break;
            }
            if clock::ticks().saturating_sub(t0) >= TIMEOUT_TICKS {
                break;
            }
        }
    }
    Ok(replies)
}

pub fn http_get(url: &Url) -> Result<Vec<u8>, HttpError> {
    let mut net = NET.lock();
    let net = net.as_mut().ok_or(HttpError::NoNet)?;
    dhcp::ensure_lease(net);
    close_tcp(net);

    let host = match &url.host {
        Host::V4(o) => Ipv4Address::new(o[0], o[1], o[2], o[3]),
        Host::Name(n) => dns::resolve(net, n).ok_or(HttpError::Failed)?,
    };
    let remote = (IpAddress::Ipv4(host), url.port);
    let local = net.next_port;
    net.next_port = if net.next_port == u16::MAX {
        FIRST_LOCAL_PORT
    } else {
        net.next_port + 1
    };

    {
        let Net {
            iface,
            sockets,
            tcp,
            ..
        } = net;
        sockets
            .get_mut::<tcp::Socket>(*tcp)
            .connect(iface.context(), remote, local)
            .map_err(|_| HttpError::Failed)?;
    }

    if url.scheme == crate::http::Scheme::Https {
        return tls::tls_get(net, url);
    }

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
    let mut sent = 0;
    let mut rx = Vec::new();
    let mut scratch = [0u8; 512];

    loop {
        enable_irq();
        if clock::ticks().saturating_sub(t0) >= GET_TIMEOUT_TICKS {
            abort_tcp(net);
            return Err(HttpError::Timeout);
        }
        net.poll_inner();

        let mut send_err = false;
        let mut recv_err = false;
        let mut dead = false;
        let recv_done;
        {
            let sock = net.sockets.get_mut::<tcp::Socket>(net.tcp);
            dead = !sock.is_open();
            if sent < req.len() && sock.may_send() {
                match sock.send_slice(&req.as_bytes()[sent..]) {
                    Ok(n) => sent += n,
                    Err(_) => send_err = true,
                }
            }
            if sock.can_recv() {
                match sock.recv_slice(&mut scratch) {
                    Ok(n) if n > 0 => {
                        let cap = BODY_CAP + 2048;
                        if rx.len() < cap {
                            let take = n.min(cap - rx.len());
                            rx.extend_from_slice(&scratch[..take]);
                        }
                    }
                    Ok(_) => {}
                    Err(tcp::RecvError::Finished) => {}
                    Err(_) => recv_err = true,
                }
            }
            // SYN-SENT has may_recv=false; only treat as closed after we sent or the socket died.
            recv_done = (sent > 0 || dead) && !sock.may_recv() && !sock.can_recv();
        }
        if send_err || recv_err {
            abort_tcp(net);
            return Err(HttpError::Failed);
        }
        if dead && sent == 0 {
            abort_tcp(net);
            return Err(HttpError::Failed);
        }

        if let Some((head, body_start)) = crate::http::split_head(&rx) {
            let body = if body_start > rx.len() {
                &[][..]
            } else {
                &rx[body_start..]
            };
            if let Some(len) = head.content_length {
                let want = len.min(BODY_CAP);
                if body.len() >= want {
                    abort_tcp(net);
                    return Ok(body[..want].to_vec());
                }
            } else if recv_done {
                abort_tcp(net);
                return Ok(body[..body.len().min(BODY_CAP)].to_vec());
            }
        } else if recv_done {
            abort_tcp(net);
            return Err(HttpError::Failed);
        }
    }
}

fn icmp_socket() -> icmp::Socket<'static> {
    let rx_meta = Vec::leak(vec![icmp::PacketMetadata::EMPTY; 4]);
    let rx_data = Vec::leak(vec![0u8; 256]);
    let tx_meta = Vec::leak(vec![icmp::PacketMetadata::EMPTY; 4]);
    let tx_data = Vec::leak(vec![0u8; 256]);
    icmp::Socket::new(
        icmp::PacketBuffer::new(rx_meta, rx_data),
        icmp::PacketBuffer::new(tx_meta, tx_data),
    )
}

fn tcp_socket() -> tcp::Socket<'static> {
    tcp::Socket::new(
        tcp::SocketBuffer::new(vec![0; 8192]),
        tcp::SocketBuffer::new(vec![0; 2048]),
    )
}

pub(super) fn abort_tcp(net: &mut Net) {
    net.sockets.get_mut::<tcp::Socket>(net.tcp).abort();
}

fn close_tcp(net: &mut Net) {
    if !net.sockets.get_mut::<tcp::Socket>(net.tcp).is_open() {
        return;
    }
    abort_tcp(net);
    let t0 = clock::ticks();
    while net.sockets.get_mut::<tcp::Socket>(net.tcp).is_open() {
        enable_irq();
        if clock::ticks().saturating_sub(t0) >= CLOSE_TIMEOUT_TICKS {
            break;
        }
        net.poll_inner();
    }
}

fn send_echo(net: &mut Net, remote: IpAddress, seq: u16) -> Result<(), PingError> {
    let checksum = net.device.capabilities().checksum;
    let sock = net.sockets.get_mut::<icmp::Socket>(net.icmp);
    if !sock.is_open() {
        sock.bind(icmp::Endpoint::Ident(ICMP_IDENT))
            .map_err(|_| PingError::Failed)?;
    }
    let repr = Icmpv4Repr::EchoRequest {
        ident: ICMP_IDENT,
        seq_no: seq,
        data: &ECHO_PAYLOAD,
    };
    let payload = sock
        .send(repr.buffer_len(), remote)
        .map_err(|_| PingError::Failed)?;
    let mut packet = Icmpv4Packet::new_unchecked(payload);
    repr.emit(&mut packet, &checksum);
    Ok(())
}

fn take_echo_reply(net: &mut Net, seq: u16) -> Option<IpAddress> {
    let checksum = net.device.capabilities().checksum;
    let sock = net.sockets.get_mut::<icmp::Socket>(net.icmp);
    if !sock.can_recv() {
        return None;
    }
    let (payload, from) = sock.recv().ok()?;
    let packet = Icmpv4Packet::new_checked(payload).ok()?;
    match Icmpv4Repr::parse(&packet, &checksum) {
        Ok(Icmpv4Repr::EchoReply { seq_no, .. }) if seq_no == seq => Some(from),
        _ => None,
    }
}

impl Net {
    pub(super) fn poll_inner(&mut self) {
        let now = Instant::from_millis(clock::millis() as i64);
        let _ = self.iface.poll(now, &mut self.device, &mut self.sockets);
        dhcp::on_poll(self);
    }
}

pub(super) fn enable_irq() {
    x86_64::instructions::interrupts::enable();
}

const NAME_MAX: usize = 256;
const PING_ARGS: usize = 32;

pub fn sys_net_ping(args_ptr: u64) -> u64 {
    if !crate::vmm::user_slice_ok(args_ptr, PING_ARGS as u64) {
        return ERR;
    }
    let mut raw = [0u8; PING_ARGS];
    if crate::fd::copy_from_user(args_ptr, PING_ARGS, &mut raw).is_err() {
        return ERR;
    }
    let name_ptr = u64::from_le_bytes(raw[0..8].try_into().unwrap());
    let name_len = u64::from_le_bytes(raw[8..16].try_into().unwrap());
    let buf = u64::from_le_bytes(raw[16..24].try_into().unwrap());
    let len = u64::from_le_bytes(raw[24..32].try_into().unwrap());
    if name_len == 0 || name_len > NAME_MAX as u64 {
        return ERR;
    }
    if !crate::vmm::user_slice_ok(name_ptr, name_len) {
        return ERR;
    }
    let nname = name_len as usize;
    let mut name_bytes = [0u8; NAME_MAX];
    if crate::fd::copy_from_user(name_ptr, nname, &mut name_bytes).is_err() {
        return ERR;
    }
    let name = match core::str::from_utf8(&name_bytes[..nname]) {
        Ok(s) => s,
        Err(_) => return ERR,
    };
    let replies = match ping_target(name) {
        Err(PingError::NoNet) => return ERR_NO_NET,
        Err(PingError::Failed) => return ERR,
        Ok(r) => r,
    };
    let cap = (len as usize) / 8;
    let n = replies.len().min(cap).min(4);
    if n > 0 && !crate::vmm::user_slice_ok(buf, (n * 8) as u64) {
        return ERR;
    }
    let mut packed_out = [0u8; 32];
    for (i, r) in replies.iter().take(n).enumerate() {
        let oct = r.from.octets();
        let off = i * 8;
        packed_out[off..off + 4].copy_from_slice(&oct);
        let ms = u32::try_from(r.time_ms).unwrap_or(u32::MAX);
        packed_out[off + 4..off + 8].copy_from_slice(&ms.to_le_bytes());
    }
    if n > 0 && crate::fd::copy_to_user(buf, &packed_out[..n * 8]).is_err() {
        return ERR;
    }
    n as u64
}

pub fn sys_http_get(args_ptr: u64) -> u64 {
    if !crate::vmm::user_slice_ok(args_ptr, HTTP_GET_ARGS as u64) {
        return ERR;
    }
    let mut raw = [0u8; HTTP_GET_ARGS];
    if crate::fd::copy_from_user(args_ptr, HTTP_GET_ARGS, &mut raw).is_err() {
        return ERR;
    }
    let url_ptr = u64::from_le_bytes(raw[0..8].try_into().unwrap());
    let url_len = u64::from_le_bytes(raw[8..16].try_into().unwrap());
    let buf_ptr = u64::from_le_bytes(raw[16..24].try_into().unwrap());
    let buf_len = u64::from_le_bytes(raw[24..32].try_into().unwrap());
    if url_len == 0 || url_len > URL_MAX as u64 {
        return ERR_BAD_URL;
    }
    if !crate::vmm::user_slice_ok(url_ptr, url_len) {
        return ERR;
    }
    let nurl = url_len as usize;
    let mut url_bytes = [0u8; URL_MAX];
    if crate::fd::copy_from_user(url_ptr, nurl, &mut url_bytes).is_err() {
        return ERR;
    }
    let url_str = match core::str::from_utf8(&url_bytes[..nurl]) {
        Ok(s) => s,
        Err(_) => return ERR_BAD_URL,
    };
    let url = match crate::http::parse_url(url_str) {
        Err(crate::http::UrlError::Bad) => return ERR_BAD_URL,
        Ok(u) => u,
    };
    let body = match http_get(&url) {
        Err(HttpError::NoNet) => return ERR_NO_NET,
        Err(HttpError::Timeout) => return ERR_TIMEOUT,
        Err(HttpError::Https) => return ERR_HTTPS,
        Err(HttpError::Failed) => return ERR,
        Ok(b) => b,
    };
    let n = body.len().min(buf_len as usize);
    if n > 0 {
        if !crate::vmm::user_slice_ok(buf_ptr, n as u64) {
            return ERR;
        }
        if crate::fd::copy_to_user(buf_ptr, &body[..n]).is_err() {
            return ERR;
        }
    }
    n as u64
}
