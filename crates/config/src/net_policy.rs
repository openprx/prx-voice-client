//! The offline switch (design 4.8.7): one place that decides whether the app
//! may open an outbound connection.
//!
//! `network_policy.offline = true` blocks every outbound connection except to
//! loopback addresses, to the hosts of provider entries with
//! `local_endpoint = true` whose address is loopback or private LAN, and to
//! the hosts in `network_policy.allow_hosts`. With the switch off everything
//! is allowed and the URL is not even parsed: the caller's own client reports
//! a bad URL.
//!
//! The check is a pure function of the policy and the target. It never
//! resolves a name: a host that is not an IP literal is judged by its name
//! alone, so only `localhost` counts as loopback by name, and a local
//! endpoint reached by any other name must be listed in `allow_hosts`.
//! Ports are not part of the decision; `allow_hosts` entries match a host,
//! not a host and port.

use std::net::{IpAddr, Ipv6Addr};

use url::{Host, Url};

use crate::app_config::AppConfig;

/// Why a connection was refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Blocked {
    /// Offline mode is on and the host is neither loopback, a local endpoint
    /// nor on the allow list.
    #[error("offline mode is on and {host} is not a local or allowed host")]
    Offline {
        /// The host that was refused, as normalised for comparison.
        host: String,
    },
    /// The URL could not be parsed or names no host.
    #[error("cannot check an unusable address `{input}`: {detail}")]
    MalformedUrl {
        /// The text that was checked.
        input: String,
        /// What is wrong with it.
        detail: String,
    },
}

/// A normalised host: lower-case name without a trailing dot, or an IP
/// literal (IPv4-mapped IPv6 addresses are folded to IPv4).
#[derive(Debug, Clone, PartialEq, Eq)]
enum Target {
    Name(String),
    Ip(IpAddr),
}

impl Target {
    fn from_url_host(host: Host<&str>) -> Self {
        match host {
            Host::Domain(name) => Self::name(name),
            Host::Ipv4(ip) => Self::Ip(IpAddr::V4(ip)),
            Host::Ipv6(ip) => Self::Ip(fold_ip(ip)),
        }
    }

    fn name(name: &str) -> Self {
        Self::Name(name.trim_end_matches('.').to_ascii_lowercase())
    }

    /// Parse a host the way the `url` crate would see it in an authority
    /// (so `[::1]`, `192.168.1.20` and `Host.Example` all work).
    fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        if text.is_empty() || text.contains("://") {
            return None;
        }
        if let Ok(ip) = text.parse::<Ipv6Addr>() {
            return Some(Self::Ip(fold_ip(ip)));
        }
        let url = Url::parse(&format!("http://{text}")).ok()?;
        url.host().map(Self::from_url_host)
    }

    fn is_loopback(&self) -> bool {
        match self {
            Self::Name(name) => name == "localhost",
            Self::Ip(ip) => ip.is_loopback(),
        }
    }

    fn is_private_lan(&self) -> bool {
        match self {
            Self::Name(_) => false,
            Self::Ip(IpAddr::V4(ip)) => ip.is_private() || ip.is_link_local(),
            Self::Ip(IpAddr::V6(ip)) => {
                let first = ip.segments()[0];
                (first & 0xfe00) == 0xfc00 || (first & 0xffc0) == 0xfe80
            }
        }
    }

    fn display(&self) -> String {
        match self {
            Self::Name(name) => name.clone(),
            Self::Ip(ip) => ip.to_string(),
        }
    }
}

fn fold_ip(ip: Ipv6Addr) -> IpAddr {
    match ip.to_ipv4_mapped() {
        Some(v4) => IpAddr::V4(v4),
        None => IpAddr::V6(ip),
    }
}

/// The outbound-traffic policy. Build it with [`NetworkPolicy::from_config`]
/// (or [`NetworkPolicy::new`]) and ask [`NetworkPolicy::check`] before every
/// connection.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NetworkPolicy {
    offline: bool,
    allowed: Vec<Target>,
    local_endpoints: Vec<Target>,
}

impl NetworkPolicy {
    /// A policy with the switch off: everything is allowed.
    pub fn online() -> Self {
        Self::default()
    }

