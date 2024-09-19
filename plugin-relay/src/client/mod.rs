use std::{fmt::Display, str::FromStr};

use tonic::metadata::{errors::InvalidMetadataValue, AsciiMetadataValue};

mod grpc;
mod plugin;

pub use plugin::RelayClientPlugin;

#[derive(Debug, Clone)]
pub struct AsciiString {
    metadata_value: AsciiMetadataValue,
    string: String,
}

impl FromStr for AsciiString {
    type Err = InvalidMetadataValue;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mv = AsciiMetadataValue::from_str(s)?;
        Ok(AsciiString {
            metadata_value: mv,
            string: s.to_owned(),
        })
    }
}

impl Display for AsciiString {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.string)
    }
}

impl AsciiString {
    pub fn as_str(&self) -> &str {
        &self.string
    }
}
