use serde::{ser::Serializer, Serialize};
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

    fn serialize_field<T>(&mut self, key: &'static str, value: &T) -> Result<(), Self::Error>
    where
        T: serde::Serialize + ?Sized,
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

    #[test]
    fn test_serialize_bool() {
        let serializer = EnabledKeysSerializer;
        let true_serialized_res = serializer.serialize_bool(true);
        assert!(true_serialized_res.is_ok());
        let true_serialized = true_serialized_res.unwrap();
        match true_serialized {
            SerializedValue::Flag(bool) => assert_eq!(bool, true),
            SerializedValue::EnabledSet(_) => assert!(false),
        }

        let serializer = EnabledKeysSerializer;
        let false_serialized_res = serializer.serialize_bool(false);
        assert!(false_serialized_res.is_ok());
        let false_serialized = false_serialized_res.unwrap();
        match false_serialized {
            SerializedValue::Flag(bool) => assert_eq!(bool, false),
            SerializedValue::EnabledSet(_) => assert!(false),
        }
    }

    #[test]
    fn test_serialize_i8() {
        let serializer = EnabledKeysSerializer;
        let serialized_res = serializer.serialize_i8(1 as i8);
        assert!(serialized_res.is_err());
        let error = serialized_res.expect_err("Expected an error of type SerializationError::UnsupportedType");
        match error {
            SerializationError::UnsupportedType => assert!(true),
            SerializationError::Message(_) => assert!(false),
        };

        let serializer = EnabledKeysSerializer;
        let serialized_res = serializer.serialize_i8(-1 as i8);
        assert!(serialized_res.is_err());
        let error = serialized_res.expect_err("Expected an error of type SerializationError::UnsupportedType");
        match error {
            SerializationError::UnsupportedType => assert!(true),
            SerializationError::Message(_) => assert!(false),
        };
    }

    #[test]
    fn test_serialize_i16() {
        let serializer = EnabledKeysSerializer;
        let serialized_res = serializer.serialize_i16(1 as i16);
        assert!(serialized_res.is_err());
        let error = serialized_res.expect_err("Expected an error of type SerializationError::UnsupportedType");
        match error {
            SerializationError::UnsupportedType => assert!(true),
            SerializationError::Message(_) => assert!(false),
        };

        let serializer = EnabledKeysSerializer;
        let serialized_res = serializer.serialize_i16(-1 as i16);
        assert!(serialized_res.is_err());
        let error = serialized_res.expect_err("Expected an error of type SerializationError::UnsupportedType");
        match error {
            SerializationError::UnsupportedType => assert!(true),
            SerializationError::Message(_) => assert!(false),
        };
    }

    #[test]
    fn test_serialize_i32() {
        let serializer = EnabledKeysSerializer;
        let serialized_res = serializer.serialize_i32(1 as i32);
        assert!(serialized_res.is_err());
        let error = serialized_res.expect_err("Expected an error of type SerializationError::UnsupportedType");
        match error {
            SerializationError::UnsupportedType => assert!(true),
            SerializationError::Message(_) => assert!(false),
        };

        let serializer = EnabledKeysSerializer;
        let serialized_res = serializer.serialize_i32(-1 as i32);
        assert!(serialized_res.is_err());
        let error = serialized_res.expect_err("Expected an error of type SerializationError::UnsupportedType");
        match error {
            SerializationError::UnsupportedType => assert!(true),
            SerializationError::Message(_) => assert!(false),
        };
    }

    #[test]
    fn test_serialize_i64() {
        let serializer = EnabledKeysSerializer;
        let serialized_res = serializer.serialize_i64(1 as i64);
        assert!(serialized_res.is_err());
        let error = serialized_res.expect_err("Expected an error of type SerializationError::UnsupportedType");
        match error {
            SerializationError::UnsupportedType => assert!(true),
            SerializationError::Message(_) => assert!(false),
        };

        let serializer = EnabledKeysSerializer;
        let serialized_res = serializer.serialize_i64(-1 as i64);
        assert!(serialized_res.is_err());
        let error = serialized_res.expect_err("Expected an error of type SerializationError::UnsupportedType");
        match error {
            SerializationError::UnsupportedType => assert!(true),
            SerializationError::Message(_) => assert!(false),
        };
    }

    #[test]
    fn test_serialize_u8() {
        let serializer = EnabledKeysSerializer;
        let serialized_res = serializer.serialize_u8(1 as u8);
        assert!(serialized_res.is_err());
        let error = serialized_res.expect_err("Expected an error of type SerializationError::UnsupportedType");
        match error {
            SerializationError::UnsupportedType => assert!(true),
            SerializationError::Message(_) => assert!(false),
        };

        let serializer = EnabledKeysSerializer;
        let serialized_res = serializer.serialize_u8(152 as u8);
        assert!(serialized_res.is_err());
        let error = serialized_res.expect_err("Expected an error of type SerializationError::UnsupportedType");
        match error {
            SerializationError::UnsupportedType => assert!(true),
            SerializationError::Message(_) => assert!(false),
        };
    }

    #[test]
    fn test_serialize_u16() {
        let serializer = EnabledKeysSerializer;
        let serialized_res = serializer.serialize_u16(1 as u16);
        assert!(serialized_res.is_err());
        let error = serialized_res.expect_err("Expected an error of type SerializationError::UnsupportedType");
        match error {
            SerializationError::UnsupportedType => assert!(true),
            SerializationError::Message(_) => assert!(false),
        };

        let serializer = EnabledKeysSerializer;
        let serialized_res = serializer.serialize_u16(152 as u16);
        assert!(serialized_res.is_err());
        let error = serialized_res.expect_err("Expected an error of type SerializationError::UnsupportedType");
        match error {
            SerializationError::UnsupportedType => assert!(true),
            SerializationError::Message(_) => assert!(false),
        };
    }

    #[test]
    fn test_serialize_u32() {
        let serializer = EnabledKeysSerializer;
        let serialized_res = serializer.serialize_u32(1 as u32);
        assert!(serialized_res.is_err());
        let error = serialized_res.expect_err("Expected an error of type SerializationError::UnsupportedType");
        match error {
            SerializationError::UnsupportedType => assert!(true),
            SerializationError::Message(_) => assert!(false),
        };

        let serializer = EnabledKeysSerializer;
        let serialized_res = serializer.serialize_u32(152 as u32);
        assert!(serialized_res.is_err());
        let error = serialized_res.expect_err("Expected an error of type SerializationError::UnsupportedType");
        match error {
            SerializationError::UnsupportedType => assert!(true),
            SerializationError::Message(_) => assert!(false),
        };
    }

    #[test]
    fn test_serialize_u64() {
        let serializer = EnabledKeysSerializer;
        let serialized_res = serializer.serialize_u64(1 as u64);
        assert!(serialized_res.is_err());
        let error = serialized_res.expect_err("Expected an error of type SerializationError::UnsupportedType");
        match error {
            SerializationError::UnsupportedType => assert!(true),
            SerializationError::Message(_) => assert!(false),
        };

        let serializer = EnabledKeysSerializer;
        let serialized_res = serializer.serialize_u64(152 as u64);
        assert!(serialized_res.is_err());
        let error = serialized_res.expect_err("Expected an error of type SerializationError::UnsupportedType");
        match error {
            SerializationError::UnsupportedType => assert!(true),
            SerializationError::Message(_) => assert!(false),
        };
    }

    #[test]
    fn test_serialize_f32() {
        let serializer = EnabledKeysSerializer;
        let serialized_res = serializer.serialize_f32(1.0 as f32);
        assert!(serialized_res.is_err());
        let error = serialized_res.expect_err("Expected an error of type SerializationError::UnsupportedType");
        match error {
            SerializationError::UnsupportedType => assert!(true),
            SerializationError::Message(_) => assert!(false),
        };

        let serializer = EnabledKeysSerializer;
        let serialized_res = serializer.serialize_f32(-152.58 as f32);
        assert!(serialized_res.is_err());
        let error = serialized_res.expect_err("Expected an error of type SerializationError::UnsupportedType");
        match error {
            SerializationError::UnsupportedType => assert!(true),
            SerializationError::Message(_) => assert!(false),
        };
    }

    #[test]
    fn test_serialize_f64() {
        let serializer = EnabledKeysSerializer;
        let serialized_res = serializer.serialize_f64(-87.0 as f64);
        assert!(serialized_res.is_err());
        let error = serialized_res.expect_err("Expected an error of type SerializationError::UnsupportedType");
        match error {
            SerializationError::UnsupportedType => assert!(true),
            SerializationError::Message(_) => assert!(false),
        };

        let serializer = EnabledKeysSerializer;
        let serialized_res = serializer.serialize_f64(-789.0 as f64);
        assert!(serialized_res.is_err());
        let error = serialized_res.expect_err("Expected an error of type SerializationError::UnsupportedType");
        match error {
            SerializationError::UnsupportedType => assert!(true),
            SerializationError::Message(_) => assert!(false),
        };
    }

    #[test]
    fn test_serialize_char() {
        let serializer = EnabledKeysSerializer;
        let serialized_res = serializer.serialize_char('5');
        assert!(serialized_res.is_err());
        let error = serialized_res.expect_err("Expected an error of type SerializationError::UnsupportedType");
        match error {
            SerializationError::UnsupportedType => assert!(true),
            SerializationError::Message(_) => assert!(false),
        };

        let serializer = EnabledKeysSerializer;
        let serialized_res = serializer.serialize_char('a');
        assert!(serialized_res.is_err());
        let error = serialized_res.expect_err("Expected an error of type SerializationError::UnsupportedType");
        match error {
            SerializationError::UnsupportedType => assert!(true),
            SerializationError::Message(_) => assert!(false),
        };
    }

    #[test]
    fn test_serialize_str() {
        let serializer = EnabledKeysSerializer;
        let serialized_res = serializer.serialize_str("Remus");
        assert!(serialized_res.is_err());
        let error = serialized_res.expect_err("Expected an error of type SerializationError::UnsupportedType");
        match error {
            SerializationError::UnsupportedType => assert!(true),
            SerializationError::Message(_) => assert!(false),
        };

        let serializer = EnabledKeysSerializer;
        let serialized_res = serializer.serialize_str("Romulus");
        assert!(serialized_res.is_err());
        let error = serialized_res.expect_err("Expected an error of type SerializationError::UnsupportedType");
        match error {
            SerializationError::UnsupportedType => assert!(true),
            SerializationError::Message(_) => assert!(false),
        };
    }

    #[test]
    fn test_serialize_bytes() {
        let serializer = EnabledKeysSerializer;
        let arr: [u8; 3] = [5, 7, 12];
        let serialized_res = serializer.serialize_bytes(&arr);
        assert!(serialized_res.is_err());
        let error = serialized_res.expect_err("Expected an error of type SerializationError::UnsupportedType");
        match error {
            SerializationError::UnsupportedType => assert!(true),
            SerializationError::Message(_) => assert!(false),
        };

        let serializer = EnabledKeysSerializer;
        let arr: [u8; 0] = [];
        let serialized_res = serializer.serialize_bytes(&arr);
        assert!(serialized_res.is_err());
        let error = serialized_res.expect_err("Expected an error of type SerializationError::UnsupportedType");
        match error {
            SerializationError::UnsupportedType => assert!(true),
            SerializationError::Message(_) => assert!(false),
        };
    }

    #[test]
    fn test_serialize_none() {
        let serializer = EnabledKeysSerializer;
        let serialized_res = serializer.serialize_none();
        assert!(serialized_res.is_err());
        let error = serialized_res.expect_err("Expected an error of type SerializationError::UnsupportedType");
        match error {
            SerializationError::UnsupportedType => assert!(true),
            SerializationError::Message(_) => assert!(false),
        };

        let serializer = EnabledKeysSerializer;
        let serialized_res = serializer.serialize_none();
        assert!(serialized_res.is_err());
        let error = serialized_res.expect_err("Expected an error of type SerializationError::UnsupportedType");
        match error {
            SerializationError::UnsupportedType => assert!(true),
            SerializationError::Message(_) => assert!(false),
        };
    }

    #[test]
    fn test_serialize_some() {
        #[derive(Serialize)]
        enum People {
            _Claudius,
            _Marcus,
            _Meto,
            _Paulus,
            _Remus,
            Romulus,
        }
        let serializer = EnabledKeysSerializer;
        let serialized_res = serializer.serialize_some(&People::Romulus);
        assert!(serialized_res.is_err());
        let error = serialized_res.expect_err("Expected an error of type SerializationError::UnsupportedType");
        match error {
            SerializationError::UnsupportedType => assert!(true),
            SerializationError::Message(_) => assert!(false),
        }
    }

    #[test]
    fn test_serialize_unit() {
        let serializer = EnabledKeysSerializer;
        let serialized_res = serializer.serialize_unit();
        assert!(serialized_res.is_err());
        let error = serialized_res.expect_err("Expected an error of type SerializationError::UnsupportedType");
        match error {
            SerializationError::UnsupportedType => assert!(true),
            SerializationError::Message(_) => assert!(false),
        }
    }

    #[test]
    fn test_serialize_unit_struct() {
        let serializer = EnabledKeysSerializer;
        let serialized_res = serializer.serialize_unit_struct("name");
        assert!(serialized_res.is_err());
        let error = serialized_res.expect_err("Expected an error of type SerializationError::UnsupportedType");
        match error {
            SerializationError::UnsupportedType => assert!(true),
            SerializationError::Message(_) => assert!(false),
        }
    }

    #[test]
    fn test_serialize_unit_variant() {
        let serializer = EnabledKeysSerializer;
        let serialized_res = serializer.serialize_unit_variant("name", 1 as u32, "variant");
        assert!(serialized_res.is_err());
        let error = serialized_res.expect_err("Expected an error of type SerializationError::UnsupportedType");
        match error {
            SerializationError::UnsupportedType => assert!(true),
            SerializationError::Message(_) => assert!(false),
        }
    }

    #[test]
    fn test_serialize_newtype_struct() {
        let serializer = EnabledKeysSerializer;
        let value = 12 as i32;
        let serialized_res = serializer.serialize_newtype_struct("name", &value);
        assert!(serialized_res.is_err());
        let error = serialized_res.expect_err("Expected an error of type SerializationError::UnsupportedType");
        match error {
            SerializationError::UnsupportedType => assert!(true),
            SerializationError::Message(_) => assert!(false),
        }
    }

    #[test]
    fn test_serialize_newtype_variant() {
        let serializer = EnabledKeysSerializer;
        let arr: [i32; 3] = [1, 2, 3];
        let serialized_res = serializer.serialize_newtype_variant("name", 18 as u32, "variant", &arr);
        assert!(serialized_res.is_err());
        let error = serialized_res.expect_err("Expected an error of type SerializationError::UnsupportedType");
        match error {
            SerializationError::UnsupportedType => assert!(true),
            SerializationError::Message(_) => assert!(false),
        }
    }

    #[test]
    fn test_serialize_seq() {
        let serializer = EnabledKeysSerializer;
        let value = 500 as usize;
        let serialized_res = serializer.serialize_seq(Some(value));
        assert!(serialized_res.is_err());
    }

    #[test]
    fn test_serialize_tuple() {
        let serializer = EnabledKeysSerializer;
        let value = 500 as usize;
        let serialized_res = serializer.serialize_tuple(value);
        assert!(serialized_res.is_err());
    }

    #[test]
    fn test_serialize_tuple_struct() {
        let serializer = EnabledKeysSerializer;
        let value = 500 as usize;
        let serialized_res = serializer.serialize_tuple_struct("name", value);
        assert!(serialized_res.is_err());
    }

    #[test]
    fn test_serialize_tuple_variant() {
        let serializer = EnabledKeysSerializer;
        let value = 500 as usize;
        let serialized_res = serializer.serialize_tuple_variant("name", 14 as u32, "variant", value);
        assert!(serialized_res.is_err());
    }

    #[test]
    fn test_serialize_map() {
        let serializer = EnabledKeysSerializer;
        let value = 500 as usize;
        let serialized_res = serializer.serialize_map(Some(value));
        assert!(serialized_res.is_err());
    }

    #[test]
    fn test_serialize_struct_variant() {
        let serializer = EnabledKeysSerializer;
        let value = 500 as usize;
        let serialized_res = serializer.serialize_struct_variant("name", 5 as u32, "variant", value);
        assert!(serialized_res.is_err());
    }
}
