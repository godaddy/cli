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
            !(host == "localhost" || host.ends_with(".localhost") || host.ends_with(".local"))
        }
        Some(Host::Ipv4(addr)) => {
            !(addr.octets()[0] == 0
                || addr.is_loopback()
                || addr.is_private()
                || addr.is_link_local())
        }
        Some(Host::Ipv6(addr)) => {
            !(addr.is_loopback() || addr.is_unspecified() || addr.is_unicast_link_local())
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
    fn rejects_loopback_and_link_local_ipv6() {
        assert!(!is_public_routable_url("https://[::]"));
        assert!(!is_public_routable_url("https://[::1]"));
        assert!(!is_public_routable_url("https://[fe80::1]"));
    }
}
