use anyhow::anyhow;
use serde::{Serialize, ser::Serializer};
use thiserror::Error;

use super::serde_util::{Impossible, SerializationError};

/// Implementing this trait allows to convert the struct into a list that contains every field that is equal to `true`.
///
/// # Restrictions
/// Only `bool` field types are supported. The structure must not be nested.
///
/// # Example
/// ```ignore
/// use serde::Serialize;
///
/// #[derive(Serialize)]
/// struct Settings {
///     key1: bool,
///     key2: bool,
/// }
///
/// impl EnabledKeys for Settings {}
///
/// let settings = Settings { key1: true, key2: false };
/// let keys = settings.enabled_keys().unwrap();
/// assert_eq!(keys, vec!["key1"]);
/// ```
// NOTE: this example cannot be tested here, there is a unit test at the end of the module.
// If you update this example, please also update the unit test.
pub trait EnabledKeys: Serialize {
    /// Generates the list of enabled keys.
    fn enabled_keys(&self) -> Result<Vec<String>, EnabledKeysError> {
        let result = self.serialize(EnabledKeysSerializer)?;
        match result {
            SerializedValue::Flag(_) => Err(EnabledKeysError(SerializationError::Message(
                "bad result type: is there a bug in the serialization?".to_string(),
            ))),
            SerializedValue::EnabledSet(keys) => Ok(keys),
        }
    }
}

