use std::time::{Duration, SystemTime};

use alumet::{
    measurement::{AttributeValue, MeasurementBuffer, MeasurementPoint, Timestamp, WrappedMeasurementValue},
    metrics::RawMetricId,
    resources::{Resource, ResourceConsumer},
};
use anyhow::Context;
use serde::{Deserialize, Serialize, ser::SerializeSeq};

/// A measurement buffer than can be serialized and deserialized. This type is similar to [`std::borrow::Cow`].
///
/// When serializing, use [`SerdeMeasurementBuffer::Borrowed`] to pass a reference to a buffer,
/// without copying it to the serializer.
///
/// When deserializing, you will get an owned buffer in [`SerdeMeasurementBuffer::Owned`].
#[derive(Debug)]
pub enum SerdeMeasurementBuffer<'a> {
    Borrowed(&'a MeasurementBuffer),
    Owned(MeasurementBuffer),
}

impl SerdeMeasurementBuffer<'_> {
    /// Returns a references to the underlying buffer.
    ///
    /// This method always work, but is typically used in the **serialization** process.
    pub fn borrowed(&self) -> &MeasurementBuffer {
        match self {
            SerdeMeasurementBuffer::Borrowed(m) => m,
            SerdeMeasurementBuffer::Owned(m) => m,
        }
    }

    /// Returns the owned buffer, if this is a [`Self::Owned`].
    ///
    /// This method is typically used in the **deserialization** process.
    ///
    /// # Panics
    /// Panics if the self value is a `Borrowed`.
    pub fn owned(self) -> MeasurementBuffer {
        match self {
            SerdeMeasurementBuffer::Borrowed(_) => panic!("this buffer is Borrowed, cannot return an owned value"),
            SerdeMeasurementBuffer::Owned(buf) => buf,
        }
    }
}

impl<'a> serde::Serialize for SerdeMeasurementBuffer<'a> {
    /// Serializes a measurement buffer.
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let buf = self.borrowed();
        let mut seq = serializer.serialize_seq(Some(buf.len()))?;
        for point in buf.iter() {
            let serializable = SerializableMeasurementPoint::from(point);
            seq.serialize_element(&serializable)?;
        }
        seq.end()
    }
}

// It's important that the 'de lifetime is NOT related to 'a, otherwise it would indicate
// that the deserialization result holds some reference to the input bytes.
impl<'de, 'a> serde::Deserialize<'de> for SerdeMeasurementBuffer<'a> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        /// A visitor that implements the "online" deserialization (element by element) of a SerializableMeasurementBuffer,
        /// converting each SerializableMeasurementPoint to a MeasurementPoint.
        struct BufVisitor;

        impl<'de> serde::de::Visitor<'de> for BufVisitor {
            type Value = MeasurementBuffer;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a sequence of measurement points")
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::SeqAccess<'de>,
            {
                let mut res = MeasurementBuffer::with_capacity(seq.size_hint().unwrap_or(0));
                while let Some(elem) = seq.next_element::<SerializableMeasurementPoint>()? {
                    // how to propagate errors in serde visitor?
                    match MeasurementPoint::try_from(elem) {
                        Ok(point) => res.push(point),
                        Err(err) => {
                            log::error!("Failed to convert SerializableMeasurementPoint to MeasurementPoint: {err:?}")
                        }
                    }
                }
                Ok(res)
            }
        }
        let inner = deserializer.deserialize_seq(BufVisitor)?;
        Ok(SerdeMeasurementBuffer::Owned(inner))
    }
}

#[derive(Serialize, Deserialize)]
struct SerializableMeasurementPoint<'a> {
    metric_id: u64,
    timestamp: UnixTimestamp,
    value: TypedValue<'a>,
    resource_kind: &'a str,
    resource_id: String,
    consumer_kind: &'a str,
    consumer_id: String,
    attributes: Vec<(&'a str, TypedValue<'a>)>,
}

