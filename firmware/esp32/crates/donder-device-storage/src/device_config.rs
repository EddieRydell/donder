use crate::{Error, Storage};
use alloc::string::String;
use serde::{Deserialize, Serialize};

pub const MAX_NAME_BYTES: usize = 32;

/// Everything a controller keeps about itself. A missing record is a fresh
/// controller: default name, its own access point, and unclaimed.
// Deliberately no Debug: the network password and token must not enter logs.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceConfig {
    pub name: String,
    /// The station network to join; `None` keeps the controller's access point.
    pub network: Option<Network>,
    /// Set by the first editor that claims the controller.
    pub token: Option<[u8; 32]>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Network {
    pub ssid: String,
    pub password: String,
}

pub fn valid_name(name: &str) -> bool {
    (1..=MAX_NAME_BYTES).contains(&name.len()) && !name.chars().any(char::is_control)
}

impl Network {
    /// Parse the editor's `{"ssid": ..., "password": ...}` request body: a
    /// WPA2 network name and password.
    pub fn from_json(bytes: &[u8]) -> Result<Self, Error> {
        let mut unescaped = [0; 64];
        let (network, _) = serde_json_core::from_slice_escaped::<Self>(bytes, &mut unescaped)
            .map_err(|_| Error::INVALID)?;
        if !(1..=32).contains(&network.ssid.len()) || !(8..=64).contains(&network.password.len()) {
            return Err(Error::INVALID);
        }
        Ok(network)
    }
}

impl DeviceConfig {
    pub fn load<S: Storage>(storage: &mut S) -> Result<Option<Self>, Error> {
        let Some(bytes) = crate::read(storage)? else {
            return Ok(None);
        };
        let mut unescaped = [0; 64];
        serde_json_core::from_slice_escaped::<Self>(&bytes, &mut unescaped)
            .map(|(config, _)| Some(config))
            .map_err(|_| Error::CORRUPTION)
    }

    pub fn save<S: Storage>(&self, storage: &mut S) -> Result<(), Error> {
        let mut bytes = [0; crate::DEVICE_BYTES];
        let length = serde_json_core::to_slice(self, &mut bytes).map_err(|_| Error::INVALID)?;
        crate::write(storage, &bytes[..length])
    }
}
