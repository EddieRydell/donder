use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use donder_package::{DeviceTokenResponse, PackageError, RegistryClient, RegistryConfig};
use keyring::Entry;
use serde::{Deserialize, Serialize};

use crate::CliError;

const CREDENTIAL_SERVICE: &str = "donder-registry-v1";
const CREDENTIAL_ACCOUNT: &str = "active-registry";
const ACCESS_EXPIRY_MARGIN_SECONDS: u64 = 30;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[serde(rename_all = "camelCase")]
struct CredentialBundle {
    registry: String,
    access_token: String,
    access_expires_at: u64,
    refresh_credential: String,
    refresh_expires_at: u64,
}

pub(crate) fn login() -> Result<(), CliError> {
    let config = RegistryConfig::read()?;
    let client = RegistryClient::discover(&config)?;
    let authorization = client.start_device_login(client_name())?;
    if authorization.device_code.is_empty()
        || authorization.user_code.is_empty()
        || authorization.verification_uri.is_empty()
        || authorization.interval == 0
        || authorization.expires_in == 0
        || authorization.expires_in > 600
    {
        return Err(PackageError::Invalid(
            "registry returned an invalid device authorization".to_string(),
        )
        .into());
    }

    println!("Authorize Donder with code {}", authorization.user_code);
    println!("{}", authorization.verification_uri);
    webbrowser::open(&authorization.verification_uri).map_err(|error| {
        PackageError::Invalid(format!(
            "could not open the device authorization page: {error}"
        ))
    })?;

    let deadline = Instant::now()
        .checked_add(Duration::from_secs(authorization.expires_in))
        .ok_or_else(|| {
            PackageError::Invalid("device authorization expiry is invalid".to_string())
        })?;
    let poll_interval = Duration::from_secs(authorization.interval);
    loop {
        if Instant::now() >= deadline {
            return Err(PackageError::Invalid(
                "device authorization expired before approval".to_string(),
            )
            .into());
        }
        thread::sleep(poll_interval);
        if let Some(token) = client.poll_device_login(&authorization.device_code)? {
            store_bundle(bundle_from_token(
                normalized_registry(client.website_url().as_str()),
                token,
            )?)?;
            println!("logged in to {}", client.website_url());
            return Ok(());
        }
    }
}

pub(crate) fn logout() -> Result<(), CliError> {
    let bundle = load_bundle()?;
    let config = RegistryConfig {
        registry: bundle.registry.clone(),
    };
    let client = RegistryClient::discover(&config)?;
    client.revoke_credential(&bundle.refresh_credential)?;
    credential_entry()?
        .delete_credential()
        .map_err(secure_storage_error)?;
    println!("logged out of {}", bundle.registry);
    Ok(())
}

pub(crate) fn whoami() -> Result<(), CliError> {
    let (client, access_token) = authenticated_client()?;
    let identity = client.identity(&access_token)?;
    println!("{} at {}", identity.username, client.website_url());
    println!("credential: {}", identity.credential_id);
    println!("scopes: {}", identity.scopes.join(", "));
    Ok(())
}

pub(crate) fn authenticated_client() -> Result<(RegistryClient, String), CliError> {
    let config = RegistryConfig::read()?;
    let client = RegistryClient::discover(&config)?;
    let mut bundle = load_bundle()?;
    let active_registry = normalized_registry(client.website_url().as_str());
    if bundle.registry != active_registry {
        return Err(PackageError::Invalid(format!(
            "stored credentials belong to `{}`, but the active registry is `{active_registry}`; log in again",
            bundle.registry
        ))
        .into());
    }

    let now = unix_time()?;
    if bundle.refresh_expires_at <= now {
        return Err(PackageError::Invalid(
            "registry credentials have expired; run `donder login`".to_string(),
        )
        .into());
    }
    if bundle.access_expires_at > now.saturating_add(ACCESS_EXPIRY_MARGIN_SECONDS) {
        return Ok((client, bundle.access_token));
    }

    let refreshed = client.refresh_credential(&bundle.refresh_credential)?;
    bundle = bundle_from_token(active_registry, refreshed)?;
    let access_token = bundle.access_token.clone();
    store_bundle(bundle)?;
    Ok((client, access_token))
}

