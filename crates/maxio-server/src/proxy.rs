//! Trusted reverse-proxy client IP resolution.
//!
//! `X-Forwarded-For` is only honored when the direct peer is listed in
//! `MAXIO_TRUSTED_PROXIES`.

use axum::http::HeaderMap;
use std::net::{IpAddr, SocketAddr};

#[derive(Debug, Clone, Default)]
pub struct TrustedProxies {
    networks: Vec<Network>,
}

#[derive(Debug, Clone)]
struct Network {
    addr: IpAddr,
    prefix_len: u8,
}

impl TrustedProxies {
    pub fn parse(spec: &str) -> Self {
        let mut networks = Vec::new();
        for part in spec.split(',') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            if let Some(net) = parse_network(part) {
                networks.push(net);
            } else {
                tracing::warn!("ignoring invalid trusted proxy CIDR: {part}");
            }
        }
        Self { networks }
    }

    pub fn is_empty(&self) -> bool {
        self.networks.is_empty()
    }

    pub fn contains(&self, ip: IpAddr) -> bool {
        self.networks.iter().any(|n| n.contains(ip))
    }

    /// Resolve the client IP for rate limiting and console login.
    ///
    /// When the peer is not trusted, returns the peer address. When trusted,
    /// walks `X-Forwarded-For` from the right, stripping trusted hops until the
    /// leftmost untrusted address remains.
    pub fn client_ip(&self, headers: &HeaderMap, peer: &SocketAddr) -> String {
        let peer_ip = peer.ip();
        if self.is_empty() || !self.contains(peer_ip) {
            return peer_ip.to_string();
        }

        let Some(forwarded) = headers.get("x-forwarded-for").and_then(|v| v.to_str().ok()) else {
            return peer_ip.to_string();
        };

        let mut chain: Vec<IpAddr> = forwarded
            .split(',')
            .filter_map(|s| s.trim().parse().ok())
            .collect();
        chain.push(peer_ip);

        while chain.len() > 1 {
            let last = *chain.last().unwrap();
            if self.contains(last) || last == peer_ip {
                chain.pop();
            } else {
                break;
            }
        }

        chain
            .first()
            .map(|ip| ip.to_string())
            .unwrap_or_else(|| peer_ip.to_string())
    }
}

impl Network {
    fn contains(&self, ip: IpAddr) -> bool {
        match (self.addr, ip) {
            (IpAddr::V4(net), IpAddr::V4(addr)) => {
                let mask = if self.prefix_len == 0 {
                    0
                } else {
                    !0u32 << (32 - self.prefix_len)
                };
                let net_bits = u32::from_be_bytes(net.octets());
                let addr_bits = u32::from_be_bytes(addr.octets());
                (net_bits & mask) == (addr_bits & mask)
            }
            (IpAddr::V6(net), IpAddr::V6(addr)) => {
                let net_segments = net.octets();
                let addr_segments = addr.octets();
                let full_bytes = (self.prefix_len / 8) as usize;
                let remainder = self.prefix_len % 8;
                if net_segments[..full_bytes] != addr_segments[..full_bytes] {
                    return false;
                }
                if remainder == 0 {
                    return true;
                }
                let mask = 0xffu8 << (8 - remainder);
                (net_segments[full_bytes] & mask) == (addr_segments[full_bytes] & mask)
            }
            _ => false,
        }
    }
}

fn parse_network(spec: &str) -> Option<Network> {
    if let Some((addr, prefix)) = spec.split_once('/') {
        let addr: IpAddr = addr.trim().parse().ok()?;
        let prefix_len: u8 = prefix.trim().parse().ok()?;
        let max = match addr {
            IpAddr::V4(_) => 32,
            IpAddr::V6(_) => 128,
        };
        if prefix_len > max {
            return None;
        }
        Some(Network { addr, prefix_len })
    } else {
        let addr: IpAddr = spec.parse().ok()?;
        let prefix_len = match addr {
            IpAddr::V4(_) => 32,
            IpAddr::V6(_) => 128,
        };
        Some(Network { addr, prefix_len })
    }
}

pub fn client_ip_from_request(
    request: &axum::extract::Request,
    trusted: &TrustedProxies,
) -> String {
    let peer = request
        .extensions()
        .get::<axum::extract::ConnectInfo<SocketAddr>>()
        .map(|c| c.0);
    let headers = request.headers();
    match peer {
        Some(addr) => trusted.client_ip(headers, &addr),
        None => "unknown".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;
    use std::net::Ipv4Addr;

    fn peer(ip: &str) -> SocketAddr {
        format!("{ip}:9000").parse().unwrap()
    }

    #[test]
    fn ignores_forwarded_without_trusted_proxies() {
        let trusted = TrustedProxies::default();
        let mut headers = HeaderMap::new();
        headers.insert("x-forwarded-for", HeaderValue::from_static("203.0.113.50"));
        assert_eq!(trusted.client_ip(&headers, &peer("10.0.0.5")), "10.0.0.5");
    }

    #[test]
    fn uses_forwarded_when_peer_is_trusted() {
        let trusted = TrustedProxies::parse("10.0.0.0/8");
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-forwarded-for",
            HeaderValue::from_static("203.0.113.50, 10.0.0.1"),
        );
        assert_eq!(
            trusted.client_ip(&headers, &peer("10.0.0.1")),
            "203.0.113.50"
        );
    }

    #[test]
    fn parses_ipv4_cidr() {
        let trusted = TrustedProxies::parse("192.168.1.10");
        assert!(trusted.contains("192.168.1.10".parse().unwrap()));
        assert!(!trusted.contains("192.168.1.11".parse().unwrap()));
    }

    #[test]
    fn v4_network_matching() {
        let trusted = TrustedProxies::parse("10.0.0.0/8");
        assert!(trusted.contains(IpAddr::V4(Ipv4Addr::new(10, 1, 2, 3))));
        assert!(!trusted.contains(IpAddr::V4(Ipv4Addr::new(11, 0, 0, 1))));
    }

    #[test]
    fn v6_network_matching() {
        let trusted = TrustedProxies::parse("2001:db8::/32");
        assert!(trusted.contains("2001:db8:1::1".parse().unwrap()));
        assert!(!trusted.contains("2001:db9::1".parse().unwrap()));
    }
}
