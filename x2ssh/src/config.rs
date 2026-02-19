use std::path::Path;

use serde::Deserialize;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub vpn: VpnConfig,
    #[serde(default)]
    pub connection: ConnectionConfig,
    #[serde(default)]
    pub retry: RetryConfig,
}

impl AppConfig {
    pub fn load(path: &Path) -> anyhow::Result<Self> {
        let content = std::fs::read_to_string(path)?;
        let config: AppConfig = toml::from_str(&content)?;
        Ok(config)
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct VpnConfig {
    #[serde(default = "default_subnet")]
    pub subnet: String,
    #[serde(default = "default_server_tun")]
    pub server_tun: String,
    #[serde(default = "default_client_tun")]
    pub client_tun: String,
    #[serde(default = "default_mtu")]
    pub mtu: u16,
    #[serde(default)]
    pub exclude: Vec<String>,
    #[serde(default)]
    pub post_up: Vec<String>,
    #[serde(default)]
    pub pre_down: Vec<String>,
}

impl Default for VpnConfig {
    fn default() -> Self {
        Self {
            subnet: default_subnet(),
            server_tun: default_server_tun(),
            client_tun: default_client_tun(),
            mtu: default_mtu(),
            exclude: Vec::new(),
            post_up: Vec::new(),
            pre_down: Vec::new(),
        }
    }
}

fn default_subnet() -> String {
    "10.8.0.0/24".to_string()
}

fn default_server_tun() -> String {
    "x2ssh0".to_string()
}

fn default_client_tun() -> String {
    "tun-x2ssh".to_string()
}

fn default_mtu() -> u16 {
    1400
}

#[derive(Debug, Clone, Deserialize)]
pub struct ConnectionConfig {
    #[serde(default = "default_port")]
    pub port: u16,
}

impl Default for ConnectionConfig {
    fn default() -> Self {
        Self {
            port: default_port(),
        }
    }
}

fn default_port() -> u16 {
    22
}

#[derive(Debug, Clone, Deserialize)]
pub struct RetryConfig {
    #[serde(default)]
    pub max_attempts: MaxAttempts,
    #[serde(default = "default_initial_delay_ms")]
    pub initial_delay_ms: u64,
    #[serde(default = "default_backoff")]
    pub backoff: f64,
    #[serde(default = "default_max_delay_ms")]
    pub max_delay_ms: u64,
    #[serde(default = "default_health_interval_ms")]
    pub health_interval_ms: u64,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: MaxAttempts::default(),
            initial_delay_ms: default_initial_delay_ms(),
            backoff: default_backoff(),
            max_delay_ms: default_max_delay_ms(),
            health_interval_ms: default_health_interval_ms(),
        }
    }
}

fn default_initial_delay_ms() -> u64 {
    1000
}

fn default_backoff() -> f64 {
    2.0
}

fn default_max_delay_ms() -> u64 {
    30000
}

fn default_health_interval_ms() -> u64 {
    5000
}

#[derive(Debug, Clone, Default)]
pub enum MaxAttempts {
    #[default]
    Inf,
    Count(u32),
}

impl<'de> Deserialize<'de> for MaxAttempts {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum SelfDeser {
            Str(String),
            Int(u32),
        }

