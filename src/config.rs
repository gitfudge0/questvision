use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::{fs, net::IpAddr, path::PathBuf};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub listen: IpAddr,
    pub port: u16,
    pub fps: u32,
    pub bitrate_mbps: u32,
    pub display: String,
    pub audio: bool,
    pub paired_tokens: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        let listen = local_ip_address::local_ip()
            .ok()
            .filter(|ip| ip.is_private_lan())
            .unwrap_or("127.0.0.1".parse().unwrap());
        Self {
            listen,
            port: 47990,
            fps: 60,
            bitrate_mbps: 12,
            display: "primary".into(),
            audio: false,
            paired_tokens: Vec::new(),
        }
    }
}

pub fn dir() -> Result<PathBuf> {
    let base = dirs::config_dir().context("could not determine native config directory")?;
    Ok(base.join("questdisplay"))
}

pub fn load() -> Result<Config> {
    let path = dir()?.join("config.toml");
    if !path.exists() {
        return Ok(Config::default());
    }
    let value: Config = toml::from_str(
        &fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?,
    )?;
    value.validate()?;
    Ok(value)
}

/// Serialize dashboard and pairing writes within this host process and preserve
/// fields owned by the other caller. Runtime settings remain a separate snapshot.
pub fn update(change: impl FnOnce(&mut Config) -> Result<()>) -> Result<Config> {
    static WRITES: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = WRITES
        .lock()
        .map_err(|_| anyhow::anyhow!("configuration write lock poisoned"))?;
    let mut config = load()?;
    change(&mut config)?;
    config.save()?;
    Ok(config)
}

impl Config {
    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!((1..=240).contains(&self.fps), "FPS must be 1..=240");
        anyhow::ensure!(
            (1..=100).contains(&self.bitrate_mbps),
            "bitrate must be 1..=100 Mbps"
        );
        anyhow::ensure!(self.port != 0, "port cannot be zero");
        anyhow::ensure!(
            self.listen.is_loopback() || self.listen.is_private_lan(),
            "listen address must be loopback or private LAN"
        );
        Ok(())
    }
    pub fn device_ids(&self) -> Vec<String> {
        self.paired_tokens
            .iter()
            .map(|digest| digest.chars().take(16).collect())
            .collect()
    }
    pub fn remove_device(&mut self, id: &str) -> Result<()> {
        anyhow::ensure!(
            (id.len() == 16 || id.len() == 64) && id.bytes().all(|byte| byte.is_ascii_hexdigit()),
            "invalid device ID"
        );
        let matches: Vec<_> = self
            .paired_tokens
            .iter()
            .enumerate()
            .filter(|(_, digest)| digest.starts_with(id))
            .map(|(index, _)| index)
            .collect();
        anyhow::ensure!(matches.len() == 1, "device ID not found or ambiguous");
        self.paired_tokens.remove(matches[0]);
        Ok(())
    }
    pub fn save(&self) -> Result<()> {
        self.validate()?;
        let dir = dir()?;
        fs::create_dir_all(&dir)?;
        let path = dir.join("config.toml");
        fs::write(&path, toml::to_string_pretty(self)?)?;
        restrict_permissions(&path)?;
        Ok(())
    }
}

pub trait LanAddress {
    fn is_private_lan(&self) -> bool;
}
impl LanAddress for IpAddr {
    fn is_private_lan(&self) -> bool {
        match self {
            IpAddr::V4(ip) => ip.is_private() || ip.is_link_local(),
            IpAddr::V6(ip) => ip.is_unique_local() || ip.is_unicast_link_local(),
        }
    }
}

#[cfg(unix)]
fn restrict_permissions(path: &std::path::Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}
#[cfg(not(unix))]
fn restrict_permissions(_path: &std::path::Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_public_bind() {
        let c = Config {
            listen: "8.8.8.8".parse().unwrap(),
            ..Config::default()
        };
        assert!(c.validate().is_err());
    }
    #[test]
    fn rejects_bad_fps() {
        let c = Config {
            fps: 0,
            ..Config::default()
        };
        assert!(c.validate().is_err());
    }
    #[test]
    fn lists_and_removes_only_matching_device() {
        let mut c = Config {
            paired_tokens: vec!["a".repeat(64), "b".repeat(64)],
            ..Config::default()
        };
        assert_eq!(c.device_ids(), vec!["a".repeat(16), "b".repeat(16)]);
        c.remove_device(&"a".repeat(16)).unwrap();
        assert_eq!(c.paired_tokens, vec!["b".repeat(64)]);
        assert!(c.remove_device(&"a".repeat(16)).is_err());
        assert!(c.remove_device("b").is_err());
    }
}
