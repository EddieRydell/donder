use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::io::Read;

use camino::Utf8Path;
use reqwest::blocking::{Client, Response};
use reqwest::{StatusCode, Url};

use crate::{
    CacheStatus, CacheStore, Dependency, DeviceIdentityResponse, DeviceLoginRequest,
    DeviceStartResponse, DeviceTokenResponse, Lockfile, PackageError, PackageId, PackageManifest,
    PublishFinalizeRequest, PublishFinalizeResponse, PublishManagementAction, PublishStageRequest,
    PublishStageResponse, REGISTRY_PROTOCOL_VERSION, RegistryDiscovery, RegistryDownloadResponse,
    RegistryRelease, RegistryResolveResponse, ResolutionPin, resolve_registry_with_pins,
    sha256_hex,
};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RegistryResolution {
    pub lock: Lockfile,
    pub deprecated_packages: BTreeSet<PackageId>,
}

struct RegistryCatalog {
    releases: BTreeMap<PackageId, Vec<RegistryRelease>>,
    deprecated_packages: BTreeSet<PackageId>,
}

#[derive(Clone, Debug)]
pub struct RegistryClient {
    website_url: Url,
    protocol_url: Url,
    resolve_url: Url,
    download_url: Url,
    device_login_url: Url,
    publish_stage_url: Url,
    publish_finalize_url: Url,
    http: Client,
}

