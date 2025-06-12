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

    fn serialize_element<U: ?Sized>(&mut self, _value: &U) -> Result<(), Self::Error>
    where
        U: serde::Serialize,
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

    fn serialize_element<U: ?Sized>(&mut self, _value: &U) -> Result<(), Self::Error>
    where
        U: serde::Serialize,
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

    fn serialize_field<U: ?Sized>(&mut self, _value: &U) -> Result<(), Self::Error>
    where
        U: serde::Serialize,
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

    fn serialize_field<U: ?Sized>(&mut self, _value: &U) -> Result<(), Self::Error>
    where
        U: serde::Serialize,
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

    fn serialize_key<U: ?Sized>(&mut self, _key: &U) -> Result<(), Self::Error>
    where
        U: serde::Serialize,
    {
        unreachable!()
    }

    fn serialize_value<U: ?Sized>(&mut self, _value: &U) -> Result<(), Self::Error>
    where
        U: serde::Serialize,
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

    fn serialize_field<U: ?Sized>(&mut self, _key: &'static str, _value: &U) -> Result<(), Self::Error>
    where
        U: serde::Serialize,
    {
        unreachable!()
    }

    fn end(self) -> Result<T, E> {
        unreachable!()
    }
}