    /// Build a policy from the switch, the allow list and the base URLs of
    /// the provider entries marked `local_endpoint`.
    ///
    /// An allow-list entry that is not a valid host is ignored. A local
    /// endpoint counts only when its host is loopback or a private LAN IP
    /// literal (design 4.8.7); a local endpoint on any other name is not
    /// allowed by this route.
    pub fn new<'a>(
        offline: bool,
        allow_hosts: &[String],
        local_endpoint_urls: impl IntoIterator<Item = &'a str>,
    ) -> Self {
        let allowed = allow_hosts.iter().filter_map(|h| Target::parse(h)).collect();
        let local_endpoints = local_endpoint_urls
            .into_iter()
            .filter_map(|u| Url::parse(u).ok())
            .filter_map(|u| u.host().map(Target::from_url_host))
            .filter(|t| t.is_loopback() || t.is_private_lan())
            .collect();
        Self { offline, allowed, local_endpoints }
    }

    /// Build the policy from the loaded configuration.
    pub fn from_config(config: &AppConfig) -> Self {
        Self::new(
            config.network_policy.offline,
            &config.network_policy.allow_hosts,
            config
                .providers
                .entries
                .iter()
                .filter(|e| e.local_endpoint)
                .map(|e| e.base_url.as_str()),
        )
    }

    /// Whether offline mode is on.
    pub fn is_offline(&self) -> bool {
        self.offline
    }

    /// May the app connect to `url`? Never performs a lookup.
    pub fn check(&self, url: &str) -> Result<(), Blocked> {
        if !self.offline {
            return Ok(());
        }
        let parsed = Url::parse(url).map_err(|e| Blocked::MalformedUrl {
            input: url.to_owned(),
            detail: e.to_string(),
        })?;
        match parsed.host() {
            Some(host) => self.check_target(&Target::from_url_host(host)),
            None => Err(Blocked::MalformedUrl {
                input: url.to_owned(),
                detail: "the address has no host".to_owned(),
            }),
        }
    }

    /// May the app connect to `host` (a name, an IPv4 address or an IPv6
    /// address, with or without brackets)? The port is irrelevant to the
    /// decision and is accepted only so callers can pass what they have.
    pub fn check_host(&self, host: &str, _port: u16) -> Result<(), Blocked> {
        if !self.offline {
            return Ok(());
        }
        let target = Target::parse(host).ok_or_else(|| Blocked::MalformedUrl {
            input: host.to_owned(),
            detail: "not a valid host name or IP address".to_owned(),
        })?;
        self.check_target(&target)
    }

    fn check_target(&self, target: &Target) -> Result<(), Blocked> {
        if target.is_loopback()
            || self.local_endpoints.contains(target)
            || self.allowed.contains(target)
        {
            Ok(())
        } else {
            Err(Blocked::Offline { host: target.display() })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::ProviderEntry;

    fn offline(allow: &[&str], local: &[&str]) -> NetworkPolicy {
        let allow: Vec<String> = allow.iter().map(|s| s.to_string()).collect();
        NetworkPolicy::new(true, &allow, local.iter().copied())
    }

    fn denied(p: &NetworkPolicy, url: &str) -> bool {
        matches!(p.check(url), Err(Blocked::Offline { .. }))
    }

    #[test]
    fn offline_off_allows_everything() {
        let p = NetworkPolicy::new(false, &[], std::iter::empty());
        assert!(p.check("https://example.com/x").is_ok());
        assert!(p.check("http://10.0.0.5:9").is_ok());
        assert!(p.check("not a url").is_ok());
        assert!(p.check_host("example.com", 443).is_ok());
        assert!(NetworkPolicy::online().check("https://huggingface.co").is_ok());
    }

    #[test]
    fn offline_allows_loopback() {
        let p = offline(&[], &[]);
        for url in [
            "http://127.0.0.1:8080/v1",
            "http://127.1.2.3/",
            "http://[::1]:11434/",
            "http://localhost/",
            "http://LOCALHOST:3000/",
            "http://localhost./",
            "http://[::ffff:127.0.0.1]/",
        ] {
            assert!(p.check(url).is_ok(), "{url} should be allowed");
        }
        assert!(p.check_host("localhost", 80).is_ok());
        assert!(p.check_host("::1", 80).is_ok());
        assert!(p.check_host("[::1]", 80).is_ok());
    }

    #[test]
    fn offline_denies_everything_else() {
        let p = offline(&[], &[]);
        for url in [
            "https://example.com/",
            "http://8.8.8.8/",
            "http://192.168.1.20:8080/",
            "http://10.0.0.1/",
            "http://[2001:db8::1]/",
            "http://localhost.example.com/",
            "http://127.0.0.1.example.com/",
        ] {
            assert!(denied(&p, url), "{url} should be denied");
        }
        assert!(matches!(p.check_host("example.com", 443), Err(Blocked::Offline { .. })));
    }

    #[test]
    fn local_endpoint_on_private_lan_is_allowed_by_host() {
        let p = offline(&[], &["http://192.168.1.20:8080/v1", "http://[fd00::5]:9/"]);
        assert!(p.check("http://192.168.1.20:8080/v1/chat").is_ok());
        assert!(p.check("http://192.168.1.20:9999/").is_ok());
        assert!(p.check("http://[fd00::5]/").is_ok());
        // Another private host is not allowed just because it is private.
        assert!(denied(&p, "http://192.168.1.21/"));
        assert!(denied(&p, "http://10.0.0.1/"));
    }

    #[test]
    fn local_endpoint_that_is_public_or_named_is_not_allowed() {
        let p = offline(&[], &["https://api.openai.com/v1", "http://8.8.8.8/", "http://box.lan/"]);
        assert!(denied(&p, "https://api.openai.com/v1/chat"));
        assert!(denied(&p, "http://8.8.8.8/"));
        assert!(denied(&p, "http://box.lan/"));
    }

    #[test]
    fn allow_list_matches_host_only_and_exactly() {
        let p = offline(&["192.168.1.20", "Models.Example.com", "[2001:db8::1]", "nas.lan:8443"], &[]);
        assert!(p.check("http://192.168.1.20:1234/").is_ok());
        assert!(p.check("https://models.example.com/file").is_ok());
        assert!(p.check("http://[2001:db8::1]/").is_ok());
        assert!(p.check("http://nas.lan/").is_ok());
        assert!(denied(&p, "http://192.168.1.21/"));
        assert!(denied(&p, "https://evil.models.example.com/"));
        assert!(denied(&p, "https://example.com/"));
    }

    #[test]
    fn unusable_allow_list_entries_are_ignored() {
        let p = offline(&["", "  ", "*", "http://x.com", "a b"], &[]);
        assert!(denied(&p, "http://x.com/"));
        assert!(denied(&p, "http://a/"));
    }

    #[test]
    fn malformed_urls_are_denied_with_a_reason() {
        let p = offline(&["example.com"], &[]);
        for url in ["", "not a url", "http://", "example.com/path", "http://exa mple.com/"] {
            match p.check(url) {
                Err(Blocked::MalformedUrl { input, detail }) => {
                    assert_eq!(input, url);
                    assert!(!detail.is_empty());
                }
                other => panic!("{url:?}: expected MalformedUrl, got {other:?}"),
            }
        }
        // A URL that parses but has no host.
        assert!(matches!(
            p.check("data:text/plain,hello"),
            Err(Blocked::MalformedUrl { .. })
        ));
        assert!(matches!(p.check_host("", 1), Err(Blocked::MalformedUrl { .. })));
        let msg = p.check("not a url").unwrap_err().to_string();
        assert!(msg.contains("not a url"));
    }

    #[test]
    fn names_are_judged_by_name_only() {
        // `localhost`-looking names that are not exactly `localhost` are not
        // loopback, and a name never matches an IP entry.
        let p = offline(&["127.0.0.2"], &[]);
        assert!(denied(&p, "http://my-localhost/"));
        assert!(denied(&p, "http://localhost.localdomain/"));
        assert!(p.check("http://127.0.0.2/").is_ok());
    }

    #[test]
    fn denial_names_the_host() {
        let p = offline(&[], &[]);
        assert_eq!(
            p.check("https://Example.COM./a").unwrap_err(),
            Blocked::Offline { host: "example.com".into() }
        );
    }

    #[test]
    fn from_config_reads_switch_allow_list_and_local_entries() {
        let mut cfg = AppConfig::default();
        cfg.network_policy.offline = true;
        cfg.network_policy.allow_hosts = vec!["files.example.org".into()];
        let entry = |id: &str, url: &str, local: bool| ProviderEntry {
            id: id.into(),
            base_url: url.into(),
            local_endpoint: local,
            ..Default::default()
        };
        cfg.providers.entries = vec![
            entry("lan", "http://192.168.1.50:8080/v1", true),
            entry("cloud", "https://api.openai.com/v1", false),
            entry("sneaky", "https://api.example.net/v1", true),
        ];
        let p = NetworkPolicy::from_config(&cfg);
        assert!(p.is_offline());
        assert!(p.check("http://192.168.1.50:8080/v1/x").is_ok());
        assert!(p.check("https://files.example.org/m.bin").is_ok());
        assert!(p.check("http://127.0.0.1:1/").is_ok());
        assert!(denied(&p, "https://api.openai.com/v1"));
        assert!(denied(&p, "https://api.example.net/v1"));

        cfg.network_policy.offline = false;
        assert!(NetworkPolicy::from_config(&cfg).check("https://api.openai.com/v1").is_ok());
    }

    #[test]
    fn default_config_is_online() {
        let p = NetworkPolicy::from_config(&AppConfig::default());
        assert!(!p.is_offline());
        assert!(p.check("https://example.com").is_ok());
    }
}