fn bundle_from_token(
    registry: String,
    token: DeviceTokenResponse,
) -> Result<CredentialBundle, CliError> {
    let now = unix_time()?;
    let access_expires_at = now.checked_add(token.expires_in).ok_or_else(|| {
        PackageError::Invalid("registry access-token expiry is invalid".to_string())
    })?;
    let refresh_expires_at = now.checked_add(token.refresh_expires_in).ok_or_else(|| {
        PackageError::Invalid("registry refresh-credential expiry is invalid".to_string())
    })?;
    Ok(CredentialBundle {
        registry,
        access_token: token.access_token,
        access_expires_at,
        refresh_credential: token.refresh_credential,
        refresh_expires_at,
    })
}

fn load_bundle() -> Result<CredentialBundle, CliError> {
    let value = credential_entry()?.get_password().map_err(|error| {
        if matches!(error, keyring::Error::NoEntry) {
            PackageError::Invalid("not logged in; run `donder login`".to_string())
        } else {
            secure_storage_error(error)
        }
    })?;
    let bundle = serde_json::from_str::<CredentialBundle>(&value).map_err(|error| {
        PackageError::Invalid(format!(
            "stored registry credentials are invalid: {error}; run `donder logout` and log in again"
        ))
    })?;
    if bundle.registry.is_empty()
        || bundle.access_token.is_empty()
        || bundle.refresh_credential.is_empty()
    {
        return Err(PackageError::Invalid(
            "stored registry credentials are incomplete".to_string(),
        )
        .into());
    }
    Ok(bundle)
}

fn store_bundle(bundle: CredentialBundle) -> Result<(), CliError> {
    let entry = credential_entry()?;
    store_bundle_with_entry(&entry, bundle)
}

fn store_bundle_with_entry(entry: &Entry, bundle: CredentialBundle) -> Result<(), CliError> {
    let value = serde_json::to_string(&bundle).map_err(PackageError::Json)?;
    entry.set_password(&value).map_err(secure_storage_error)?;
    Ok(())
}

fn credential_entry() -> Result<Entry, CliError> {
    Ok(Entry::new(CREDENTIAL_SERVICE, CREDENTIAL_ACCOUNT).map_err(secure_storage_error)?)
}

fn secure_storage_error(error: keyring::Error) -> PackageError {
    PackageError::Invalid(format!("OS credential storage is unavailable: {error}"))
}

fn unix_time() -> Result<u64, CliError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .map_err(|error| PackageError::Invalid(format!("system clock is invalid: {error}")).into())
}

fn normalized_registry(value: &str) -> String {
    value.trim_end_matches('/').to_string()
}

fn client_name() -> String {
    format!("Donder CLI {}", env!("CARGO_PKG_VERSION"))
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::sync::{Arc, Mutex};

    use super::*;
    use keyring_core::mock::{Cred as MockCredential, CredData as MockCredentialData};

    fn mock_entry() -> (Entry, Arc<MockCredential>) {
        let credential = Arc::new(MockCredential {
            specifiers: (
                CREDENTIAL_SERVICE.to_string(),
                CREDENTIAL_ACCOUNT.to_string(),
            ),
            inner: Mutex::new(RefCell::new(MockCredentialData::default())),
        });
        let inner = keyring_core::Entry::new_with_credential(credential.clone());
        (Entry { inner }, credential)
    }

    fn bundle() -> CredentialBundle {
        CredentialBundle {
            registry: "https://registry.donder.dev".to_string(),
            access_token: "access-secret".to_string(),
            access_expires_at: 1_000,
            refresh_credential: "refresh-secret".to_string(),
            refresh_expires_at: 2_000,
        }
    }

    #[test]
    fn secure_storage_failure_has_no_plaintext_fallback() {
        let (entry, credential) = mock_entry();
        credential.set_error(keyring::Error::NoStorageAccess(Box::new(
            std::io::Error::other("locked credential store"),
        )));

        let error = store_bundle_with_entry(&entry, bundle()).expect_err("storage failure");

        assert!(
            error
                .to_string()
                .contains("OS credential storage is unavailable")
        );
        assert!(matches!(
            entry.get_password().expect_err("credential was not stored"),
            keyring::Error::NoEntry
        ));
    }

    #[test]
    fn credential_bundle_is_stored_as_one_secure_record() {
        let (entry, _) = mock_entry();
        let expected = bundle();

        store_bundle_with_entry(&entry, expected.clone()).expect("store");
        let actual: CredentialBundle =
            serde_json::from_str(&entry.get_password().expect("credential")).expect("bundle");

        assert_eq!(actual, expected);
    }
}
