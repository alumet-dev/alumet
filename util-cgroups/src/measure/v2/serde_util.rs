use std::fmt::Display;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SerializationError {
    #[error("unsupported type")]
    UnsupportedType,
    #[error("{0}")]
    Message(String),
}

impl serde::ser::Error for SerializationError {
    fn custom<T: Display>(msg: T) -> Self {
        Self::Message(msg.to_string())
    }
}

pub struct Impossible<T, E>(std::marker::PhantomData<(T, E)>);

impl<T, E: serde::ser::Error> serde::ser::SerializeSeq for Impossible<T, E> {
    type Ok = T;
    type Error = E;

    fn serialize_element<U>(&mut self, _value: &U) -> Result<(), Self::Error>
    where
        U: serde::Serialize + ?Sized,
    {
        unreachable!()
    }

    fn end(self) -> Result<T, E> {
        unreachable!()
    }
}

impl<T, E: serde::ser::Error> serde::ser::SerializeTuple for Impossible<T, E> {
    type Ok = T;
    type Error = E;

    fn serialize_element<U>(&mut self, _value: &U) -> Result<(), Self::Error>
    where
        U: serde::Serialize + ?Sized,
    {
        unreachable!()
    }

    fn end(self) -> Result<T, E> {
        unreachable!()
    }
}

impl<T, E: serde::ser::Error> serde::ser::SerializeTupleStruct for Impossible<T, E> {
    type Ok = T;
    type Error = E;

    fn serialize_field<U>(&mut self, _value: &U) -> Result<(), Self::Error>
    where
        U: serde::Serialize + ?Sized,
    {
        unreachable!()
    }

    fn end(self) -> Result<T, E> {
        unreachable!()
    }
}

impl<T, E: serde::ser::Error> serde::ser::SerializeTupleVariant for Impossible<T, E> {
    type Ok = T;
    type Error = E;

    fn serialize_field<U>(&mut self, _value: &U) -> Result<(), Self::Error>
    where
        U: serde::Serialize + ?Sized,
    {
        unreachable!()
    }

    fn end(self) -> Result<T, E> {
        unreachable!()
    }
}

impl<T, E: serde::ser::Error> serde::ser::SerializeMap for Impossible<T, E> {
    type Ok = T;
    type Error = E;

    fn serialize_key<U>(&mut self, _key: &U) -> Result<(), Self::Error>
    where
        U: serde::Serialize + ?Sized,
    {
        unreachable!()
    }

    fn serialize_value<U>(&mut self, _value: &U) -> Result<(), Self::Error>
    where
        U: serde::Serialize + ?Sized,
    {
        unreachable!()
    }

    fn end(self) -> Result<T, E> {
        unreachable!()
    }
}

impl<T, E: serde::ser::Error> serde::ser::SerializeStructVariant for Impossible<T, E> {
    type Ok = T;
    type Error = E;

    fn serialize_field<U>(&mut self, _key: &'static str, _value: &U) -> Result<(), Self::Error>
    where
        U: serde::Serialize + ?Sized,
    {
        unreachable!()
    }

    fn end(self) -> Result<T, E> {
        unreachable!()
    }
}

#[cfg(test)]
mod tests {
    use crate::measure::v2::serde_util::SerializationError;

    // Doesn't test for custom function from serde::ser::Error implementation for SerializationError
    #[test]
    pub fn test_custom() -> anyhow::Result<()> {
        let msg = SerializationError::UnsupportedType;
        assert_eq!(format!("{msg}"), "unsupported type");
        let msg = SerializationError::Message("My SerializationError message".to_string());
        assert_eq!(format!("{msg}"), "My SerializationError message");
        Ok(())
    }
}
