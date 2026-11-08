use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::sync::{Arc, LazyLock};

use ipnet::{Ipv4Net, Ipv6Net};
use reqwest::dns::{Addrs, Name, Resolve, Resolving};

static BLOCKED_IPV4: LazyLock<Vec<Ipv4Net>> = LazyLock::new(|| {
    vec![
        "0.0.0.0/8".parse().expect("valid CIDR"), // "This host on this network"
        "10.0.0.0/8".parse().expect("valid CIDR"), // RFC 1918 Private
        "100.64.0.0/10".parse().expect("valid CIDR"), // RFC 6598 Shared CGNAT
        "127.0.0.0/8".parse().expect("valid CIDR"), // RFC 1122 Loopback
        "169.254.0.0/16".parse().expect("valid CIDR"), // RFC 3927 Link-Local / Cloud Metadata
        "172.16.0.0/12".parse().expect("valid CIDR"), // RFC 1918 Private
        "192.0.0.0/24".parse().expect("valid CIDR"), // RFC 6890 IETF Protocol Assignments
        "192.0.2.0/24".parse().expect("valid CIDR"), // RFC 5737 TEST-NET-1
        "192.88.99.0/24".parse().expect("valid CIDR"), // RFC 7526 6to4 Relay Anycast
        "192.168.0.0/16".parse().expect("valid CIDR"), // RFC 1918 Private
        "198.18.0.0/15".parse().expect("valid CIDR"), // RFC 2544 Benchmarking
        "198.51.100.0/24".parse().expect("valid CIDR"), // RFC 5737 TEST-NET-2
        "203.0.113.0/24".parse().expect("valid CIDR"), // RFC 5737 TEST-NET-3
        "224.0.0.0/4".parse().expect("valid CIDR"), // RFC 5771 Multicast
        "240.0.0.0/4".parse().expect("valid CIDR"), // RFC 1112 Reserved for Future Use
        "255.255.255.255/32".parse().expect("valid CIDR"), // RFC 919 Limited Broadcast
    ]
});

static BLOCKED_IPV6: LazyLock<Vec<Ipv6Net>> = LazyLock::new(|| {
    vec![
        "::/128".parse().expect("valid CIDR"),    // Unspecified address
        "::1/128".parse().expect("valid CIDR"),   // Loopback
        "100::/64".parse().expect("valid CIDR"),  // RFC 6666 Discard-only
        "fc00::/7".parse().expect("valid CIDR"),  // RFC 4193 Unique Local (ULA)
        "fe80::/10".parse().expect("valid CIDR"), // RFC 4291 Link-Local Unicast
        "ff00::/8".parse().expect("valid CIDR"),  // RFC 4291 Multicast
        "64:ff9b::/96".parse().expect("valid CIDR"), // RFC 6052 NAT64 Well-Known Prefix
        "64:ff9b:1::/48".parse().expect("valid CIDR"), // RFC 8215 Local-Use NAT64
        "2001:2::/48".parse().expect("valid CIDR"), // RFC 5180 BMWG Benchmarking
        "2001:10::/28".parse().expect("valid CIDR"), // RFC 4843 Deprecated ORCHID
        "2001:20::/28".parse().expect("valid CIDR"), // RFC 7343 ORCHIDv2
        "2001:db8::/32".parse().expect("valid CIDR"), // RFC 3849 Documentation
    ]
});

/// Prefixes that remain strictly BLOCKED even when an origin is listed in ATIHSRAH_PRIVATE_ORIGINS.
///
/// Cloud metadata (169.254.0.0/16), multicast, broadcast, and unspecified networks
/// must never be reachable by proxy requests under any circumstances.
static BLOCKED_PRIVATE_IPV4: LazyLock<Vec<Ipv4Net>> = LazyLock::new(|| {
    vec![
        "0.0.0.0/8".parse().expect("valid CIDR"), // "This host on this network"
        "169.254.0.0/16".parse().expect("valid CIDR"), // RFC 3927 Link-Local / Cloud Metadata
        "224.0.0.0/4".parse().expect("valid CIDR"), // RFC 5771 Multicast
        "255.255.255.255/32".parse().expect("valid CIDR"), // RFC 919 Limited Broadcast
    ]
});

static BLOCKED_PRIVATE_IPV6: LazyLock<Vec<Ipv6Net>> = LazyLock::new(|| {
    vec![
        "::/128".parse().expect("valid CIDR"),    // Unspecified
        "fe80::/10".parse().expect("valid CIDR"), // RFC 4291 Link-Local Unicast
        "ff00::/8".parse().expect("valid CIDR"),  // RFC 4291 Multicast
    ]
});

