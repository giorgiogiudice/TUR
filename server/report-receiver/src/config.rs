// SPDX-License-Identifier: LicenseRef-TUR-Noncommercial-1.0.0

//! The receiver's configuration: one JSON file, by default
//! `/etc/tur-report-receiver/config.json`. `deploy/config.example.json` is a
//! complete example.

use std::net::{IpAddr, SocketAddr};
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;

/// The default location of the configuration file.
pub const DEFAULT_CONFIG: &str = "/etc/tur-report-receiver/config.json";

/// The whole configuration.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Where to listen for the reverse proxy. Keep it on the loopback
    /// interface: TLS is the proxy's job.
    pub listen: SocketAddr,
    /// The address clients use, for example `https://reports.example.org`.
    /// Status addresses in receipts are built from it.
    pub public_url: String,
    /// Where reports are stored. Nothing in it is readable without the key.
    pub data_dir: PathBuf,
    /// The directory of encryption keys, `<id>.key`, outside `data_dir`.
    pub keys_dir: PathBuf,
    /// The identifier of the key new reports are encrypted with.
    pub current_key: String,
    /// How long a raw bundle is kept, in days.
    #[serde(default = "default_retention_days")]
    pub retention_days: u64,
    /// How long the state of a report stays readable, in days.
    #[serde(default = "default_status_retention_days")]
    pub status_retention_days: u64,
    /// Size, rate and concurrency limits.
    #[serde(default)]
    pub limits: Limits,
    /// The tracker summaries are filed in.
    pub github: GitHubConfig,
    /// Which peers are trusted to say who the client is.
    #[serde(default)]
    pub proxy: ProxyConfig,
    /// Intervals of the background work.
    #[serde(default)]
    pub timing: Timing,
}

fn default_retention_days() -> u64 {
    90
}

fn default_status_retention_days() -> u64 {
    365
}

/// Size, rate and concurrency limits.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Limits {
    /// The largest upload, as sent.
    pub max_upload_bytes: u64,
    /// The largest a bundle may expand to.
    pub max_unpacked_bytes: u64,
    /// Uploads one address may make in an hour.
    pub uploads_per_address_per_hour: u32,
    /// Uploads everyone together may make in an hour.
    pub uploads_per_hour: u32,
    /// Status lookups one address may make in an hour.
    pub lookups_per_address_per_hour: u32,
    /// Uploads verified at the same time. Each holds up to
    /// `max_unpacked_bytes` twice in memory.
    pub max_concurrent_uploads: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_upload_bytes: tur_diag::report::MAX_UPLOAD_BYTES,
            max_unpacked_bytes: tur_diag::report::MAX_UNPACKED_BYTES,
            uploads_per_address_per_hour: 6,
            uploads_per_hour: 120,
            lookups_per_address_per_hour: 120,
            max_concurrent_uploads: 2,
        }
    }
}

/// The GitHub repository summaries are filed in.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GitHubConfig {
    /// The REST API's base address.
    #[serde(default = "default_api_url")]
    pub api_url: String,
    /// `owner/name` of the private repository.
    pub repository: String,
    /// A file holding a fine-grained token that may write issues in that
    /// repository only. Read at every filing, never logged.
    pub token_file: PathBuf,
    /// Labels set on new issues. They must exist in the repository.
    #[serde(default = "default_labels")]
    pub labels: Vec<String>,
}

fn default_api_url() -> String {
    "https://api.github.com".to_owned()
}

fn default_labels() -> Vec<String> {
    vec!["problem-report".to_owned()]
}

/// Which peers may name the client in `X-Forwarded-For`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct ProxyConfig {
    /// Peers whose `X-Forwarded-For` is believed: the reverse proxy.
    pub trusted: Vec<IpAddr>,
}

impl Default for ProxyConfig {
    fn default() -> Self {
        ProxyConfig {
            trusted: vec![
                IpAddr::from([127, 0, 0, 1]),
                IpAddr::from([0, 0, 0, 0, 0, 0, 0, 1]),
            ],
        }
    }
}

