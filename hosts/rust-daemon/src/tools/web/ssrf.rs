//! Public web traffic only. The connector receives the exact DNS answers that
//! passed policy, so there is no second lookup between validation and dialing.
//! Owner-configured loopback services and scoped browser traffic use their own
//! clients and do not inherit an exception to this policy.

use std::io;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use reqwest::dns::{Addrs, Name, Resolve, Resolving};
use reqwest::{redirect, ClientBuilder, Url};

const MAX_DNS_ADDRESSES: usize = 64;

pub(super) fn client_builder() -> ClientBuilder {
    reqwest::Client::builder()
        // A proxy may resolve the destination itself, bypassing this resolver.
        // No implicit environment or OS proxy is allowed on model-selected URLs.
        .no_proxy()
        .dns_resolver(Arc::new(PublicResolver::default()))
        .redirect(redirect::Policy::custom(|attempt| {
            if attempt.previous().len() >= 10 {
                return attempt.error("too many redirects");
            }
            match reject_url(attempt.url()) {
                Ok(()) => attempt.follow(),
                Err(reason) => attempt.error(reason),
            }
        }))
}

/// Literal IPs bypass DNS inside reqwest, so check the initial URL and every
/// redirect too. Named hosts are additionally checked by `PublicResolver`.
pub(super) fn reject_url(url: &Url) -> Result<(), String> {
    if !matches!(url.scheme(), "http" | "https") {
        return Err("web_fetch only supports http/https".into());
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err("web_fetch refuses URL credentials".into());
    }
    let host = url
        .host_str()
        .ok_or("web_fetch URL has no host")?
        .trim_start_matches('[')
        .trim_end_matches(']');
    match host.parse::<IpAddr>() {
        Ok(ip) => reject_ip(ip),
        Err(_) => reject_hostname(host),
    }
}

fn reject_hostname(host: &str) -> Result<(), String> {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    if !host.contains('.')
        || host == "metadata.goog"
        || [
            "localhost",
            "local",
            "internal",
            "lan",
            "home",
            "onion",
            "arpa",
        ]
        .iter()
        .any(|suffix| host == *suffix || host.ends_with(&format!(".{suffix}")))
    {
        return Err("web_fetch refuses local or special-use hostname".into());
    }
    Ok(())
}

pub(super) fn reject_ip(ip: IpAddr) -> Result<(), String> {
    let allowed = match ip {
        IpAddr::V4(v4) => {
            let number = u32::from(v4);
            // Conservative special-purpose ranges, multicast/reserved space,
            // and Azure's host-platform virtual address. Match the browser's
            // existing public IPv4 policy rather than allowing reserved space.
            const DENIED: [([u8; 4], u32); 19] = [
                ([0, 0, 0, 0], 8),
                ([10, 0, 0, 0], 8),
                ([100, 64, 0, 0], 10),
                ([127, 0, 0, 0], 8),
                ([168, 63, 129, 16], 32),
                ([169, 254, 0, 0], 16),
                ([172, 16, 0, 0], 12),
                ([192, 0, 0, 0], 24),
                ([192, 0, 2, 0], 24),
                ([192, 31, 196, 0], 24),
                ([192, 52, 193, 0], 24),
                ([192, 88, 99, 0], 24),
                ([192, 168, 0, 0], 16),
                ([192, 175, 48, 0], 24),
                ([198, 18, 0, 0], 15),
                ([198, 51, 100, 0], 24),
                ([203, 0, 113, 0], 24),
                ([224, 0, 0, 0], 4),
                ([240, 0, 0, 0], 4),
            ];
            !DENIED.iter().any(|(prefix, bits)| {
                let mask = u32::MAX << (32 - bits);
                number & mask == u32::from_be_bytes(*prefix) & mask
            })
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return reject_ip(IpAddr::V4(v4));
            }
            let segments = v6.segments();
            // Only ordinary global unicast. This excludes local/reserved and
            // NAT64 allocations, whose routing may encode private IPv4 space.
            // Also refuse protocol assignments, 6to4 and documentation space.
            segments[0] & 0xe000 == 0x2000
                && !(segments[0] == 0x2001 && segments[1] <= 0x01ff)
                && segments[0] != 0x2002
                && !(segments[0] == 0x2001 && segments[1] == 0x0db8)
                && !(segments[0] == 0x3fff && segments[1] & 0xf000 == 0)
        }
    };
    if allowed {
        Ok(())
    } else {
        Err("web_fetch refuses private or special-use address".into())
    }
}

struct SystemResolver;

impl Resolve for SystemResolver {
    fn resolve(&self, name: Name) -> Resolving {
        Box::pin(async move {
            let addresses: Vec<_> = tokio::net::lookup_host((name.as_str(), 0))
                .await?
                .take(MAX_DNS_ADDRESSES + 1)
                .collect();
            Ok(Box::new(addresses.into_iter()) as Addrs)
        })
    }
}

struct PublicResolver {
    inner: Arc<dyn Resolve>,
}

impl Default for PublicResolver {
    fn default() -> Self {
        Self {
            inner: Arc::new(SystemResolver),
        }
    }
}

impl Resolve for PublicResolver {
    fn resolve(&self, name: Name) -> Resolving {
        if let Err(reason) = reject_hostname(name.as_str()) {
            return Box::pin(async move { Err(io::Error::other(reason).into()) });
        }
        let resolving = self.inner.resolve(name);
        Box::pin(async move {
            let addresses: Vec<SocketAddr> = resolving.await?.take(MAX_DNS_ADDRESSES + 1).collect();
            if addresses.is_empty() || addresses.len() > MAX_DNS_ADDRESSES {
                return Err(
                    io::Error::other("web_fetch DNS returned no bounded address set").into(),
                );
            }
            for address in &addresses {
                reject_ip(address.ip()).map_err(io::Error::other)?;
                if matches!(address, SocketAddr::V6(v6) if v6.scope_id() != 0) {
                    return Err(io::Error::other("web_fetch refuses scoped IPv6 address").into());
                }
            }
            // Return the checked addresses themselves, preserving the original
            // URL hostname for HTTP Host and TLS SNI/certificate verification.
            Ok(Box::new(addresses.into_iter()) as Addrs)
        })
    }
}

#[cfg(test)]
mod tests;
