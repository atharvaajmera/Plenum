//! DNS-sinkhole fallback addressing for the official Plenum relay.
use std::sync::{OnceLock, RwLock};
use crate::signaling::IceServer;

pub const RELAY_HOST: &str = "relay.plenumonline.me";

/// Default fallback IPs for the official relay (e.g. Mumbai EC2 elastic IP).
pub const RELAY_FALLBACK_IPS: &[&str] = &["13.232.226.146"];

/// Primary default fallback IP preserved for backwards compatibility.
pub const RELAY_FALLBACK_IP: &str = "13.232.226.146";

/// Compile-time/build env override (e.g. `PLENUM_RELAY_FALLBACK_IPS="1.2.3.4,5.6.7.8"`).
pub const BUILD_RELAY_FALLBACK_IPS: Option<&str> = option_env!("PLENUM_RELAY_FALLBACK_IPS");

/// URL where the canonical relay-config.json is published.
pub const REMOTE_CONFIG_URL: &str =
    "https://raw.githubusercontent.com/atharvaajmera/Plenum/main/relay-config.json";

static CUSTOM_FALLBACK_IPS: OnceLock<RwLock<Vec<String>>> = OnceLock::new();
static REMOTE_FALLBACK_IPS: OnceLock<RwLock<Vec<String>>> = OnceLock::new();

fn custom_fallback_storage() -> &'static RwLock<Vec<String>> {
    CUSTOM_FALLBACK_IPS.get_or_init(|| RwLock::new(Vec::new()))
}

fn remote_fallback_storage() -> &'static RwLock<Vec<String>> {
    REMOTE_FALLBACK_IPS.get_or_init(|| RwLock::new(Vec::new()))
}

/// Override fallback IPs via CLI flags or user settings.
pub fn set_custom_fallback_ips(ips: Vec<String>) {
    let cleaned: Vec<String> = ips
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if let Ok(mut lock) = custom_fallback_storage().write() {
        *lock = cleaned;
    }
}

/// Update fallback IPs fetched from remote published config.
pub fn set_remote_fallback_ips(ips: Vec<String>) {
    let cleaned: Vec<String> = ips
        .into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect();
    if let Ok(mut lock) = remote_fallback_storage().write() {
        *lock = cleaned;
    }
}

/// Returns the effective list of fallback IPs to try in order when DNS is sinkholed.
///
/// Priority:
/// 1. User settings / CLI custom overrides
/// 2. Runtime environment variable `PLENUM_RELAY_FALLBACK_IPS`
/// 3. Remotely fetched signed relay config (`relay-config.json`)
/// 4. Build-time environment variable `PLENUM_RELAY_FALLBACK_IPS`
/// 5. Embedded `RELAY_FALLBACK_IPS`
pub fn get_fallback_ips() -> Vec<String> {
    // 1. Custom settings / CLI override
    if let Ok(custom) = custom_fallback_storage().read() {
        if !custom.is_empty() {
            return custom.clone();
        }
    }

    // 2. Runtime environment variable
    if let Ok(val) = std::env::var("PLENUM_RELAY_FALLBACK_IPS") {
        let env_ips: Vec<String> = val
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if !env_ips.is_empty() {
            return env_ips;
        }
    }

    // 3. Remotely fetched config
    if let Ok(remote) = remote_fallback_storage().read() {
        if !remote.is_empty() {
            return remote.clone();
        }
    }

    // 4. Build-time environment variable
    if let Some(build_env) = BUILD_RELAY_FALLBACK_IPS {
        let build_ips: Vec<String> = build_env
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        if !build_ips.is_empty() {
            return build_ips;
        }
    }

    // 5. Default embedded fallback
    RELAY_FALLBACK_IPS
        .iter()
        .map(|s| s.to_string())
        .collect()
}

pub fn fallback_applies(url: &str) -> bool {
    url::Url::parse(url)
        .ok()
        .and_then(|parsed| parsed.host_str().map(|host| host == RELAY_HOST))
        .unwrap_or(false)
}

pub fn rewrite_host_to_fallback_ip(url: &str) -> String {
    let ips = get_fallback_ips();
    let ip = ips.first().map(|s| s.as_str()).unwrap_or(RELAY_FALLBACK_IP);
    rewrite_host_to_target_ip(url, ip)
}

pub fn rewrite_host_to_target_ip(url: &str, target_ip: &str) -> String {
    url.replace(RELAY_HOST, target_ip)
}

