//! DNS A lookup: dotted hostname, cache of 4, one query at a time.

use alloc::string::String;

use smoltcp::socket::dns::{self, GetQueryResultError};
use smoltcp::wire::{DnsQueryType, IpAddress, Ipv4Address};

use super::Net;

const DNS_TIMEOUT_TICKS: u64 = 200;
const CACHE_CAP: usize = 4;

pub(super) struct Cache {
    slots: [Option<(String, Ipv4Address)>; CACHE_CAP],
    next: usize,
}

impl Cache {
    pub(super) fn new() -> Self {
        Self {
            slots: [None, None, None, None],
            next: 0,
        }
    }

    fn lookup(&self, key: &str) -> Option<Ipv4Address> {
        for slot in self.slots.iter() {
            if let Some((name, ip)) = slot {
                if name.eq_ignore_ascii_case(key) {
                    return Some(*ip);
                }
            }
        }
        None
    }

    fn store(&mut self, key: &str, ip: Ipv4Address) {
        let mut lower = String::from(key);
        lower.make_ascii_lowercase();
        self.slots[self.next] = Some((lower, ip));
        self.next = (self.next + 1) % CACHE_CAP;
    }
}

pub fn is_hostname(s: &str) -> bool {
    let s = s.strip_suffix('.').unwrap_or(s);
    if s.is_empty() || s.len() > 253 || !s.contains('.') {
        return false;
    }
    for lab in s.split('.') {
        if lab.is_empty() || lab.len() > 63 {
            return false;
        }
        if !lab
            .as_bytes()
            .iter()
            .all(|b| b.is_ascii_alphanumeric() || *b == b'-')
        {
            return false;
        }
    }
    true
}

pub(super) fn resolve(net: &mut Net, name: &str) -> Option<Ipv4Address> {
    if !net.has_dns {
        return None;
    }
    if let Some(ip) = net.dns_cache.lookup(name) {
        return Some(ip);
    }
    let handle = {
        let Net {
            iface,
            sockets,
            dns: dns_h,
            ..
        } = net;
        sockets
            .get_mut::<dns::Socket>(*dns_h)
            .start_query(iface.context(), name, DnsQueryType::A)
            .ok()?
    };
    let t0 = crate::clock::ticks();
    loop {
        super::enable_irq();
        net.poll_inner();
        match net
            .sockets
            .get_mut::<dns::Socket>(net.dns)
            .get_query_result(handle)
        {
            Ok(addrs) => {
                let IpAddress::Ipv4(ip) = addrs.iter().next().copied()?;
                net.dns_cache.store(name, ip);
                return Some(ip);
            }
            Err(GetQueryResultError::Pending) => {
                if crate::clock::ticks().saturating_sub(t0) >= DNS_TIMEOUT_TICKS {
                    net.sockets
                        .get_mut::<dns::Socket>(net.dns)
                        .cancel_query(handle);
                    return None;
                }
            }
            Err(GetQueryResultError::Failed) => return None,
        }
    }
}
