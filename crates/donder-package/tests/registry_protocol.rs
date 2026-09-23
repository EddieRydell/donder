use donder_package::{PackageId, RegistryClient, RegistryConfig};
use httpmock::Method::{GET, POST, PUT};
use httpmock::MockServer;
use serde_json::json;
use uuid::Uuid;

fn discovery(server: &MockServer) -> serde_json::Value {
    let base = server.base_url();
    let protocol = format!("{base}/functions/v1");
    json!({
        "registryVersion": 1,
        "protocolUrl": protocol,
        "websiteUrl": base,
        "endpoints": {
            "resolve": format!("{protocol}/registry-v1?op=resolve"),
            "download": format!("{protocol}/registry-v1?op=download"),
            "deviceLogin": format!("{protocol}/device-login"),
            "publishStage": format!("{protocol}/manage-package"),
            "publishFinalize": format!("{protocol}/publish-package-version")
        }
    })
}

#[test]
fn mocked_registry_supports_device_auth_identity_and_publication() {
    let server = MockServer::start();
    let discovery_mock = server.mock(|when, then| {
        when.method(GET).path("/.well-known/donder-registry.json");
        then.status(200).json_body(discovery(&server));
    });
    let start_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/functions/v1/device-login")
            .json_body(json!({
                "action": "start",
                "registryVersion": 1,
                "clientName": "Donder test"
            }));
        then.status(200).json_body(json!({
            "registryVersion": 1,
            "deviceCode": "device-secret",
            "userCode": "ABCD1234",
            "verificationUri": format!("{}/device/approve?code=ABCD1234", server.base_url()),
            "interval": 1,
            "expiresIn": 600
        }));
    });
    let poll_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/functions/v1/device-login")
            .json_body(json!({
                "action": "poll",
                "registryVersion": 1,
                "deviceCode": "device-secret"
            }));
        then.status(200).json_body(json!({
            "registryVersion": 1,
            "accessToken": "access-secret",
            "refreshCredential": "refresh-secret",
            "expiresIn": 900,
            "refreshExpiresIn": 7_776_000
        }));
    });
    let identity_id = Uuid::new_v4();
    let identity_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/functions/v1/device-login")
            .header("authorization", "Bearer access-secret")
            .json_body(json!({
                "action": "whoami",
                "registryVersion": 1
            }));
        then.status(200).json_body(json!({
            "registryVersion": 1,
            "username": "alice",
            "credentialId": identity_id,
            "scopes": ["publication:manage"]
        }));
    });

    let upload_id = Uuid::new_v4();
    let archive = "strict archive fixture";
    let stage_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/functions/v1/manage-package")
            .header("authorization", "Bearer access-secret")
            .json_body(json!({
                "registryVersion": 1,
                "action": "stage",
                "originalFilename": "library-1.0.0.zip",
                "sizeBytes": archive.len()
            }));
        then.status(200).json_body(json!({
            "registryVersion": 1,
            "uploadId": upload_id,
            "uploadUrl": format!("{}/storage/upload", server.base_url())
        }));
    });
    let upload_mock = server.mock(|when, then| {
        when.method(PUT)
            .path("/storage/upload")
            .header("content-type", "application/zip")
            .header("x-upsert", "false")
            .body(archive);
        then.status(200);
    });
    let version_id = Uuid::new_v4();
    let finalize_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/functions/v1/publish-package-version")
            .header("authorization", "Bearer access-secret")
            .json_body(json!({
                "registryVersion": 1,
                "uploadId": upload_id
            }));
        then.status(200).json_body(json!({
            "registryVersion": 1,
            "package": "alice/library",
            "version": "1.0.0",
            "versionId": version_id,
            "archiveSha256": "a".repeat(64)
        }));
    });

    let client = RegistryClient::discover(&RegistryConfig {
        registry: server.base_url(),
    })
    .expect("registry discovery");
    let authorization = client
        .start_device_login("Donder test")
        .expect("device authorization");
    assert_eq!(authorization.user_code, "ABCD1234");
    let token = client
        .poll_device_login(&authorization.device_code)
        .expect("poll")
        .expect("approved token");
    assert_eq!(token.access_token, "access-secret");
    let identity = client.identity(&token.access_token).expect("identity");
    assert_eq!(identity.username, "alice");
    assert_eq!(identity.credential_id, identity_id);
    let published = client
        .publish(&token.access_token, "library-1.0.0.zip", archive.as_bytes())
        .expect("publication");
    assert_eq!(
        published.package,
        PackageId::new("alice/library").expect("package")
    );
    assert_eq!(published.version_id, version_id);

    discovery_mock.assert();
    start_mock.assert();
    poll_mock.assert();
    identity_mock.assert();
    stage_mock.assert();
    upload_mock.assert();
    finalize_mock.assert();
}

#[test]
fn pending_device_authorization_is_not_treated_as_failure() {
    let server = MockServer::start();
    let discovery_mock = server.mock(|when, then| {
        when.method(GET).path("/.well-known/donder-registry.json");
        then.status(200).json_body(discovery(&server));
    });
    let poll_mock = server.mock(|when, then| {
        when.method(POST)
            .path("/functions/v1/device-login")
            .json_body(json!({
                "action": "poll",
                "registryVersion": 1,
                "deviceCode": "pending-device"
            }));
        then.status(428)
            .json_body(json!({ "error": "authorization_pending" }));
    });

    let client = RegistryClient::discover(&RegistryConfig {
        registry: server.base_url(),
    })
    .expect("registry discovery");
    assert_eq!(
        client
            .poll_device_login("pending-device")
            .expect("pending poll"),
        None
    );

    discovery_mock.assert();
    poll_mock.assert();
}
