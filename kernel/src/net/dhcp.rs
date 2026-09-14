//! DHCP client: apply lease on poll; wait or static fallback on ping/get.

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write;

use smoltcp::socket::{dhcpv4, dns};
use smoltcp::wire::{IpAddress, IpCidr, Ipv4Address, Ipv4Cidr};

use super::{GATEWAY, Net, OUR_IP, PREFIX};

pub(super) const LEASE_TIMEOUT_TICKS: u64 = 300;

pub(super) enum Lease {
    Discovering,
    Leased(Ipv4Address),
    Static,
}

pub(super) fn on_poll(net: &mut Net) {
    let event = net.sockets.get_mut::<dhcpv4::Socket>(net.dhcp).poll();
    match event {
        None => {}
        Some(dhcpv4::Event::Deconfigured) => apply_deconfigured(net),
        Some(dhcpv4::Event::Configured(cfg)) => {
            let address = cfg.address;
            let router = cfg.router;
            let mut servers = Vec::new();
            for ip in cfg.dns_servers.iter() {
                servers.push(IpAddress::Ipv4(*ip));
            }
            apply_configured(net, address, router, &servers);
        }
    }
}

pub(super) fn ensure_lease(net: &mut Net) {
    match net.lease {
        Lease::Leased(_) | Lease::Static => {
            announce(net);
            return;
        }
        Lease::Discovering => {}
    }
    let t0 = crate::clock::ticks();
    loop {
        super::enable_irq();
        net.poll_inner();
        match net.lease {
            Lease::Leased(_) | Lease::Static => {
                announce(net);
                return;
            }
            Lease::Discovering => {}
        }
        if crate::clock::ticks().saturating_sub(t0) >= LEASE_TIMEOUT_TICKS {
            apply_static(net);
            announce(net);
            return;
        }
    }
}

fn apply_configured(
    net: &mut Net,
    address: Ipv4Cidr,
    router: Option<Ipv4Address>,
    servers: &[IpAddress],
) {
    net.iface.update_ip_addrs(|addrs| {
        addrs.clear();
        let _ = addrs.push(IpCidr::Ipv4(address));
    });
    net.iface.routes_mut().remove_default_ipv4_route();
    if let Some(gw) = router {
        let _ = net.iface.routes_mut().add_default_ipv4_route(gw);
    }
    net.sockets
        .get_mut::<dns::Socket>(net.dns)
        .update_servers(servers);
    net.has_dns = !servers.is_empty();
    net.lease = Lease::Leased(address.address());
}

fn apply_deconfigured(net: &mut Net) {
    net.iface.update_ip_addrs(|addrs| {
        addrs.clear();
    });
    net.iface.routes_mut().remove_default_ipv4_route();
    net.sockets
        .get_mut::<dns::Socket>(net.dns)
        .update_servers(&[]);
    net.has_dns = false;
    net.lease = Lease::Discovering;
}

fn apply_static(net: &mut Net) {
    net.iface.update_ip_addrs(|addrs| {
        addrs.clear();
        let _ = addrs.push(IpCidr::new(IpAddress::Ipv4(OUR_IP), PREFIX));
    });
    net.iface.routes_mut().remove_default_ipv4_route();
    let _ = net.iface.routes_mut().add_default_ipv4_route(GATEWAY);
    net.sockets
        .get_mut::<dns::Socket>(net.dns)
        .update_servers(&[]);
    net.has_dns = false;
    net.lease = Lease::Static;
}

fn announce(net: &mut Net) {
    if net.announced {
        return;
    }
    net.announced = true;
    match net.lease {
        Lease::Leased(ip) => {
            let o = ip.octets();
            let mut line = String::new();
            let _ = write!(line, "net: {}.{}.{}.{}\n", o[0], o[1], o[2], o[3]);
            crate::serial::write_str(&line);
        }
        Lease::Static => crate::serial::write_str("net: dhcp failed\n"),
        Lease::Discovering => {}
    }
}