impl RegistryClient {
    pub fn discover(registry: &crate::RegistryConfig) -> Result<Self, PackageError> {
        let mut website_url = registry.parsed_registry()?;
        if !website_url.path().ends_with('/') {
            let mut path = website_url.path().to_string();
            path.push('/');
            website_url.set_path(&path);
        }
        let discovery_url = website_url
            .join(".well-known/donder-registry.json")
            .map_err(registry_error)?;
        let http = Client::builder()
            .user_agent(concat!("donder/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(registry_error)?;
        let discovery = response_json::<RegistryDiscovery>(
            http.get(discovery_url).send().map_err(registry_error)?,
            "registry discovery",
        )?;
        if discovery.registry_version != REGISTRY_PROTOCOL_VERSION {
            return Err(PackageError::Invalid(format!(
                "registry protocol version {} is unsupported",
                discovery.registry_version
            )));
        }
        let protocol_url = Url::parse(&discovery.protocol_url).map_err(registry_error)?;
        if protocol_url.query().is_some() || protocol_url.fragment().is_some() {
            return Err(PackageError::Invalid(
                "registry protocolUrl cannot contain a query or fragment".to_string(),
            ));
        }
        let discovered_website = Url::parse(&discovery.website_url).map_err(registry_error)?;
        if normalized_origin(&discovered_website) != normalized_origin(&website_url) {
            return Err(PackageError::Invalid(
                "registry discovery websiteUrl does not match the configured registry".to_string(),
            ));
        }
        validate_remote_url(&protocol_url, &website_url)?;
        let resolve_url = discovery_endpoint(
            &discovery.endpoints.resolve,
            "resolve",
            &protocol_url,
            &website_url,
        )?;
        let download_url = discovery_endpoint(
            &discovery.endpoints.download,
            "download",
            &protocol_url,
            &website_url,
        )?;
        let device_login_url = discovery_endpoint(
            &discovery.endpoints.device_login,
            "device login",
            &protocol_url,
            &website_url,
        )?;
        let publish_stage_url = discovery_endpoint(
            &discovery.endpoints.publish_stage,
            "publish staging",
            &protocol_url,
            &website_url,
        )?;
        let publish_finalize_url = discovery_endpoint(
            &discovery.endpoints.publish_finalize,
            "publish finalization",
            &protocol_url,
            &website_url,
        )?;
        validate_operation_query(&resolve_url, "resolve")?;
        validate_operation_query(&download_url, "download")?;
        validate_no_query(&device_login_url, "device login")?;
        validate_no_query(&publish_stage_url, "publish staging")?;
        validate_no_query(&publish_finalize_url, "publish finalization")?;
        Ok(Self {
            website_url,
            protocol_url,
            resolve_url,
            download_url,
            device_login_url,
            publish_stage_url,
            publish_finalize_url,
            http,
        })
    }

    pub fn website_url(&self) -> &Url {
        &self.website_url
    }

    pub fn protocol_url(&self) -> &Url {
        &self.protocol_url
    }

    pub fn start_device_login(
        &self,
        client_name: impl Into<String>,
    ) -> Result<DeviceStartResponse, PackageError> {
        self.post_device(
            &DeviceLoginRequest::Start {
                registry_version: REGISTRY_PROTOCOL_VERSION,
                client_name: client_name.into(),
            },
            None,
        )
    }

    pub fn poll_device_login(
        &self,
        device_code: &str,
    ) -> Result<Option<DeviceTokenResponse>, PackageError> {
        let response = self
            .http
            .post(self.device_login_url.clone())
            .json(&DeviceLoginRequest::Poll {
                registry_version: REGISTRY_PROTOCOL_VERSION,
                device_code: device_code.to_string(),
            })
            .send()
            .map_err(registry_error)?;
        if response.status() == StatusCode::PRECONDITION_REQUIRED {
            return Ok(None);
        }
        let token = response_json(response, "device token response")?;
        validate_token_response(&token)?;
        Ok(Some(token))
    }

    pub fn refresh_credential(
        &self,
        refresh_credential: &str,
    ) -> Result<DeviceTokenResponse, PackageError> {
        let token = self.post_device(
            &DeviceLoginRequest::Refresh {
                registry_version: REGISTRY_PROTOCOL_VERSION,
                refresh_credential: refresh_credential.to_string(),
            },
            None,
        )?;
        validate_token_response(&token)?;
        Ok(token)
    }

    pub fn revoke_credential(&self, refresh_credential: &str) -> Result<(), PackageError> {
        let response = self
            .http
            .post(self.device_login_url.clone())
            .json(&DeviceLoginRequest::Revoke {
                registry_version: REGISTRY_PROTOCOL_VERSION,
                refresh_credential: refresh_credential.to_string(),
            })
            .send()
            .map_err(registry_error)?;
        let _ = response.error_for_status().map_err(registry_error)?;
        Ok(())
    }

    pub fn identity(&self, access_token: &str) -> Result<DeviceIdentityResponse, PackageError> {
        let identity: DeviceIdentityResponse = self.post_device(
            &DeviceLoginRequest::WhoAmI {
                registry_version: REGISTRY_PROTOCOL_VERSION,
            },
            Some(access_token),
        )?;
        if identity.registry_version != REGISTRY_PROTOCOL_VERSION {
            return Err(PackageError::Invalid(
                "registry returned an unsupported identity response".to_string(),
            ));
        }
        Ok(identity)
    }

    pub fn publish(
        &self,
        access_token: &str,
        filename: &str,
        archive: &[u8],
    ) -> Result<PublishFinalizeResponse, PackageError> {
        let stage = response_json::<PublishStageResponse>(
            self.http
                .post(self.publish_stage_url.clone())
                .bearer_auth(access_token)
                .json(&PublishStageRequest {
                    registry_version: REGISTRY_PROTOCOL_VERSION,
                    action: PublishManagementAction::Stage,
                    original_filename: filename.to_string(),
                    size_bytes: archive.len() as u64,
                })
                .send()
                .map_err(registry_error)?,
            "publish staging response",
        )?;
        if stage.registry_version != REGISTRY_PROTOCOL_VERSION {
            return Err(PackageError::Invalid(
                "registry returned an unsupported publish staging response".to_string(),
            ));
        }
        let upload_url = Url::parse(&stage.upload_url).map_err(registry_error)?;
        validate_remote_url(&upload_url, &self.website_url)?;
        self.http
            .put(upload_url)
            .header(reqwest::header::CONTENT_TYPE, "application/zip")
            .header("x-upsert", "false")
            .body(archive.to_vec())
            .send()
            .map_err(registry_error)?
            .error_for_status()
            .map_err(registry_error)?;
        let finalized = response_json::<PublishFinalizeResponse>(
            self.http
                .post(self.publish_finalize_url.clone())
                .bearer_auth(access_token)
                .json(&PublishFinalizeRequest {
                    registry_version: REGISTRY_PROTOCOL_VERSION,
                    upload_id: stage.upload_id,
                })
                .send()
                .map_err(registry_error)?,
            "publish finalization response",
        )?;
        if finalized.registry_version != REGISTRY_PROTOCOL_VERSION {
            return Err(PackageError::Invalid(
                "registry returned an unsupported publish finalization response".to_string(),
            ));
        }
        Ok(finalized)
    }

    pub fn resolve_lock(
        &self,
        root: &Utf8Path,
        manifest: &PackageManifest,
    ) -> Result<Lockfile, PackageError> {
        self.resolve_lock_with_pins(root, manifest, &BTreeMap::new())
    }

    pub fn resolve_lock_with_pins(
        &self,
        root: &Utf8Path,
        manifest: &PackageManifest,
        pins: &BTreeMap<PackageId, ResolutionPin>,
    ) -> Result<Lockfile, PackageError> {
        self.resolve_with_pins(root, manifest, pins)
            .map(|resolution| resolution.lock)
    }

    pub fn resolve_with_pins(
        &self,
        root: &Utf8Path,
        manifest: &PackageManifest,
        pins: &BTreeMap<PackageId, ResolutionPin>,
    ) -> Result<RegistryResolution, PackageError> {
        let seeds = crate::collect_path_modules(root, manifest)?
            .values()
            .flat_map(|module| module.manifest.dependencies.values())
            .chain(manifest.dependencies.values())
            .filter_map(|dependency| match dependency {
                Dependency::Registry { package, .. } => Some(package.clone()),
                Dependency::Path { .. } => None,
            })
            .collect::<BTreeSet<_>>();
        let catalog = self.fetch_catalog(seeds, pins)?;
        let lock = resolve_registry_with_pins(
            root,
            manifest,
            self.website_url.as_str().trim_end_matches('/'),
            &catalog.releases,
            pins,
        )?;
        let deprecated_packages = catalog
            .deprecated_packages
            .into_iter()
            .filter(|package| lock.packages.contains_key(package))
            .collect();
        Ok(RegistryResolution {
            lock,
            deprecated_packages,
        })
    }

    pub fn ensure_locked_artifacts(
        &self,
        lockfile: &Lockfile,
        cache: &CacheStore,
        mut validate: impl FnMut(
            &Utf8Path,
            &PackageId,
            &crate::LockedPackage,
            &Lockfile,
            &CacheStore,
        ) -> Result<(), PackageError>,
    ) -> Result<(), PackageError> {
        for package in locked_package_install_order(lockfile)? {
            let locked = lockfile.packages.get(&package).ok_or_else(|| {
                PackageError::Invalid(format!(
                    "donder.lock installation order contains missing package `{package}`"
                ))
            })?;
            match cache.status(&locked.archive_sha256)? {
                CacheStatus::Ready => {
                    let package_root = cache.package_root(&locked.archive_sha256)?;
                    validate(&package_root, &package, locked, lockfile, cache)?;
                    continue;
                }
                CacheStatus::Missing => {}
            }
            let mut endpoint = self.download_url.clone();
            endpoint
                .query_pairs_mut()
                .append_pair("package", package.as_str())
                .append_pair("version", &locked.version.to_string())
                .append_pair("sha256", &locked.archive_sha256);
            let download = response_json::<RegistryDownloadResponse>(
                self.http.get(endpoint).send().map_err(registry_error)?,
                "release download metadata",
            )?;
            if download.registry_version != REGISTRY_PROTOCOL_VERSION
                || download.package != package
                || download.version != locked.version
                || download.module_id != locked.module_id
                || download.archive_sha256 != locked.archive_sha256
            {
                return Err(PackageError::Invalid(format!(
                    "registry download metadata does not match locked `{package}@{}`",
                    locked.version
                )));
            }
            if download.size_bytes as usize > crate::MAX_ARCHIVE_BYTES {
                return Err(PackageError::Archive(format!(
                    "locked `{package}@{}` exceeds the archive limit",
                    locked.version
                )));
            }
            let url = Url::parse(&download.url).map_err(registry_error)?;
            validate_remote_url(&url, &self.website_url)?;
            let response = self.http.get(url).send().map_err(registry_error)?;
            let bytes = bounded_bytes(response, download.size_bytes, crate::MAX_ARCHIVE_BYTES)?;
            if sha256_hex(&bytes) != locked.archive_sha256 {
                return Err(PackageError::Archive(format!(
                    "downloaded `{package}@{}` failed SHA-256 verification",
                    locked.version
                )));
            }
            let _ = cache.install(&locked.archive_sha256, &bytes, |package_root| {
                validate(package_root, &package, locked, lockfile, cache)
            })?;
        }
        Ok(())
    }

    pub fn release_metadata(
        &self,
        package: &PackageId,
    ) -> Result<RegistryResolveResponse, PackageError> {
        self.release_metadata_with_pin(package, None)
    }

    pub fn deprecated_locked_packages(
        &self,
        lockfile: &Lockfile,
    ) -> Result<BTreeSet<PackageId>, PackageError> {
        let mut deprecated = BTreeSet::new();
        for (package, locked) in &lockfile.packages {
            let pin = ResolutionPin {
                version: locked.version.clone(),
                archive_sha256: locked.archive_sha256.clone(),
            };
            if self
                .release_metadata_with_pin(package, Some(&pin))?
                .deprecated
            {
                deprecated.insert(package.clone());
            }
        }
        Ok(deprecated)
    }

    fn release_metadata_with_pin(
        &self,
        package: &PackageId,
        pin: Option<&ResolutionPin>,
    ) -> Result<RegistryResolveResponse, PackageError> {
        let mut endpoint = self.resolve_url.clone();
        endpoint
            .query_pairs_mut()
            .append_pair("package", package.as_str());
        if let Some(pin) = pin {
            endpoint
                .query_pairs_mut()
                .append_pair("lockedVersion", &pin.version.to_string())
                .append_pair("lockedSha256", &pin.archive_sha256);
        }
        let response = response_json::<RegistryResolveResponse>(
            self.http.get(endpoint).send().map_err(registry_error)?,
            "package metadata",
        )?;
        if response.registry_version != REGISTRY_PROTOCOL_VERSION || response.package != *package {
            return Err(PackageError::Invalid(format!(
                "registry returned mismatched metadata for `{package}`"
            )));
        }
        Ok(response)
    }

    fn fetch_catalog(
        &self,
        seeds: BTreeSet<PackageId>,
        pins: &BTreeMap<PackageId, ResolutionPin>,
    ) -> Result<RegistryCatalog, PackageError> {
        let mut pending = VecDeque::from_iter(seeds);
        let mut available = BTreeMap::new();
        let mut deprecated_packages = BTreeSet::new();
        while let Some(package) = pending.pop_front() {
            if available.contains_key(&package) {
                continue;
            }
            let metadata = self.release_metadata_with_pin(&package, pins.get(&package))?;
            if metadata.deprecated {
                deprecated_packages.insert(package.clone());
            }
            let releases = metadata
                .versions
                .into_iter()
                .map(|release| {
                    release.validate_contract()?;
                    let runtime_compatible = release.is_runtime_compatible();
                    let yanked = release.status == crate::RegistryReleaseStatus::Yanked;
                    for dependency in release.dependencies.values() {
                        match dependency {
                            Dependency::Registry { package, .. } => {
                                if !available.contains_key(package) {
                                    pending.push_back(package.clone());
                                }
                            }
                            Dependency::Path { .. } => {
                                return Err(PackageError::Invalid(format!(
                                    "registry release `{package}@{}` contains a path dependency",
                                    release.version
                                )));
                            }
                        }
                    }
                    Ok(RegistryRelease {
                        package: package.clone(),
                        version: release.version,
                        archive_sha256: release.archive_sha256,
                        module_id: release.module_id,
                        dependencies: release.dependencies,
                        yanked,
                        runtime_compatible,
                    })
                })
                .collect::<Result<Vec<_>, _>>()?;
            available.insert(package, releases);
        }
        Ok(RegistryCatalog {
            releases: available,
            deprecated_packages,
        })
    }

    fn post_device<T: serde::de::DeserializeOwned>(
        &self,
        request: &DeviceLoginRequest,
        bearer: Option<&str>,
    ) -> Result<T, PackageError> {
        let mut builder = self.http.post(self.device_login_url.clone()).json(request);
        if let Some(bearer) = bearer {
            builder = builder.bearer_auth(bearer);
        }
        response_json(
            builder.send().map_err(registry_error)?,
            "device authorization response",
        )
    }
}

fn locked_package_install_order(lockfile: &Lockfile) -> Result<Vec<PackageId>, PackageError> {
    fn visit(
        package: &PackageId,
        lockfile: &Lockfile,
        visiting: &mut Vec<PackageId>,
        visited: &mut BTreeSet<PackageId>,
        order: &mut Vec<PackageId>,
    ) -> Result<(), PackageError> {
        if visited.contains(package) {
            return Ok(());
        }
        if let Some(index) = visiting.iter().position(|entry| entry == package) {
            let mut cycle = visiting[index..]
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>();
            cycle.push(package.to_string());
            return Err(PackageError::Invalid(format!(
                "package dependency cycle while installing artifacts: {}",
                cycle.join(" -> ")
            )));
        }
        let locked = lockfile.packages.get(package).ok_or_else(|| {
            PackageError::Invalid(format!(
                "locked package graph points to missing package `{package}`"
            ))
        })?;
        visiting.push(package.clone());
        for dependency in locked.dependencies.values() {
            visit(dependency, lockfile, visiting, visited, order)?;
        }
        let _ = visiting.pop();
        visited.insert(package.clone());
        order.push(package.clone());
        Ok(())
    }

    let mut visiting = Vec::new();
    let mut visited = BTreeSet::new();
    let mut order = Vec::new();
    for package in lockfile.packages.keys() {
        visit(package, lockfile, &mut visiting, &mut visited, &mut order)?;
    }
    Ok(order)
}

fn validate_token_response(token: &DeviceTokenResponse) -> Result<(), PackageError> {
    if token.registry_version != REGISTRY_PROTOCOL_VERSION
        || token.access_token.is_empty()
        || token.refresh_credential.is_empty()
        || token.expires_in == 0
        || token.refresh_expires_in == 0
    {
        return Err(PackageError::Invalid(
            "registry returned an invalid credential response".to_string(),
        ));
    }
    Ok(())
}

fn response_json<T: serde::de::DeserializeOwned>(
    response: Response,
    operation: &str,
) -> Result<T, PackageError> {
    let response = response.error_for_status().map_err(registry_error)?;
    response.json::<T>().map_err(|error| {
        PackageError::Invalid(format!("registry returned invalid {operation}: {error}"))
    })
}

fn bounded_bytes(
    mut response: Response,
    expected_size: u64,
    maximum_size: usize,
) -> Result<Vec<u8>, PackageError> {
    let status = response.status();
    if status != StatusCode::OK {
        return Err(PackageError::Invalid(format!(
            "release download failed with HTTP {status}"
        )));
    }
    if let Some(content_length) = response.content_length()
        && content_length != expected_size
    {
        return Err(PackageError::Archive(
            "release download Content-Length does not match registry metadata".to_string(),
        ));
    }
    let mut bytes = Vec::new();
    response
        .by_ref()
        .take(maximum_size as u64 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > maximum_size {
        return Err(PackageError::Archive(format!(
            "archive exceeds {maximum_size} bytes"
        )));
    }
    if bytes.len() as u64 != expected_size {
        return Err(PackageError::Archive(
            "release download size does not match registry metadata".to_string(),
        ));
    }
    Ok(bytes)
}

fn validate_remote_url(url: &Url, configured: &Url) -> Result<(), PackageError> {
    let local = configured.scheme() == "http"
        && matches!(
            configured.host_str(),
            Some("localhost" | "127.0.0.1" | "::1")
        );
    if url.scheme() != "https" && !(local && url.scheme() == "http") {
        return Err(PackageError::Invalid(
            "registry endpoint must use HTTPS".to_string(),
        ));
    }
    if url.username() != "" || url.password().is_some() {
        return Err(PackageError::Invalid(
            "registry endpoint cannot contain URL credentials".to_string(),
        ));
    }
    Ok(())
}

fn discovery_endpoint(
    value: &str,
    label: &str,
    protocol_url: &Url,
    website_url: &Url,
) -> Result<Url, PackageError> {
    let endpoint = Url::parse(value).map_err(registry_error)?;
    validate_remote_url(&endpoint, website_url)?;
    if endpoint.fragment().is_some()
        || normalized_origin(&endpoint) != normalized_origin(protocol_url)
    {
        return Err(PackageError::Invalid(format!(
            "registry {label} endpoint is outside protocolUrl"
        )));
    }
    let mut protocol_path = protocol_url.path().trim_end_matches('/').to_string();
    protocol_path.push('/');
    if !endpoint.path().starts_with(&protocol_path) {
        return Err(PackageError::Invalid(format!(
            "registry {label} endpoint is outside protocolUrl"
        )));
    }
    Ok(endpoint)
}

fn validate_operation_query(endpoint: &Url, expected: &str) -> Result<(), PackageError> {
    let query = endpoint.query_pairs().collect::<Vec<_>>();
    if query.len() != 1 || query[0].0 != "op" || query[0].1 != expected {
        return Err(PackageError::Invalid(format!(
            "registry {expected} endpoint must contain only `op={expected}`"
        )));
    }
    Ok(())
}

fn validate_no_query(endpoint: &Url, label: &str) -> Result<(), PackageError> {
    if endpoint.query().is_some() {
        return Err(PackageError::Invalid(format!(
            "registry {label} endpoint cannot contain a query"
        )));
    }
    Ok(())
}

fn normalized_origin(url: &Url) -> (String, Option<String>, Option<u16>) {
    (
        url.scheme().to_string(),
        url.host_str().map(ToString::to_string),
        url.port_or_known_default(),
    )
}

fn registry_error(error: impl std::fmt::Display) -> PackageError {
    PackageError::Invalid(format!("registry request failed: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LockedPackage;
    use semver::Version;
    use uuid::Uuid;

    fn locked(dependencies: BTreeMap<String, PackageId>) -> LockedPackage {
        LockedPackage {
            version: Version::parse("1.0.0").expect("version"),
            archive_sha256: "a".repeat(64),
            module_id: Uuid::new_v4(),
            dependencies,
        }
    }

    #[test]
    fn artifact_install_order_places_dependencies_first() {
        let foundation = PackageId::new("test/foundation").expect("package");
        let library = PackageId::new("test/library").expect("package");
        let lock = Lockfile {
            lock_version: crate::LOCK_VERSION,
            manifest_sha256: "b".repeat(64),
            registry: "https://registry.donder.dev".to_string(),
            packages: BTreeMap::from([
                (foundation.clone(), locked(BTreeMap::new())),
                (
                    library.clone(),
                    locked(BTreeMap::from([(
                        "foundation".to_string(),
                        foundation.clone(),
                    )])),
                ),
            ]),
            path_dependencies: BTreeMap::new(),
        };

        assert_eq!(
            locked_package_install_order(&lock).expect("install order"),
            vec![foundation, library]
        );
    }

    #[test]
    fn artifact_install_order_rejects_package_cycles() {
        let left = PackageId::new("test/left").expect("package");
        let right = PackageId::new("test/right").expect("package");
        let lock = Lockfile {
            lock_version: crate::LOCK_VERSION,
            manifest_sha256: "b".repeat(64),
            registry: "https://registry.donder.dev".to_string(),
            packages: BTreeMap::from([
                (
                    left.clone(),
                    locked(BTreeMap::from([("right".to_string(), right.clone())])),
                ),
                (right, locked(BTreeMap::from([("left".to_string(), left)]))),
            ]),
            path_dependencies: BTreeMap::new(),
        };

        assert!(
            locked_package_install_order(&lock)
                .expect_err("cycle")
                .to_string()
                .contains("package dependency cycle")
        );
    }
}