/// Intervals of the background work, in seconds.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Timing {
    /// How often pending reports are filed.
    pub file_interval_seconds: u64,
    /// The first wait after a failed filing; it doubles with each failure.
    pub retry_initial_seconds: u64,
    /// The longest wait between filing attempts.
    pub retry_max_seconds: u64,
    /// How often expired bundles and records are deleted.
    pub sweep_interval_seconds: u64,
    /// How long an upload may take to arrive.
    pub upload_timeout_seconds: u64,
}

impl Default for Timing {
    fn default() -> Self {
        Timing {
            file_interval_seconds: 30,
            retry_initial_seconds: 60,
            retry_max_seconds: 3600,
            sweep_interval_seconds: 3600,
            upload_timeout_seconds: 120,
        }
    }
}

/// Whether `inner` is `outer` or lies below it. Both are plain absolute
/// paths (checked by [`plain_absolute`]).
fn within(inner: &Path, outer: &Path) -> bool {
    inner.starts_with(outer)
}

/// Whether `path` is absolute and has no `.` or `..` component.
fn plain_absolute(path: &Path) -> bool {
    path.is_absolute()
        && path
            .components()
            .all(|component| matches!(component, Component::RootDir | Component::Normal(_)))
}

/// Whether `id` can name a key file: 1 to 32 letters, digits, `_` or `-`.
pub fn is_key_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 32
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

impl Config {
    /// Reads and checks the configuration in `path`.
    pub fn load(path: &Path) -> Result<Config, String> {
        let text = std::fs::read_to_string(path)
            .map_err(|error| format!("{}: {error}", path.display()))?;
        Config::parse(&text).map_err(|problem| format!("{}: {problem}", path.display()))
    }

    /// Parses and checks a configuration.
    pub fn parse(text: &str) -> Result<Config, String> {
        let mut config: Config =
            serde_json::from_str(text).map_err(|error| format!("not a configuration: {error}"))?;
        config.check()?;
        Ok(config)
    }

    /// Checks the values and normalises the addresses.
    pub fn check(&mut self) -> Result<(), String> {
        self.public_url = tur_diag::report::normalise_receiver_url(&self.public_url)
            .map_err(|problem| format!("public_url {problem}"))?;
        self.github.api_url = tur_diag::report::normalise_receiver_url(&self.github.api_url)
            .map_err(|problem| format!("github.api_url {problem}"))?;
        for (name, path) in [
            ("data_dir", &self.data_dir),
            ("keys_dir", &self.keys_dir),
            ("github.token_file", &self.github.token_file),
        ] {
            if !plain_absolute(path) {
                return Err(format!(
                    "{name} must be an absolute path without . or .. components"
                ));
            }
        }
        if within(&self.keys_dir, &self.data_dir) || within(&self.data_dir, &self.keys_dir) {
            return Err("keys_dir must be outside data_dir, and data_dir outside keys_dir".to_owned());
        }
        if within(&self.github.token_file, &self.data_dir) {
            return Err("github.token_file must be outside data_dir".to_owned());
        }
        if !is_key_id(&self.current_key) {
            return Err("current_key must be 1 to 32 letters, digits, _ or -".to_owned());
        }
        let repository_ok = self
            .github
            .repository
            .split_once('/')
            .is_some_and(|(owner, name)| {
                let part = |text: &str| {
                    !text.is_empty()
                        && text.len() <= 100
                        && text
                            .bytes()
                            .all(|byte| byte.is_ascii_alphanumeric() || b"-_.".contains(&byte))
                };
                part(owner) && part(name)
            });
        if !repository_ok {
            return Err("github.repository must be owner/name".to_owned());
        }
        for label in &self.github.labels {
            if label.is_empty() || label.len() > 50 || label.chars().any(char::is_control) {
                return Err("github.labels must be 1 to 50 characters each".to_owned());
            }
        }
        if !(1..=3650).contains(&self.retention_days) {
            return Err("retention_days must be 1 to 3650".to_owned());
        }
        if self.status_retention_days < self.retention_days {
            return Err("status_retention_days must not be shorter than retention_days".to_owned());
        }
        let limits = &self.limits;
        if limits.max_upload_bytes == 0
            || limits.max_upload_bytes > 64 * 1024 * 1024
            || limits.max_unpacked_bytes == 0
            || limits.max_unpacked_bytes > 512 * 1024 * 1024
        {
            return Err("limits: sizes must be above 0, at most 64 MiB sent and 512 MiB unpacked".to_owned());
        }
        if limits.uploads_per_address_per_hour == 0
            || limits.uploads_per_hour == 0
            || limits.lookups_per_address_per_hour == 0
            || limits.max_concurrent_uploads == 0
        {
            return Err("limits: rates and concurrency must be above 0".to_owned());
        }
        let timing = &self.timing;
        if timing.file_interval_seconds == 0
            || timing.retry_initial_seconds == 0
            || timing.retry_max_seconds < timing.retry_initial_seconds
            || timing.sweep_interval_seconds == 0
            || timing.upload_timeout_seconds == 0
        {
            return Err("timing: intervals must be above 0, retry_max at least retry_initial".to_owned());
        }
        Ok(())
    }