pub fn rewrite_ice_servers_to_fallback_ip(servers: &[IceServer]) -> Vec<IceServer> {
    let ips = get_fallback_ips();
    let ip = ips.first().map(|s| s.as_str()).unwrap_or(RELAY_FALLBACK_IP);
    rewrite_ice_servers_to_target_ip(servers, ip)
}

pub fn rewrite_ice_servers_to_target_ip(servers: &[IceServer], target_ip: &str) -> Vec<IceServer> {
    servers
        .iter()
        .map(|server| IceServer {
            urls: server
                .urls
                .iter()
                .map(|u| rewrite_host_to_target_ip(u, target_ip))
                .collect(),
            username: server.username.clone(),
            credential: server.credential.clone(),
        })
        .collect()
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RelayConfig {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub relay_host: String,
    #[serde(default)]
    pub relay_server_url: Option<String>,
    #[serde(default)]
    pub fallback_ips: Vec<String>,
    #[serde(default)]
    pub updated_at: Option<String>,
}

/// Fetches published remote relay configuration with a short timeout.
pub async fn fetch_and_apply_remote_relay_config(url: Option<&str>) -> Option<RelayConfig> {
    let target_url = url.unwrap_or(REMOTE_CONFIG_URL);
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()
        .ok()?;
    let resp = client.get(target_url).send().await.ok()?;
    if resp.status().is_success() {
        if let Ok(config) = resp.json::<RelayConfig>().await {
            if !config.fallback_ips.is_empty() {
                set_remote_fallback_ips(config.fallback_ips.clone());
            }
            return Some(config);
        }
    }
    None
}

/// Spawns a non-blocking background thread to fetch published relay config at startup.
pub fn spawn_remote_config_fetch() {
    std::thread::spawn(|| {
        if let Ok(rt) = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            rt.block_on(async {
                let _ = fetch_and_apply_remote_relay_config(None).await;
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fallback_applies_only_to_official_relay() {
        assert!(fallback_applies("wss://relay.plenumonline.me/ws"));
        assert!(fallback_applies(
            "https://relay.plenumonline.me/turn-credentials"
        ));
        assert!(!fallback_applies("wss://example.com/ws"));
        assert!(!fallback_applies("not a url"));
        assert!(!fallback_applies(""));
    }

    #[test]
    fn rewrites_stun_and_turn_urls() {
        assert_eq!(
            rewrite_host_to_fallback_ip("stun:relay.plenumonline.me:3478"),
            format!("stun:{RELAY_FALLBACK_IP}:3478")
        );
        assert_eq!(
            rewrite_host_to_fallback_ip("turn:relay.plenumonline.me:3478?transport=udp"),
            format!("turn:{RELAY_FALLBACK_IP}:3478?transport=udp")
        );
        assert_eq!(
            rewrite_host_to_fallback_ip("stun:stun.l.google.com:19302"),
            "stun:stun.l.google.com:19302"
        );
    }

    #[test]
    fn rewrites_with_custom_target_ip() {
        assert_eq!(
            rewrite_host_to_target_ip("stun:relay.plenumonline.me:3478", "1.2.3.4"),
            "stun:1.2.3.4:3478"
        );
    }

    #[test]
    fn rewrites_ice_server_list_preserving_credentials() {
        let servers = vec![
            IceServer {
                urls: vec!["stun:relay.plenumonline.me:3478".into()],
                username: None,
                credential: None,
            },
            IceServer {
                urls: vec!["turn:relay.plenumonline.me:3478?transport=udp".into()],
                username: Some("user".into()),
                credential: Some("pass".into()),
            },
        ];
        let rewritten = rewrite_ice_servers_to_fallback_ip(&servers);
        assert_eq!(
            rewritten[0].urls[0],
            format!("stun:{RELAY_FALLBACK_IP}:3478")
        );
        assert_eq!(
            rewritten[1].urls[0],
            format!("turn:{RELAY_FALLBACK_IP}:3478?transport=udp")
        );
        assert_eq!(rewritten[1].username.as_deref(), Some("user"));
        assert_eq!(rewritten[1].credential.as_deref(), Some("pass"));
    }

    #[test]
    fn custom_fallback_ips_override_defaults() {
        set_custom_fallback_ips(vec!["198.51.100.1".to_string(), "198.51.100.2".to_string()]);
        let ips = get_fallback_ips();
        assert_eq!(ips, vec!["198.51.100.1", "198.51.100.2"]);

        // Reset custom overrides for other tests
        set_custom_fallback_ips(Vec::new());
        let default_ips = get_fallback_ips();
        assert_eq!(default_ips, vec![RELAY_FALLBACK_IP]);
    }
}