/// Evaluates whether an IP address is a safe, publicly-routable internet IP.
///
/// Blocks private, loopback, link-local, cloud metadata, multicast, broadcast,
/// test networks, and documentation networks for both IPv4 and IPv6.
///
/// Crucially, IPv4-mapped IPv6 addresses (e.g. `::ffff:192.168.1.1`) and IPv4-compatible
/// IPv6 addresses are unmapped and re-checked against all IPv4 rules to prevent
/// encapsulation bypasses.
pub fn is_safe_public_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_safe_public_ipv4(v4),
        IpAddr::V6(v6) => {
            let octets = v6.octets();

            // 1. Check for IPv4-mapped addresses (::ffff:a.b.c.d)
            if let Some(mapped_v4) = v6.to_ipv4_mapped() {
                if !is_safe_public_ipv4(&mapped_v4) {
                    return false;
                }
            }

            // 2. Check for IPv4-compatible addresses (::a.b.c.d)
            if let Some(compat_v4) = v6.to_ipv4() {
                if !is_safe_public_ipv4(&compat_v4) {
                    return false;
                }
            }

            // 3. Check for SIIT IPv4-translated addresses (::ffff:0:a.b.c.d / RFC 7915)
            if octets[0..8] == [0; 8] && octets[8..10] == [0xff, 0xff] && octets[10..12] == [0, 0] {
                let siit_v4 = Ipv4Addr::new(octets[12], octets[13], octets[14], octets[15]);
                if !is_safe_public_ipv4(&siit_v4) {
                    return false;
                }
            }

            // 4. Check for 6to4 addresses (2002::/16 / RFC 3056)
            if octets[0] == 0x20 && octets[1] == 0x02 {
                let embedded_v4 = Ipv4Addr::new(octets[2], octets[3], octets[4], octets[5]);
                if !is_safe_public_ipv4(&embedded_v4) {
                    return false;
                }
            }

            // 5. Check for Teredo tunneling (2001:0000::/32 / RFC 4380)
            if octets[0] == 0x20 && octets[1] == 0x01 && octets[2] == 0x00 && octets[3] == 0x00 {
                let teredo_v4 = Ipv4Addr::new(!octets[12], !octets[13], !octets[14], !octets[15]);
                if !is_safe_public_ipv4(&teredo_v4) {
                    return false;
                }
            }

            // 6. Check against IPv6 blocked prefixes
            for net in BLOCKED_IPV6.iter() {
                if net.contains(v6) {
                    return false;
                }
            }

            true
        }
    }
}

fn is_safe_public_ipv4(v4: &Ipv4Addr) -> bool {
    for net in BLOCKED_IPV4.iter() {
        if net.contains(v4) {
            return false;
        }
    }
    true
}

/// Evaluates whether an IP address is permissible for an explicitly listed private origin.
///
/// Permitted: RFC 1918 private subnets (10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16),
/// loopback (127.0.0.0/8, ::1), ULA (fc00::/7), CGNAT (100.64.0.0/10), and public internet IPs.
///
/// Strictly BLOCKED: Cloud metadata (169.254.0.0/16), link-local unicast (fe80::/10),
/// multicast (224.0.0.0/4, ff00::/8), broadcast (255.255.255.255), and unspecified (0.0.0.0/8, ::/128).
pub fn is_safe_private_ip(ip: &IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => is_safe_private_ipv4(v4),
        IpAddr::V6(v6) => {
            let octets = v6.octets();

            // 1. Check for IPv4-mapped addresses (::ffff:a.b.c.d)
            if let Some(mapped_v4) = v6.to_ipv4_mapped() {
                if !is_safe_private_ipv4(&mapped_v4) {
                    return false;
                }
            }

            // 2. Check for IPv4-compatible addresses (::a.b.c.d)
            if let Some(compat_v4) = v6.to_ipv4() {
                if !is_safe_private_ipv4(&compat_v4) {
                    return false;
                }
            }

            // 3. Check for SIIT IPv4-translated addresses (::ffff:0:a.b.c.d / RFC 7915)
            if octets[0..8] == [0; 8] && octets[8..10] == [0xff, 0xff] && octets[10..12] == [0, 0] {
                let siit_v4 = Ipv4Addr::new(octets[12], octets[13], octets[14], octets[15]);
                if !is_safe_private_ipv4(&siit_v4) {
                    return false;
                }
            }

            // 4. Check for 6to4 addresses (2002::/16 / RFC 3056)
            if octets[0] == 0x20 && octets[1] == 0x02 {
                let embedded_v4 = Ipv4Addr::new(octets[2], octets[3], octets[4], octets[5]);
                if !is_safe_private_ipv4(&embedded_v4) {
                    return false;
                }
            }

            // 5. Check for Teredo tunneling (2001:0000::/32 / RFC 4380)
            if octets[0] == 0x20 && octets[1] == 0x01 && octets[2] == 0x00 && octets[3] == 0x00 {
                let teredo_v4 = Ipv4Addr::new(!octets[12], !octets[13], !octets[14], !octets[15]);
                if !is_safe_private_ipv4(&teredo_v4) {
                    return false;
                }
            }

            // 6. Check against IPv6 blocked prefixes for private origins
            for net in BLOCKED_PRIVATE_IPV6.iter() {
                if net.contains(v6) {
                    return false;
                }
            }

            true
        }
    }
}

