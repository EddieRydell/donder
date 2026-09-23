use std::collections::{BTreeMap, BTreeSet};

use semver::Version;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const REGISTRY_PROTOCOL_VERSION: u8 = 1;

use super::{
    Dependency, LANGUAGE_VERSION, MAX_ARCHIVE_BYTES, MAX_EXPANDED_BYTES, MAX_FILES, PackageError,
    PackageId, ReleaseExportGroup, require_donder_document, valid_alias, valid_language_version,
    valid_object_name, validate_relative_path, validate_sha256,
};

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct RegistryDiscovery {
    pub registry_version: u8,
    pub protocol_url: String,
    pub website_url: String,
    pub endpoints: RegistryEndpoints,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct RegistryEndpoints {
    pub resolve: String,
    pub download: String,
    pub device_login: String,
    pub publish_stage: String,
    pub publish_finalize: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct RegistryResolveResponse {
    pub registry_version: u8,
    pub package: PackageId,
    pub deprecated: bool,
    pub versions: Vec<RegistryReleaseMetadata>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct RegistryReleaseMetadata {
    pub version: Version,
    pub module_id: Uuid,
    pub language_version: String,
    pub archive_sha256: String,
    pub size_bytes: u64,
    pub expanded_size: u64,
    pub file_count: usize,
    pub has_audio: bool,
    pub status: RegistryReleaseStatus,
    pub dependencies: BTreeMap<String, Dependency>,
    pub exports: BTreeMap<String, ReleaseExportGroup>,
}

impl RegistryReleaseMetadata {
    pub fn validate_contract(&self) -> Result<(), PackageError> {
        if self.module_id.get_version_num() != 4 {
            return Err(PackageError::Invalid(format!(
                "registry release `{}` has an invalid moduleId",
                self.version
            )));
        }
        if !valid_language_version(&self.language_version) {
            return Err(PackageError::Invalid(format!(
                "registry release `{}` has an invalid languageVersion",
                self.version
            )));
        }
        validate_sha256(&self.archive_sha256, "registry archive hash")?;
        if self.size_bytes == 0 || self.size_bytes > MAX_ARCHIVE_BYTES as u64 {
            return Err(PackageError::Invalid(format!(
                "registry release `{}` has an invalid compressed size",
                self.version
            )));
        }
        if self.expanded_size == 0 || self.expanded_size > MAX_EXPANDED_BYTES {
            return Err(PackageError::Invalid(format!(
                "registry release `{}` has an invalid expanded size",
                self.version
            )));
        }
        if self.file_count == 0 || self.file_count > MAX_FILES {
            return Err(PackageError::Invalid(format!(
                "registry release `{}` has an invalid file count",
                self.version
            )));
        }
        for (alias, dependency) in &self.dependencies {
            if !valid_alias(alias) {
                return Err(PackageError::Invalid(format!(
                    "registry release `{}` has invalid dependency alias `{alias}`",
                    self.version
                )));
            }
            if matches!(dependency, Dependency::Path { .. }) {
                return Err(PackageError::Invalid(format!(
                    "registry release `{}` contains a path dependency",
                    self.version
                )));
            }
        }
        if self.exports.is_empty() {
            return Err(PackageError::Invalid(format!(
                "registry release `{}` has no export groups",
                self.version
            )));
        }
        for (group_name, group) in &self.exports {
            if !valid_alias(group_name) || group.documents.is_empty() {
                return Err(PackageError::Invalid(format!(
                    "registry release `{}` has invalid export group `{group_name}`",
                    self.version
                )));
            }
            let mut documents = BTreeSet::new();
            for document in &group.documents {
                validate_relative_path(document, "registry export document")?;
                require_donder_document(document, "registry export document")?;
                if !documents.insert(document) {
                    return Err(PackageError::Invalid(format!(
                        "registry export group `{group_name}` lists `{document}` more than once"
                    )));
                }
            }
            let mut objects = BTreeSet::new();
            for object in &group.objects {
                if !documents.contains(&object.document)
                    || !valid_object_name(&object.name)
                    || !objects.insert(object.name.as_str())
                {
                    return Err(PackageError::Invalid(format!(
                        "registry export group `{group_name}` has an invalid or duplicate object"
                    )));
                }
            }
        }
        Ok(())
    }

    pub fn is_runtime_compatible(&self) -> bool {
        self.language_version == LANGUAGE_VERSION
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct RegistryDownloadResponse {
    pub registry_version: u8,
    pub package: PackageId,
    pub version: Version,
    pub module_id: Uuid,
    pub archive_sha256: String,
    pub size_bytes: u64,
    pub status: RegistryReleaseStatus,
    pub url: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RegistryReleaseStatus {
    Published,
    Yanked,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(
    tag = "action",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum DeviceLoginRequest {
    Start {
        registry_version: u8,
        client_name: String,
    },
    Poll {
        registry_version: u8,
        device_code: String,
    },
    Approve {
        registry_version: u8,
        user_code: String,
    },
    Deny {
        registry_version: u8,
        user_code: String,
    },
    Refresh {
        registry_version: u8,
        refresh_credential: String,
    },
    Revoke {
        registry_version: u8,
        refresh_credential: String,
    },
    #[serde(rename = "whoami")]
    WhoAmI { registry_version: u8 },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct DeviceStartResponse {
    pub registry_version: u8,
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    pub interval: u64,
    pub expires_in: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct DeviceTokenResponse {
    pub registry_version: u8,
    pub access_token: String,
    pub refresh_credential: String,
    pub expires_in: u64,
    pub refresh_expires_in: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct DeviceIdentityResponse {
    pub registry_version: u8,
    pub username: String,
    pub credential_id: Uuid,
    pub scopes: Vec<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct PublishStageRequest {
    pub registry_version: u8,
    pub action: PublishManagementAction,
    pub original_filename: String,
    pub size_bytes: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PublishManagementAction {
    Stage,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct PublishStageResponse {
    pub registry_version: u8,
    pub upload_id: Uuid,
    pub upload_url: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct PublishFinalizeRequest {
    pub registry_version: u8,
    pub upload_id: Uuid,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
pub struct PublishFinalizeResponse {
    pub registry_version: u8,
    pub package: PackageId,
    pub version: Version,
    pub version_id: Uuid,
    pub archive_sha256: String,
}
