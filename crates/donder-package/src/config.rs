use std::fs;

use camino::Utf8PathBuf;
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::{CacheStore, PackageError, atomic_write, canonical_json};

pub const DEFAULT_REGISTRY_URL: &str = "https://donder.eddierydell.com";
const REGISTRY_CONFIG_FILE: &str = "registry.json";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DonderDirectories {
    pub config: Utf8PathBuf,
    pub cache: Utf8PathBuf,
}

impl DonderDirectories {
    pub fn discover() -> Result<Self, PackageError> {
        let directories = ProjectDirs::from("com", "Donder", "Donder").ok_or_else(|| {
            PackageError::Invalid("platform configuration directories are unavailable".to_string())
        })?;
        let config =
            Utf8PathBuf::from_path_buf(directories.config_dir().to_path_buf()).map_err(|_| {
                PackageError::Invalid("platform configuration path is not UTF-8".to_string())
            })?;
        let cache = Utf8PathBuf::from_path_buf(directories.cache_dir().to_path_buf())
            .map_err(|_| PackageError::Invalid("platform cache path is not UTF-8".to_string()))?;
        Ok(Self { config, cache })
    }

    pub fn package_cache(&self) -> CacheStore {
        CacheStore::new(self.cache.join("packages-v1"))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegistryConfig {
    pub registry: String,
}

impl Default for RegistryConfig {
    fn default() -> Self {
        Self {
            registry: DEFAULT_REGISTRY_URL.to_string(),
        }
    }
}

impl RegistryConfig {
    pub fn read() -> Result<Self, PackageError> {
        let directories = DonderDirectories::discover()?;
        Self::read_from(&directories.config)
    }

    pub fn read_from(config_directory: &camino::Utf8Path) -> Result<Self, PackageError> {
        let path = config_directory.join(REGISTRY_CONFIG_FILE);
        let config = match fs::read(path) {
            Ok(bytes) => serde_json::from_slice(&bytes)?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(error) => return Err(PackageError::Io(error)),
        };
        config.validate()?;
        Ok(config)
    }

    pub fn write(&self) -> Result<(), PackageError> {
        let directories = DonderDirectories::discover()?;
        self.write_to(&directories.config)
    }

    pub fn write_to(&self, config_directory: &camino::Utf8Path) -> Result<(), PackageError> {
        self.validate()?;
        fs::create_dir_all(config_directory)?;
        atomic_write(
            &config_directory.join(REGISTRY_CONFIG_FILE),
            &canonical_json(self)?,
        )
    }

    pub fn parsed_registry(&self) -> Result<Url, PackageError> {
        parse_registry_url(&self.registry)
    }

    fn validate(&self) -> Result<(), PackageError> {
        let _ = self.parsed_registry()?;
        Ok(())
    }
}

fn parse_registry_url(value: &str) -> Result<Url, PackageError> {
    let url = Url::parse(value)
        .map_err(|error| PackageError::Invalid(format!("invalid registry URL: {error}")))?;
    let local_http =
        url.scheme() == "http" && matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "::1"));
    if url.scheme() != "https" && !local_http {
        return Err(PackageError::Invalid(
            "registry URL must use HTTPS (HTTP is only allowed for localhost)".to_string(),
        ));
    }
    if url.cannot_be_a_base()
        || url.username() != ""
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(PackageError::Invalid(
            "registry URL must be an origin or base path without credentials, query, or fragment"
                .to_string(),
        ));
    }
    Ok(url)
}