fn is_safe_private_ipv4(v4: &Ipv4Addr) -> bool {
    for net in BLOCKED_PRIVATE_IPV4.iter() {
        if net.contains(v4) {
            return false;
        }
    }
    true
}

/// Abstract DNS lookup interface to allow deterministic unit testing without network.
pub trait HostLookup: Send + Sync + 'static {
    fn lookup(&self, host: &str, port: u16) -> Result<Vec<SocketAddr>, std::io::Error>;
}

/// Standard production DNS lookup utilizing system resolution.
#[derive(Clone, Default, Debug)]
pub struct SystemHostLookup;

impl HostLookup for SystemHostLookup {
    fn lookup(&self, host: &str, port: u16) -> Result<Vec<SocketAddr>, std::io::Error> {
        use std::net::ToSocketAddrs;
        let addrs: Vec<SocketAddr> = (host, port).to_socket_addrs()?.collect();
        Ok(addrs)
    }
}

/// Custom reqwest DNS resolver implementing `reqwest::dns::Resolve`.
///
/// ### Why this closes the DNS Rebinding Window:
/// In a standard HTTP proxy, validating an IP at URL inspection time does NOT protect
/// against DNS rebinding (TOCTOU attacks). An attacker can configure a malicious nameserver
/// with a 0-second TTL that responds with a benign public IP during validation, but returns
/// `169.254.169.254` (cloud metadata) or `127.0.0.1` when hyper/reqwest creates the TCP socket.
///
/// By plugging directly into `reqwest::dns::Resolve`:
/// 1. Resolution happens inside the HTTP client's connection establishment pipeline.
/// 2. For public origins, *every* returned IP address is filtered through `is_safe_public_ip()`.
/// 3. For listed private origins, addresses are filtered through `is_safe_private_ip()`,
///    which permits RFC 1918 / ULA private networks but strictly blocks cloud metadata (169.254.0.0/16)
///    and multicast/broadcast.
/// 4. If any DNS response returns only non-public/restricted addresses, resolution immediately
///    fails with an `ErrorKind::PermissionDenied` error BEFORE a TCP SYN packet can ever be sent.
/// 5. Reqwest connects exclusively to the pre-filtered safe IP addresses, while TLS SNI
///    and certificate validation still verify against the original hostname.
#[derive(Clone, Debug)]
pub struct SafeDnsResolver<L = SystemHostLookup> {
    lookup: Arc<L>,
    allowed_private_hosts: Arc<HashSet<String>>,
}

impl SafeDnsResolver<SystemHostLookup> {
    pub fn new_system() -> Self {
        Self {
            lookup: Arc::new(SystemHostLookup),
            allowed_private_hosts: Arc::new(HashSet::new()),
        }
    }

    pub fn new_system_with_private_hosts(allowed_private_hosts: HashSet<String>) -> Self {
        Self {
            lookup: Arc::new(SystemHostLookup),
            allowed_private_hosts: Arc::new(allowed_private_hosts),
        }
    }
}

impl<L: HostLookup> SafeDnsResolver<L> {
    pub fn new(lookup: L) -> Self {
        Self {
            lookup: Arc::new(lookup),
            allowed_private_hosts: Arc::new(HashSet::new()),
        }
    }

    pub fn new_with_private_hosts(lookup: L, allowed_private_hosts: HashSet<String>) -> Self {
        Self {
            lookup: Arc::new(lookup),
            allowed_private_hosts: Arc::new(allowed_private_hosts),
        }
    }
}