impl<'a> From<&'a MeasurementPoint> for SerializableMeasurementPoint<'a> {
    fn from(point: &'a MeasurementPoint) -> Self {
        let (secs, nanos) = point.timestamp.to_unix_timestamp();
        let timestamp = UnixTimestamp { secs, nanos };
        let attributes = point.attributes().map(|(k, v)| (k, TypedValue::from(v))).collect();
        Self {
            metric_id: point.metric.as_u64(),
            timestamp,
            value: TypedValue::from(&point.value),
            resource_kind: point.resource.kind(),
            resource_id: point.resource.id_string().unwrap_or(String::from("")),
            consumer_kind: point.consumer.kind(),
            consumer_id: point.consumer.id_string().unwrap_or(String::from("")),
            attributes,
        }
    }
}

impl<'a> TryFrom<SerializableMeasurementPoint<'a>> for MeasurementPoint {
    type Error = anyhow::Error;

    fn try_from(point: SerializableMeasurementPoint<'a>) -> Result<Self, Self::Error> {
        let timestamp = SystemTime::UNIX_EPOCH
            .checked_add(Duration::new(point.timestamp.secs, point.timestamp.nanos))
            .context("invalid timestamp")?
            .into();
        let metric = RawMetricId::from_u64(point.metric_id);
        let resource = Resource::parse(point.resource_kind.to_owned(), point.resource_id)?;
        let consumer = ResourceConsumer::parse(point.consumer_kind.to_owned(), point.consumer_id)?;
        let value: WrappedMeasurementValue = point.value.into();
        let attributes = point
            .attributes
            .iter()
            .map(|(k, v)| (k.to_string(), AttributeValue::from(v)))
            .collect();
        Ok(MeasurementPoint::new_untyped(timestamp, metric, resource, consumer, value).with_attr_vec(attributes))
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub enum TypedValue<'a> {
    F64(f64),
    U64(u64),
    Bool(bool),
    Str(&'a str),
    ListU64(Vec<u64>), // TODO optimize
}

#[derive(Serialize, Deserialize)]
struct UnixTimestamp {
    secs: u64,
    nanos: u32,
}

impl<'a> From<&'a WrappedMeasurementValue> for TypedValue<'a> {
    fn from(value: &'a WrappedMeasurementValue) -> Self {
        match value {
            WrappedMeasurementValue::F64(v) => TypedValue::F64(*v),
            WrappedMeasurementValue::U64(v) => TypedValue::U64(*v),
        }
    }
}

impl<'a> From<TypedValue<'a>> for WrappedMeasurementValue {
    fn from(value: TypedValue<'a>) -> Self {
        match value {
            TypedValue::F64(v) => WrappedMeasurementValue::F64(v),
            TypedValue::U64(v) => WrappedMeasurementValue::U64(v),
            _ => unreachable!("MeasurementPoint values should never be of this type, got {value:?}"),
        }
    }
}

impl<'a> From<&'a AttributeValue> for TypedValue<'a> {
    fn from(value: &'a AttributeValue) -> Self {
        match value {
            AttributeValue::F64(v) => TypedValue::F64(*v),
            AttributeValue::U64(v) => TypedValue::U64(*v),
            AttributeValue::Bool(v) => TypedValue::Bool(*v),
            AttributeValue::Str(v) => TypedValue::Str(v),
            AttributeValue::String(v) => TypedValue::Str(v),
            AttributeValue::ListU64(items) => TypedValue::ListU64(items.to_owned()),
        }
    }
}

impl<'a> From<&'a TypedValue<'a>> for AttributeValue {
    fn from(value: &'a TypedValue<'a>) -> Self {
        match value {
            TypedValue::F64(v) => AttributeValue::F64(*v),
            TypedValue::U64(v) => AttributeValue::U64(*v),
            TypedValue::Bool(v) => AttributeValue::Bool(*v),
            TypedValue::Str(v) => AttributeValue::String(v.to_string()),
            TypedValue::ListU64(items) => AttributeValue::ListU64(items.to_owned()),
        }
    }
}

impl<'a> From<&'a Timestamp> for UnixTimestamp {
    fn from(value: &'a Timestamp) -> Self {
        let (secs, nanos) = value.to_unix_timestamp();
        Self { secs, nanos }
    }
}