    /// The retention period in seconds.
    pub fn retention_seconds(&self) -> u64 {
        self.retention_days.saturating_mul(86_400)
    }

    /// How long a report's state stays readable, in seconds.
    pub fn status_retention_seconds(&self) -> u64 {
        self.status_retention_days.saturating_mul(86_400)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE: &str = include_str!("../deploy/config.example.json");

    #[test]
    fn the_example_configuration_is_valid() {
        let config = Config::parse(EXAMPLE).expect("the example parses");
        assert_eq!(config.retention_days, 90);
        assert_eq!(config.retention_seconds(), 90 * 86_400);
        assert!(config.listen.ip().is_loopback());
        assert!(config.public_url.starts_with("https://"));
        assert_eq!(config.github.api_url, "https://api.github.com");
    }

    fn with(change: impl Fn(&mut serde_json::Value)) -> Result<Config, String> {
        let mut value: serde_json::Value = serde_json::from_str(EXAMPLE).expect("JSON");
        change(&mut value);
        Config::parse(&value.to_string())
    }

    #[test]
    fn unsafe_configurations_are_refused() {
        // The key must not live in the data directory.
        assert!(with(|v| v["keys_dir"] = "/var/lib/tur-report-receiver/keys".into()).is_err());
        assert!(with(|v| v["data_dir"] = "/etc/tur-report-receiver/keys/data".into()).is_err());
        assert!(with(|v| v["github"]["token_file"] = "/var/lib/tur-report-receiver/t".into()).is_err());
        assert!(with(|v| v["data_dir"] = "relative/dir".into()).is_err());
        assert!(with(|v| v["data_dir"] = "/var/lib/../etc".into()).is_err());
        // Only https, except on this machine.
        assert!(with(|v| v["public_url"] = "http://reports.example.org".into()).is_err());
        assert!(with(|v| v["github"]["api_url"] = "http://api.github.com".into()).is_err());
        assert!(with(|v| v["github"]["api_url"] = "http://127.0.0.1:9100".into()).is_ok());
        assert!(with(|v| v["github"]["repository"] = "no-slash".into()).is_err());
        assert!(with(|v| v["current_key"] = "../key".into()).is_err());
        assert!(with(|v| v["retention_days"] = 0.into()).is_err());
        assert!(with(|v| v["status_retention_days"] = 30.into()).is_err());
        assert!(with(|v| v["limits"] = serde_json::json!({"max_upload_bytes": 0})).is_err());
        assert!(with(|v| v["unknown"] = 1.into()).is_err());
    }
}
