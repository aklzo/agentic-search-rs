use std::net::IpAddr;

use tokio::net::lookup_host;
use url::{Host, Url};

use crate::error::{AgentError, Result};

/// SSRF guard: validate scheme/host shape without touching the network.
/// IP-literal hosts are checked here; named hosts still need
/// [`ensure_public_host`] to catch DNS pointing at internal ranges.
pub fn validate_url(url: &Url) -> Result<()> {
    if !matches!(url.scheme(), "http" | "https") {
        return Err(blocked(url, "only http/https schemes are allowed"));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(blocked(url, "credentials in URL are not allowed"));
    }
    match url.host() {
        None => Err(blocked(url, "URL has no host")),
        Some(Host::Ipv4(ip)) => check_ip(url, IpAddr::V4(ip)),
        Some(Host::Ipv6(ip)) => check_ip(url, IpAddr::V6(ip)),
        Some(Host::Domain(domain)) => check_domain(url, domain),
    }
}

/// Resolve a named host and reject it if any address is non-public.
/// Call after [`validate_url`] and immediately before fetching.
pub async fn ensure_public_host(url: &Url) -> Result<()> {
    let domain = match url.host() {
        Some(Host::Domain(domain)) => domain.to_string(),
        // IP literals were already checked synchronously.
        _ => return Ok(()),
    };
    let port = url.port_or_known_default().unwrap_or(443);
    let addrs = lookup_host((domain.as_str(), port))
        .await
        .map_err(|e| blocked(url, &format!("DNS resolution failed: {e}")))?;
    for addr in addrs {
        check_ip(url, addr.ip())?;
    }
    Ok(())
}

fn check_domain(url: &Url, domain: &str) -> Result<()> {
    let lower = domain.to_ascii_lowercase();
    let is_internal_name = lower == "localhost"
        || lower.ends_with(".localhost")
        || lower.ends_with(".local")
        || lower.ends_with(".internal")
        || !lower.contains('.');
    if is_internal_name {
        return Err(blocked(url, "internal hostname"));
    }
    Ok(())
}

fn check_ip(url: &Url, ip: IpAddr) -> Result<()> {
    if !is_public_ip(ip) {
        return Err(blocked(url, "address is not publicly routable"));
    }
    Ok(())
}

fn is_public_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            !(v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_documentation()
                // Carrier-grade NAT 100.64.0.0/10.
                || (v4.octets()[0] == 100 && (v4.octets()[1] & 0xC0) == 64)
                // 0.0.0.0/8.
                || v4.octets()[0] == 0)
        }
        IpAddr::V6(v6) => {
            if let Some(mapped) = v6.to_ipv4_mapped() {
                return is_public_ip(IpAddr::V4(mapped));
            }
            !(v6.is_loopback()
                || v6.is_unspecified()
                // Unique-local fc00::/7.
                || (v6.segments()[0] & 0xFE00) == 0xFC00
                // Link-local fe80::/10.
                || (v6.segments()[0] & 0xFFC0) == 0xFE80)
        }
    }
}

fn blocked(url: &Url, reason: &str) -> AgentError {
    AgentError::BlockedUrl(format!("{url}: {reason}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check(url: &str) -> Result<()> {
        validate_url(&Url::parse(url).unwrap())
    }

    #[test]
    fn allows_public_https_urls() {
        assert!(check("https://example.com/page").is_ok());
        assert!(check("http://93.184.216.34/").is_ok());
    }

    #[test]
    fn rejects_non_http_schemes() {
        assert!(check("file:///etc/passwd").is_err());
        assert!(check("ftp://example.com/").is_err());
    }

    #[test]
    fn rejects_loopback_and_private_addresses() {
        assert!(check("http://127.0.0.1/").is_err());
        assert!(check("http://10.0.0.5/admin").is_err());
        assert!(check("http://192.168.1.1/").is_err());
        assert!(check("http://172.16.0.1/").is_err());
        assert!(check("http://169.254.169.254/latest/meta-data").is_err());
        assert!(check("http://100.64.0.1/").is_err());
        assert!(check("http://[::1]/").is_err());
        assert!(check("http://[fd00::1]/").is_err());
        assert!(check("http://[::ffff:127.0.0.1]/").is_err());
    }

    #[test]
    fn rejects_internal_hostnames() {
        assert!(check("http://localhost/").is_err());
        assert!(check("http://localhost:8080/").is_err());
        assert!(check("http://intranet/").is_err());
        assert!(check("http://printer.local/").is_err());
        assert!(check("http://db.internal/").is_err());
    }

    #[test]
    fn rejects_credentials_in_url() {
        assert!(check("https://user:pass@example.com/").is_err());
    }

    #[tokio::test]
    async fn dns_check_rejects_loopback_resolution() {
        let url = Url::parse("http://localtest.me/").unwrap();
        // localtest.me resolves to 127.0.0.1; offline the lookup fails, which
        // is also a rejection. Either way the fetch must be blocked.
        assert!(ensure_public_host(&url).await.is_err());
    }
}
