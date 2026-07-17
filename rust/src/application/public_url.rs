use url::{Host, Url};

/// Returns true when `value` is a syntactically valid `http(s)` URL whose host
/// is expected to be resolvable on the public internet.
///
/// The following host classes are rejected:
///  - `localhost` and any `*.localhost` / `*.local` hostnames
///  - IPv4 loopback `127.0.0.0/8`, unspecified `0.0.0.0/8`
///  - IPv6 loopback `::1` and unspecified `::`
///  - IPv4 link-local `169.254.0.0/16`
///  - IPv6 link-local `fe80::/10`
///  - RFC1918 private IPv4 ranges: `10.0.0.0/8`, `172.16.0.0/12`, `192.168.0.0/16`
///  - CGNAT / shared address space `100.64.0.0/10` (RFC 6598)
///  - IPv6 unique local addresses `fc00::/7` (RFC 4193)
///
/// Enforced client-side so users get an actionable error immediately rather than
/// an opaque 403 from the upstream WAF.
pub fn is_public_routable_url(value: &str) -> bool {
    let Ok(url) = Url::parse(value) else {
        return false;
    };

    if url.scheme() != "http" && url.scheme() != "https" {
        return false;
    }

    match url.host() {
        Some(Host::Domain(domain)) => {
            let host = domain.to_ascii_lowercase();
            !(host == "localhost"
                || host == "local"
                || host.ends_with(".localhost")
                || host.ends_with(".local"))
        }
        Some(Host::Ipv4(addr)) => {
            let octets = addr.octets();
            let is_shared = octets[0] == 100 && (64..=127).contains(&octets[1]);
            !(octets[0] == 0
                || addr.is_loopback()
                || addr.is_private()
                || addr.is_link_local()
                || is_shared)
        }
        Some(Host::Ipv6(addr)) => {
            let seg0 = addr.segments()[0];
            let is_unique_local = (seg0 & 0xfe00) == 0xfc00; // fc00::/7
            !(addr.is_loopback()
                || addr.is_unspecified()
                || addr.is_unicast_link_local()
                || is_unique_local)
        }
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_public_https_url() {
        assert!(is_public_routable_url("https://example.com"));
        assert!(is_public_routable_url("https://[2606:4700::1111]"));
    }

    #[test]
    fn rejects_non_http_scheme() {
        assert!(!is_public_routable_url("ftp://example.com/x"));
    }

    #[test]
    fn rejects_localhost_and_dot_local() {
        assert!(!is_public_routable_url("http://localhost"));
        assert!(!is_public_routable_url("http://local"));
        assert!(!is_public_routable_url("https://api.localhost"));
        assert!(!is_public_routable_url("https://api.local"));
    }

    #[test]
    fn rejects_loopback_and_private_ipv4() {
        assert!(!is_public_routable_url("http://127.0.0.1"));
        assert!(!is_public_routable_url("http://0.0.0.0"));
        assert!(!is_public_routable_url("http://10.0.0.1"));
        assert!(!is_public_routable_url("http://172.16.0.1"));
        assert!(!is_public_routable_url("http://192.168.1.1"));
        assert!(!is_public_routable_url("http://169.254.1.1"));
    }

    #[test]
    fn rejects_cgnat_shared_address_space() {
        assert!(!is_public_routable_url("http://100.64.0.1"));
        assert!(!is_public_routable_url("http://100.127.255.255"));
        assert!(is_public_routable_url("http://100.63.255.255"));
        assert!(is_public_routable_url("http://100.128.0.1"));
    }

    #[test]
    fn rejects_loopback_and_link_local_ipv6() {
        assert!(!is_public_routable_url("https://[::]"));
        assert!(!is_public_routable_url("https://[::1]"));
        assert!(!is_public_routable_url("https://[fe80::1]"));
    }

    #[test]
    fn rejects_unique_local_ipv6() {
        assert!(!is_public_routable_url("https://[fc00::1]"));
        assert!(!is_public_routable_url("https://[fd12:3456:789a::1]"));
        assert!(is_public_routable_url("https://[fe00::1]"));
    }
}
