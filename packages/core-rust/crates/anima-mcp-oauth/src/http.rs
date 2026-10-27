use super::OAuthError;
use ::http::{Request, Response};
use rmcp::transport::auth::{OAuthHttpClient, OAuthHttpClientFuture, OAuthHttpRequest};
use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    time::Duration,
};
use url::{Host, Url};
use zeroize::Zeroizing;

const MAX_BODY_BYTES: usize = 64 * 1024;
const MAX_REQUEST_BYTES: usize = 16 * 1024;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(8);

#[derive(Clone, Copy)]
pub(super) struct NetworkPolicy {
    allow_loopback: bool,
}
pub(super) fn literal_loopback(url: &Url) -> bool {
    matches!(url.host(),Some(Host::Ipv4(ip)) if ip.is_loopback())
        || matches!(url.host(),Some(Host::Ipv6(ip)) if ip.is_loopback())
}
fn loopback_host(url: &Url) -> bool {
    literal_loopback(url) || matches!(url.host(), Some(Host::Domain("localhost")))
}
fn public_address(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            !(ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_unspecified()
                || ip.is_multicast()
                || ip.is_broadcast()
                || ip.is_documentation()
                || ip.octets()[0] == 0
                || ip.octets()[0] >= 240
                || (ip.octets()[0] == 192 && ip.octets()[1] == 0)
                || (ip.octets()[0] == 192 && ip.octets()[1] == 88 && ip.octets()[2] == 99)
                || (ip.octets()[0] == 198 && matches!(ip.octets()[1], 18 | 19))
                || (ip.octets()[0] == 100 && (64..=127).contains(&ip.octets()[1])))
        }
        IpAddr::V6(ip) => match ip.to_ipv4_mapped() {
            Some(ip) => public_address(IpAddr::V4(ip)),
            None => {
                // Only global unicast, excluding translation/tunnel ranges and
                // special-purpose/documentation prefixes. Negative private-only
                // checks miss deprecated site-local and IPv4-compatible forms.
                (ip.segments()[0] & 0xe000) == 0x2000
                    && ip.segments()[0] != 0x2002
                    && ip.segments()[0] != 0x3fff
                    && !(ip.segments()[0] == 0x2001
                        && (ip.segments()[1] < 0x200 || ip.segments()[1] == 0xdb8))
            }
        },
    }
}
impl NetworkPolicy {
    pub(super) fn for_resource(resource: &str) -> Result<Self, OAuthError> {
        let resource = Url::parse(resource).map_err(|_| OAuthError::InvalidConfiguration)?;
        let policy = Self {
            allow_loopback: loopback_host(&resource),
        };
        policy.validate_url(&resource)?;
        Ok(policy)
    }
    pub(super) fn validate_url(self, url: &Url) -> Result<(), OAuthError> {
        if url.as_str().len() > 4096
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
            || url.host().is_none()
            || !(url.scheme() == "https"
                || (url.scheme() == "http" && self.allow_loopback && loopback_host(url)))
        {
            return Err(OAuthError::InvalidConfiguration);
        }
        match url.host() {
            Some(Host::Ipv4(ip)) => self.validate_address(IpAddr::V4(ip)),
            Some(Host::Ipv6(ip)) => self.validate_address(IpAddr::V6(ip)),
            Some(Host::Domain("localhost")) if self.allow_loopback => Ok(()),
            Some(Host::Domain(domain))
                if domain.eq_ignore_ascii_case("localhost")
                    || domain.ends_with(".localhost")
                    || !domain.contains('.') =>
            {
                Err(OAuthError::InvalidConfiguration)
            }
            Some(Host::Domain(_)) => Ok(()),
            None => Err(OAuthError::InvalidConfiguration),
        }
    }
    fn validate_address(self, ip: IpAddr) -> Result<(), OAuthError> {
        if public_address(ip) || (self.allow_loopback && ip.is_loopback()) {
            Ok(())
        } else {
            Err(OAuthError::InvalidConfiguration)
        }
    }
}
pub(super) struct RestrictedHttp {
    policy: NetworkPolicy,
    refresh_audience: Option<String>,
}
impl RestrictedHttp {
    pub(super) fn new(policy: NetworkPolicy) -> Self {
        Self {
            policy,
            refresh_audience: None,
        }
    }
    pub(super) fn with_refresh_audience(policy: NetworkPolicy, audience: String) -> Self {
        Self {
            policy,
            refresh_audience: Some(audience),
        }
    }
    async fn builder(&self, url: &Url) -> Result<reqwest::Client, OAuthError> {
        self.policy.validate_url(url)?;
        let host = url.host_str().ok_or(OAuthError::InvalidConfiguration)?;
        let port = url
            .port_or_known_default()
            .ok_or(OAuthError::InvalidConfiguration)?;
        let addresses: Vec<SocketAddr> = match url.host() {
            Some(Host::Ipv4(ip)) => vec![SocketAddr::new(IpAddr::V4(ip), port)],
            Some(Host::Ipv6(ip)) => vec![SocketAddr::new(IpAddr::V6(ip), port)],
            Some(Host::Domain("localhost")) => vec![
                SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port),
                SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), port),
            ],
            _ => tokio::time::timeout(REQUEST_TIMEOUT, tokio::net::lookup_host((host, port)))
                .await
                .map_err(|_| OAuthError::Unavailable)?
                .map_err(|_| OAuthError::Unavailable)?
                .collect(),
        };
        if addresses.is_empty() || addresses.len() > 32 {
            return Err(OAuthError::Unavailable);
        }
        for address in &addresses {
            self.policy.validate_address(address.ip())?;
        }
        // Pin validated addresses into the physical request. Proxies and
        // redirects cannot bypass the address policy or forward credentials.
        reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(REQUEST_TIMEOUT)
            .connect_timeout(Duration::from_secs(4))
            .resolve_to_addrs(host, &addresses)
            .build()
            .map_err(|_| OAuthError::Unavailable)
    }
    pub(super) async fn challenge(&self, resource: &str) -> Result<Option<String>, OAuthError> {
        let url = Url::parse(resource).map_err(|_| OAuthError::InvalidConfiguration)?;
        let response = self
            .builder(&url)
            .await?
            .get(url)
            .header("accept", "application/json")
            .send()
            .await
            .map_err(|_| OAuthError::Unavailable)?;
        if response.status().is_redirection() {
            return Err(OAuthError::InvalidConfiguration);
        }
        if response.status() != reqwest::StatusCode::UNAUTHORIZED {
            return Ok(None);
        }
        let values: Vec<_> = response
            .headers()
            .get_all("www-authenticate")
            .iter()
            .map(|value| value.to_str().map(str::to_owned))
            .collect::<Result<_, _>>()
            .map_err(|_| OAuthError::InvalidConfiguration)?;
        let challenge = values.join(", ");
        if challenge.len() > 8192 {
            return Err(OAuthError::InvalidConfiguration);
        }
        Ok((!challenge.is_empty()).then_some(challenge))
    }
    pub(super) async fn send(
        &self,
        request: Request<Vec<u8>>,
    ) -> Result<Response<Vec<u8>>, OAuthError> {
        let (parts, body) = request.into_parts();
        let body = Zeroizing::new(body);
        if body.len() > MAX_REQUEST_BYTES || parts.headers.len() > 32 {
            return Err(OAuthError::InvalidConfiguration);
        }
        let url =
            Url::parse(&parts.uri.to_string()).map_err(|_| OAuthError::InvalidConfiguration)?;
        let client = self.builder(&url).await?;
        let mut response = client
            .request(parts.method, url)
            .headers(parts.headers)
            .body(body.to_vec())
            .send()
            .await
            .map_err(|_| OAuthError::Unavailable)?;
        if response.status().is_redirection() {
            return Err(OAuthError::InvalidConfiguration);
        }
        if response
            .content_length()
            .is_some_and(|size| size > MAX_BODY_BYTES as u64)
            || response.headers().len() > 64
        {
            return Err(OAuthError::InvalidConfiguration);
        }
        let mut builder = Response::builder().status(response.status());
        for (name, value) in response.headers() {
            builder = builder.header(name, value);
        }
        let mut body = Zeroizing::new(Vec::new());
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| OAuthError::Unavailable)?
        {
            if body.len().saturating_add(chunk.len()) > MAX_BODY_BYTES {
                return Err(OAuthError::InvalidConfiguration);
            }
            body.extend_from_slice(&chunk);
        }
        builder
            .body(body.to_vec())
            .map_err(|_| OAuthError::Unavailable)
    }
}
impl OAuthHttpClient for RestrictedHttp {
    fn execute(&self, request: OAuthHttpRequest) -> OAuthHttpClientFuture<'_> {
        Box::pin(async move {
            let mut request = request.request;
            if let Some(audience) = &self.refresh_audience {
                // Reconstructing the SDK base URL adds a slash to origin-only
                // resources. Retain the exact advertised refresh audience.
                if request.method() == ::http::Method::POST
                    && request
                        .headers()
                        .get("content-type")
                        .is_some_and(|value| value == "application/x-www-form-urlencoded")
                    && url::form_urlencoded::parse(request.body())
                        .any(|(key, value)| key == "grant_type" && value == "refresh_token")
                {
                    let original = Zeroizing::new(std::mem::take(request.body_mut()));
                    let mut serializer = url::form_urlencoded::Serializer::new(String::new());
                    for (key, value) in url::form_urlencoded::parse(&original) {
                        serializer
                            .append_pair(&key, if key == "resource" { audience } else { &value });
                    }
                    let body = Zeroizing::new(serializer.finish());
                    *request.body_mut() = body.as_bytes().to_vec();
                }
            }
            self.send(request)
                .await
                .map_err(|error| Box::new(error) as _)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn remote_metadata_cannot_escape_to_local_or_private_networks() {
        let policy = NetworkPolicy::for_resource("https://service.example/mcp").unwrap();
        for value in [
            "https://127.0.0.1/token",
            "https://10.2.3.4/token",
            "https://169.254.169.254/token",
            "https://[::1]/token",
            "https://[::ffff:127.0.0.1]/token",
            "https://[fd00::1]/token",
            "https://localhost/token",
        ] {
            assert_eq!(
                policy.validate_url(&Url::parse(value).unwrap()),
                Err(OAuthError::InvalidConfiguration)
            );
        }
        let local = NetworkPolicy::for_resource("http://127.0.0.1:8888/mcp").unwrap();
        assert!(local
            .validate_url(&Url::parse("http://127.0.0.1:9999/token").unwrap())
            .is_ok());
        assert!(local
            .validate_url(&Url::parse("https://10.2.3.4/token").unwrap())
            .is_err());
    }
    #[test]
    fn metadata_rejects_reserved_and_translation_addresses() {
        let policy = NetworkPolicy::for_resource("https://service.example/mcp").unwrap();
        for value in [
            "https://198.18.0.1/token",
            "https://192.0.0.1/token",
            "https://[fec0::1]/token",
            "https://[::127.0.0.1]/token",
            "https://[64:ff9b::127.0.0.1]/token",
            "https://[2002:7f00:1::]/token",
            "https://[2001:db8::1]/token",
        ] {
            assert_eq!(
                policy.validate_url(&Url::parse(value).unwrap()),
                Err(OAuthError::InvalidConfiguration),
                "accepted {value}"
            );
        }
        for value in [
            "https://8.8.8.8/token",
            "https://[2606:4700:4700::1111]/token",
        ] {
            assert!(policy.validate_url(&Url::parse(value).unwrap()).is_ok());
        }
    }
}
