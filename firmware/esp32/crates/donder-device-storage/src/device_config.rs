use crate::{Error, Storage};
use alloc::string::String;
use donder_device_protocol::NetworkCredentials;
use serde::{Deserialize, Serialize};

/// Everything a controller keeps about itself. A missing record is a fresh
/// controller: default name and its own access point.
// Deliberately no Debug: the network password must not enter logs.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceConfig {
    pub name: String,
    /// The station network to join; `None` keeps the controller's access point.
    pub network: Option<NetworkCredentials>,
}

/// Parse the editor's `PUT /network` body.
pub fn network_from_json(bytes: &[u8]) -> Result<NetworkCredentials, Error> {
    let mut unescaped = [0; 64];
    let (network, _) =
        serde_json_core::from_slice_escaped::<NetworkCredentials>(bytes, &mut unescaped)
            .map_err(|_| Error::INVALID)?;
    if !network.is_valid() {
        return Err(Error::INVALID);
    }
    Ok(network)
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