        match SelfDeser::deserialize(deserializer)? {
            SelfDeser::Str(x) if x.eq_ignore_ascii_case("inf") => Ok(MaxAttempts::Inf),
            SelfDeser::Str(x) => Err(serde::de::Error::custom(format!(
                "expected \"inf\" or a number, got: {x}"
            ))),
            SelfDeser::Int(x) => Ok(MaxAttempts::Count(x)),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::path::PathBuf;

    use super::*;

    fn write_temp_config(content: &str) -> (tempfile::NamedTempFile, PathBuf) {
        let mut temp = tempfile::NamedTempFile::new().unwrap();
        temp.write_all(content.as_bytes()).unwrap();
        let path = temp.path().to_path_buf();
        (temp, path)
    }

    #[test]
    fn test_parse_full_config() {
        let toml = r#"
[vpn]
subnet = "192.168.100.0/24"
server_tun = "wg0"
client_tun = "wg-x2ssh"
mtu = 1280
exclude = ["10.0.0.0/8"]
post_up = ["ip link set $tun up"]
pre_down = ["ip link delete $tun"]

[connection]
port = 2222

[retry]
max_attempts = 5
initial_delay_ms = 500
backoff = 1.5
max_delay_ms = 10000
health_interval_ms = 3000
"#;
        let (_temp, path) = write_temp_config(toml);
        let config = AppConfig::load(&path).unwrap();

        assert_eq!(config.vpn.subnet, "192.168.100.0/24");
        assert_eq!(config.vpn.server_tun, "wg0");
        assert_eq!(config.vpn.client_tun, "wg-x2ssh");
        assert_eq!(config.vpn.mtu, 1280);
        assert_eq!(config.vpn.exclude, vec!["10.0.0.0/8"]);
        assert_eq!(config.vpn.post_up, vec!["ip link set $tun up"]);
        assert_eq!(config.vpn.pre_down, vec!["ip link delete $tun"]);
        assert_eq!(config.connection.port, 2222);
        assert!(matches!(config.retry.max_attempts, MaxAttempts::Count(5)));
        assert_eq!(config.retry.initial_delay_ms, 500);
        assert_eq!(config.retry.backoff, 1.5);
        assert_eq!(config.retry.max_delay_ms, 10000);
        assert_eq!(config.retry.health_interval_ms, 3000);
    }

    #[test]
    fn test_parse_partial_config_uses_defaults() {
        let toml = r#"
[vpn]
subnet = "10.9.0.0/24"
"#;
        let (_temp, path) = write_temp_config(toml);
        let config = AppConfig::load(&path).unwrap();

        assert_eq!(config.vpn.subnet, "10.9.0.0/24");
        assert_eq!(config.vpn.server_tun, "x2ssh0"); // default
        assert_eq!(config.connection.port, 22); // default
        assert!(matches!(config.retry.max_attempts, MaxAttempts::Inf)); // default
    }

    #[test]
    fn test_parse_empty_file_all_defaults() {
        let toml = "";
        let (_temp, path) = write_temp_config(toml);
        let config = AppConfig::load(&path).unwrap();

        assert_eq!(config.vpn.subnet, "10.8.0.0/24");
        assert_eq!(config.vpn.mtu, 1400);
        assert_eq!(config.connection.port, 22);
        assert!(matches!(config.retry.max_attempts, MaxAttempts::Inf));
    }

    #[test]
    fn test_default_config() {
        let config = AppConfig::default();
        assert_eq!(config.vpn.subnet, "10.8.0.0/24");
        assert_eq!(config.vpn.mtu, 1400);
    }

    #[test]
    fn test_max_attempts_inf() {
        let toml = r#"[retry]
max_attempts = "inf""#;
        let (_temp, path) = write_temp_config(toml);
        let config = AppConfig::load(&path).unwrap();
        assert!(matches!(config.retry.max_attempts, MaxAttempts::Inf));
    }

    #[test]
    fn test_max_attempts_count() {
        let toml = r#"[retry]
max_attempts = 5"#;
        let (_temp, path) = write_temp_config(toml);
        let config = AppConfig::load(&path).unwrap();
        assert!(matches!(config.retry.max_attempts, MaxAttempts::Count(5)));
    }

    #[test]
    fn test_max_attempts_zero_allowed() {
        let toml = r#"[retry]
max_attempts = 0"#;
        let (_temp, path) = write_temp_config(toml);
        let config = AppConfig::load(&path).unwrap();
        assert!(matches!(config.retry.max_attempts, MaxAttempts::Count(0)));
    }

    #[test]
    fn test_invalid_max_attempts_fails() {
        let toml = r#"[retry]
max_attempts = "invalid""#;
        let (_temp, path) = write_temp_config(toml);
        assert!(AppConfig::load(&path).is_err());
    }

    #[test]
    fn test_missing_file_uses_default() {
        let config = AppConfig::load(Path::new("/nonexistent/config.toml"));
        assert!(config.is_err()); // File not found should error
    }
}
