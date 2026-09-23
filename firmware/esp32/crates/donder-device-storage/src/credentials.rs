use crate::{Error, Record, Storage};
use alloc::string::String;
use serde::{Deserialize, Serialize};

// Deliberately no Debug: credentials must not enter device logs.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Credentials {
    pub ssid: String,
    pub password: String,
    pub token: [u8; 32],
}

impl Credentials {
    pub fn validate(&self) -> Result<(), Error> {
        if !(1..=32).contains(&self.ssid.len())
            || !(8..=64).contains(&self.password.len())
            || !self.token.iter().all(u8::is_ascii_hexdigit)
        {
            return Err(Error::INVALID);
        }
        Ok(())
    }

    pub fn load<S: Storage>(storage: &mut S) -> Result<Option<Self>, Error> {
        let Some(mut bytes) = crate::read(storage, Record::Credentials, 1024)? else {
            return Ok(None);
        };
        let mut unescaped = [0; 64];
        let decoded = serde_json_core::from_slice_escaped::<Self>(&bytes, &mut unescaped)
            .map_err(|_| Error::CORRUPTION)
            .and_then(|(credentials, length)| {
                if length != bytes.len() {
                    return Err(Error::CORRUPTION);
                }
                credentials.validate()?;
                Ok(credentials)
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
            .and_then(|length| crate::write(storage, Record::Credentials, &bytes[..length]));
        bytes.fill(0);
        result
    }
}
