use crate::error::{AdminError, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Named connection to a MaxIO instance (remote by default).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    /// Base URL, e.g. `https://maxio.example.com` (no trailing slash).
    pub endpoint: String,
    #[serde(default)]
    pub region: Option<String>,
    #[serde(default)]
    pub access_key: Option<String>,
    #[serde(default)]
    pub secret_key: Option<String>,
    /// Admin token for `/api/admin/v1` (P2-13). Falls back to access/secret when unset.
    #[serde(default)]
    pub admin_token: Option<String>,
    #[serde(default)]
    pub timeout_ms: Option<u64>,
    /// Skip TLS certificate verification (development only).
    #[serde(default)]
    pub tls_insecure: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct ConfigFile {
    #[serde(default)]
    pub default_profile: Option<String>,
    #[serde(default)]
    pub profiles: HashMap<String, Profile>,
}

impl ConfigFile {
    pub fn load(path: Option<&Path>) -> Result<Self> {
        let path = path.map(PathBuf::from).unwrap_or_else(default_config_path);
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = fs::read_to_string(&path)
            .map_err(|e| AdminError::Config(format!("read {}: {e}", path.display())))?;
        toml::from_str(&raw)
            .map_err(|e| AdminError::Config(format!("parse {}: {e}", path.display())))
    }

    pub fn resolve_profile(&self, name: Option<&str>) -> Result<(String, Profile)> {
        let name = name
            .map(str::to_string)
            .or_else(|| self.default_profile.clone())
            .ok_or_else(|| {
                AdminError::Config(
                    "no profile selected; pass --profile or set default_profile in config".into(),
                )
            })?;
        let profile = self
            .profiles
            .get(&name)
            .cloned()
            .ok_or_else(|| AdminError::ProfileNotFound(name.clone()))?;
        Ok((name, profile))
    }
}

/// Resolves the platform user config directory without pulling in `dirs` (MPL-2.0).
pub fn user_config_dir() -> Option<PathBuf> {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME")
        && !xdg.is_empty()
    {
        return Some(PathBuf::from(xdg));
    }

    #[cfg(windows)]
    {
        std::env::var_os("APPDATA").map(PathBuf::from)
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config"))
    }
}

pub fn default_config_path() -> PathBuf {
    user_config_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("maxio")
        .join("config.toml")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard};

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    fn env_lock() -> MutexGuard<'static, ()> {
        ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner())
    }

    #[test]
    fn user_config_dir_prefers_xdg_config_home() {
        let _guard = env_lock();
        let temp = tempfile::tempdir().unwrap();
        let custom = temp.path().join("xdg-config");
        std::fs::create_dir_all(&custom).unwrap();

        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", &custom);
        }
        assert_eq!(user_config_dir().as_deref(), Some(custom.as_path()));

        unsafe {
            std::env::remove_var("XDG_CONFIG_HOME");
        }
    }

    #[test]
    fn user_config_dir_falls_back_to_home_dot_config() {
        let _guard = env_lock();
        let temp = tempfile::tempdir().unwrap();
        let home = temp.path().join("home");
        std::fs::create_dir_all(&home).unwrap();

        unsafe {
            std::env::remove_var("XDG_CONFIG_HOME");
            std::env::set_var("HOME", &home);
        }
        assert_eq!(
            user_config_dir().as_deref(),
            Some(home.join(".config").as_path())
        );

        unsafe {
            std::env::remove_var("HOME");
        }
    }

    #[test]
    fn default_config_path_joins_maxio_config_toml() {
        let _guard = env_lock();
        let temp = tempfile::tempdir().unwrap();
        let custom = temp.path().join("xdg-config");
        std::fs::create_dir_all(&custom).unwrap();

        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", &custom);
        }
        assert_eq!(
            default_config_path(),
            custom.join("maxio").join("config.toml")
        );

        unsafe {
            std::env::remove_var("XDG_CONFIG_HOME");
        }
    }

    #[test]
    fn parses_example_config() {
        let raw = r#"
default_profile = "local"

[profiles.local]
endpoint = "http://127.0.0.1:9000"
access_key = "maxioadmin"
secret_key = "maxioadmin"
timeout_ms = 3000
"#;
        let cfg: ConfigFile = toml::from_str(raw).unwrap();
        let (name, p) = cfg.resolve_profile(None).unwrap();
        assert_eq!(name, "local");
        assert_eq!(p.endpoint, "http://127.0.0.1:9000");
    }
}
