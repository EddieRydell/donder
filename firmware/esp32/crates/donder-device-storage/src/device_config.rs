use crate::{Error, Record, Storage};
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
    pub fn validate(&self) -> Result<(), Error> {
        if !(1..=32).contains(&self.ssid.len()) || !(8..=64).contains(&self.password.len()) {
            return Err(Error::INVALID);
        }
        Ok(())
    }

    /// Parse the editor's `{"ssid": ..., "password": ...}` request body.
    pub fn from_json(bytes: &[u8]) -> Result<Self, Error> {
        let mut unescaped = [0; 64];
        let parsed = serde_json_core::from_slice_escaped::<Self>(bytes, &mut unescaped)
            .map_err(|_| Error::INVALID)
            .and_then(|(network, length)| {
                if length != bytes.len() {
                    return Err(Error::INVALID);
                }
                network.validate()?;
                Ok(network)
            });
        unescaped.fill(0);
        parsed
    }
}

impl DeviceConfig {
    pub fn validate(&self) -> Result<(), Error> {
        if !valid_name(&self.name)
            || self
                .token
                .is_some_and(|token| !token.iter().all(u8::is_ascii_hexdigit))
        {
            return Err(Error::INVALID);
        }
        self.network.as_ref().map_or(Ok(()), Network::validate)
    }

    pub fn load<S: Storage>(storage: &mut S) -> Result<Option<Self>, Error> {
        let Some(mut bytes) = crate::read(storage, Record::Device, 1024)? else {
            return Ok(None);
        };
        let mut unescaped = [0; 64];
        let decoded = serde_json_core::from_slice_escaped::<Self>(&bytes, &mut unescaped)
            .map_err(|_| Error::CORRUPTION)
            .and_then(|(config, length)| {
                if length != bytes.len() {
                    return Err(Error::CORRUPTION);
                }
                config.validate()?;
                Ok(config)
            });
        bytes.fill(0);
        unescaped.fill(0);
        decoded.map(Some)
    }

    pub fn save<S: Storage>(&self, storage: &mut S) -> Result<(), Error> {
        self.validate()?;
        let mut bytes = [0; 1024];
        let result = serde_json_core::to_slice(self, &mut bytes)
            .map_err(|_| Error::INVALID)
            .and_then(|length| crate::write(storage, Record::Device, &bytes[..length]));
        bytes.fill(0);
        result
    }
}