impl<L: HostLookup> Resolve for SafeDnsResolver<L> {
    fn resolve(&self, name: Name) -> Resolving {
        let lookup = Arc::clone(&self.lookup);
        let host = name.as_str().trim_end_matches('.').to_lowercase();
        let is_private_host = self.allowed_private_hosts.contains(&host);

        Box::pin(async move {
            let addrs = lookup
                .lookup(&host, 443)
                .map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)?;

            let safe_addrs: Vec<SocketAddr> = if is_private_host {
                addrs
                    .into_iter()
                    .filter(|addr| is_safe_private_ip(&addr.ip()))
                    .collect()
            } else {
                addrs
                    .into_iter()
                    .filter(|addr| is_safe_public_ip(&addr.ip()))
                    .collect()
            };

            if safe_addrs.is_empty() {
                let msg = if is_private_host {
                    format!("SSRF protection blocked: destination IP is restricted or cloud metadata for private origin '{host}'")
                } else {
                    format!("SSRF protection blocked: no safe public IP addresses resolved for '{host}'")
                };
                return Err(Box::new(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    msg,
                ))
                    as Box<dyn std::error::Error + Send + Sync>);
            }

            let iter: Addrs = Box::new(safe_addrs.into_iter());
            Ok(iter)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

    #[test]
    fn test_blocked_ipv4_addresses() {
        let blocked = [
            "0.0.0.0",
            "0.1.2.3",
            "10.0.0.1",
            "10.255.255.255",
            "100.64.0.1",
            "100.127.255.255",
            "127.0.0.1",
            "127.1.2.3",
            "169.254.169.254", // AWS/GCP/Azure instance metadata
            "169.254.0.1",
            "172.16.0.1",
            "172.31.255.255",
            "192.0.0.1",
            "192.0.2.1",
            "192.88.99.1",
            "192.168.0.1",
            "192.168.1.100",
            "198.18.0.1",
            "198.19.255.255",
            "198.51.100.1",
            "203.0.113.1",
            "224.0.0.1", // Multicast
            "239.255.255.255",
            "240.0.0.1", // Reserved
            "255.255.255.255",
        ];

        for ip_str in blocked {
            let ip = IpAddr::from_str(ip_str).unwrap();
            assert!(!is_safe_public_ip(&ip), "Expected {ip_str} to be BLOCKED");
        }
    }

    #[test]
    fn test_allowed_public_ipv4_addresses() {
        let allowed = [
            "1.1.1.1",
            "8.8.8.8",
            "93.184.216.34",
            "151.101.1.140",
            "140.82.121.4",
        ];

        for ip_str in allowed {
            let ip = IpAddr::from_str(ip_str).unwrap();
            assert!(is_safe_public_ip(&ip), "Expected {ip_str} to be ALLOWED");
        }
    }

    #[test]
    fn test_blocked_ipv6_addresses() {
        let blocked = [
            "::",                        // Unspecified
            "::1",                       // Loopback
            "fc00::1",                   // ULA
            "fd12:3456:789a:1::1",       // ULA
            "fe80::1",                   // Link-local
            "fe80::1234:5678:9abc:def0", // Link-local
            "ff02::1",                   // Multicast
            "64:ff9b::192.168.1.1",      // NAT64 mapping private IPv4
            "64:ff9b::8.8.8.8",          // NAT64 prefix itself is blocked
            "2001:db8::1",               // Documentation
        ];

        for ip_str in blocked {
            let ip = IpAddr::from_str(ip_str).unwrap();
            assert!(!is_safe_public_ip(&ip), "Expected {ip_str} to be BLOCKED");
        }
    }

    #[test]
    fn test_ipv4_mapped_ipv6_addresses() {
        // ::ffff:192.168.1.1 (IPv4-mapped private) -> MUST be blocked
        let mapped_private = IpAddr::from_str("::ffff:192.168.1.1").unwrap();
        assert!(!is_safe_public_ip(&mapped_private));

        // ::ffff:127.0.0.1 (IPv4-mapped loopback) -> MUST be blocked
        let mapped_loopback = IpAddr::from_str("::ffff:127.0.0.1").unwrap();
        assert!(!is_safe_public_ip(&mapped_loopback));

        // ::ffff:169.254.169.254 (IPv4-mapped metadata) -> MUST be blocked
        let mapped_metadata = IpAddr::from_str("::ffff:169.254.169.254").unwrap();
        assert!(!is_safe_public_ip(&mapped_metadata));

        // ::ffff:10.0.0.1 (IPv4-mapped RFC1918) -> MUST be blocked
        let mapped_10 = IpAddr::from_str("::ffff:10.0.0.1").unwrap();
        assert!(!is_safe_public_ip(&mapped_10));

        // ::ffff:8.8.8.8 (IPv4-mapped public) -> allowed
        let mapped_public = IpAddr::from_str("::ffff:8.8.8.8").unwrap();
        assert!(is_safe_public_ip(&mapped_public));
    }

    #[test]
    fn test_allowed_public_ipv6_addresses() {
        let allowed = [
            "2606:4700:4700::1111", // Cloudflare DNS
            "2001:4860:4860::8888", // Google DNS
            "2607:f8b0:4005:805::200e",
        ];

        for ip_str in allowed {
            let ip = IpAddr::from_str(ip_str).unwrap();
            assert!(is_safe_public_ip(&ip), "Expected {ip_str} to be ALLOWED");
        }
    }
}