#[derive(Debug, Error)]
#[error("enabled_keys() failed: invalid settings structure")]
pub struct EnabledKeysError(#[from] SerializationError);

pub struct EnabledKeysSerializer;

#[derive(Debug, PartialEq, Eq)]
pub enum SerializedValue {
    /// A single flag.
    Flag(bool),
    /// A set of enabled keys. Disabled keys are not included.
    EnabledSet(Vec<String>),
}

type Unsupported = Impossible<SerializedValue, SerializationError>;

#[allow(unused_variables)]
impl Serializer for EnabledKeysSerializer {
    type Ok = SerializedValue;
    type Error = SerializationError;

    type SerializeSeq = Unsupported;
    type SerializeTuple = Unsupported;
    type SerializeTupleStruct = Unsupported;
    type SerializeTupleVariant = Unsupported;
    type SerializeMap = Unsupported;
    type SerializeStruct = SerializeStructWrapper;
    type SerializeStructVariant = Unsupported;

    fn serialize_bool(self, v: bool) -> Result<Self::Ok, Self::Error> {
        Ok(SerializedValue::Flag(v))
    }

    fn serialize_i8(self, v: i8) -> Result<Self::Ok, Self::Error> {
        Err(SerializationError::UnsupportedType)
    }

    fn serialize_i16(self, v: i16) -> Result<Self::Ok, Self::Error> {
        Err(SerializationError::UnsupportedType)
    }

    fn serialize_i32(self, v: i32) -> Result<Self::Ok, Self::Error> {
        Err(SerializationError::UnsupportedType)
    }

    fn serialize_i64(self, v: i64) -> Result<Self::Ok, Self::Error> {
        Err(SerializationError::UnsupportedType)
    }

    fn serialize_u8(self, v: u8) -> Result<Self::Ok, Self::Error> {
        Err(SerializationError::UnsupportedType)
    }

    fn serialize_u16(self, v: u16) -> Result<Self::Ok, Self::Error> {
        Err(SerializationError::UnsupportedType)
    }

    fn serialize_u32(self, v: u32) -> Result<Self::Ok, Self::Error> {
        Err(SerializationError::UnsupportedType)
    }

    fn serialize_u64(self, v: u64) -> Result<Self::Ok, Self::Error> {
        Err(SerializationError::UnsupportedType)
    }

    fn serialize_f32(self, v: f32) -> Result<Self::Ok, Self::Error> {
        Err(SerializationError::UnsupportedType)
    }

    fn serialize_f64(self, v: f64) -> Result<Self::Ok, Self::Error> {
        Err(SerializationError::UnsupportedType)
    }

    fn serialize_char(self, v: char) -> Result<Self::Ok, Self::Error> {
        Err(SerializationError::UnsupportedType)
    }

    fn serialize_str(self, v: &str) -> Result<Self::Ok, Self::Error> {
        Err(SerializationError::UnsupportedType)
    }

    fn serialize_bytes(self, v: &[u8]) -> Result<Self::Ok, Self::Error> {
        Err(SerializationError::UnsupportedType)
    }

    fn serialize_none(self) -> Result<Self::Ok, Self::Error> {
        // TODO it could be interesting to support Option<_>
        Err(SerializationError::UnsupportedType)
    }

    fn serialize_some<T>(self, value: &T) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + Serialize,
    {
        Err(SerializationError::UnsupportedType)
    }

    fn serialize_unit(self) -> Result<Self::Ok, Self::Error> {
        Err(SerializationError::UnsupportedType)
    }

    fn serialize_unit_struct(self, name: &'static str) -> Result<Self::Ok, Self::Error> {
        Err(SerializationError::UnsupportedType)
    }

    fn serialize_unit_variant(
        self,
        name: &'static str,
        variant_index: u32,
        variant: &'static str,
    ) -> Result<Self::Ok, Self::Error> {
        Err(SerializationError::UnsupportedType)
    }

    fn serialize_newtype_struct<T>(self, name: &'static str, value: &T) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + Serialize,
    {
        Err(SerializationError::UnsupportedType)
    }

    fn serialize_newtype_variant<T>(
        self,
        name: &'static str,
        variant_index: u32,
        variant: &'static str,
        value: &T,
    ) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + Serialize,
    {
        Err(SerializationError::UnsupportedType)
    }

    fn serialize_seq(self, len: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
        Err(SerializationError::UnsupportedType)
    }

    fn serialize_tuple(self, len: usize) -> Result<Self::SerializeTuple, Self::Error> {
        Err(SerializationError::UnsupportedType)
    }

    fn serialize_tuple_struct(self, name: &'static str, len: usize) -> Result<Self::SerializeTupleStruct, Self::Error> {
        Err(SerializationError::UnsupportedType)
    }

    fn serialize_tuple_variant(
        self,
        name: &'static str,
        variant_index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleVariant, Self::Error> {
        Err(SerializationError::UnsupportedType)
    }

    fn serialize_map(self, len: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        Err(SerializationError::UnsupportedType)
    }

    fn serialize_struct(self, name: &'static str, len: usize) -> Result<Self::SerializeStruct, Self::Error> {
        Ok(SerializeStructWrapper {
            enabled_keys: Vec::with_capacity(len),
        })
    }

    fn serialize_struct_variant(
        self,
        name: &'static str,
        variant_index: u32,
        variant: &'static str,
        len: usize,
    ) -> Result<Self::SerializeStructVariant, Self::Error> {
        Err(SerializationError::UnsupportedType)
    }
}

pub struct SerializeStructWrapper {
    enabled_keys: Vec<String>,
}

impl serde::ser::SerializeStruct for SerializeStructWrapper {
    type Ok = SerializedValue;
    type Error = SerializationError;

    fn serialize_field<T: ?Sized>(&mut self, key: &'static str, value: &T) -> Result<(), Self::Error>
    where
        T: serde::Serialize,
    {
        let value = value.serialize(EnabledKeysSerializer)?;
        match value {
            SerializedValue::Flag(enabled) => {
                if enabled {
                    self.enabled_keys.push(key.to_owned());
                }
                Ok(())
            }
            SerializedValue::EnabledSet(_) => Err(SerializationError::Message(
                "nested structures are not supported".to_owned(),
            )),
        }
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(SerializedValue::EnabledSet(self.enabled_keys))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serialize_settings_keys() {
        #[derive(Debug, Serialize)]
        struct Keys {
            abc: bool,
            def: bool,
            #[serde(rename = "pagetables")]
            page_tables: bool,
        }

        // some true, some false
        let key = Keys {
            abc: true,
            def: false,
            page_tables: true,
        };
        let res = key.serialize(EnabledKeysSerializer).unwrap();
        assert_eq!(
            res,
            SerializedValue::EnabledSet(vec![String::from("abc"), String::from("pagetables")])
        );

        // all false
        let key = Keys {
            abc: false,
            def: false,
            page_tables: false,
        };
        let res = key.serialize(EnabledKeysSerializer).unwrap();
        assert_eq!(res, SerializedValue::EnabledSet(vec![]));
    }

    /// Tests the example of [`EnabledKeys`].
    /// This test exists because Rust doctests cannot test private items.
    #[test]
    fn example() {
        use serde::Serialize;
        #[derive(Serialize)]
        struct Settings {
            key1: bool,
            key2: bool,
        }

        impl EnabledKeys for Settings {}

        let settings = Settings {
            key1: true,
            key2: false,
        };
        let keys = settings.enabled_keys().unwrap();
        assert_eq!(keys, vec!["key1"]);
    }
}
