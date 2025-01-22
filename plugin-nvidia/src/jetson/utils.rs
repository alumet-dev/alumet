use std::{fs::File, path::PathBuf};

use alumet::{metrics::TypedMetricId, resources::Resource, units::PrefixedUnit};

/// Detected INA sensor.
pub struct InaSensor {
    /// Path to the sysfs directory of the sensor.
    pub path: PathBuf,
    /// I2C id of the sensor.
    pub i2c_id: String,
    /// Channels available on this sensor.
    /// Each INA3221 has at least one channel.
    pub channels: Vec<InaChannel>,
}

/// Detected INA channel.
pub struct InaChannel {
    pub id: u32,
    pub label: String,
    pub metrics: Vec<InaRailMetric>,
    // Added in a second pass based on the Jetson documentation. (TODO: fill it)
    pub description: Option<String>,
}

/// Detected metric available in a channel.
pub struct InaRailMetric {
    pub path: PathBuf,
    pub unit: PrefixedUnit,
    pub name: String,
}

/// A channel metric that has been "opened" for reading.
pub struct OpenedInaMetric {
    /// Id of the metric registered in Alumet.
    /// The INA sensors provides integer values.
    pub metric_id: TypedMetricId<u64>,
    /// Id of the "resource" corresponding to the INA sensor.
    pub resource_id: Resource,
    /// The virtual file in the sysfs, opened for reading.
    pub file: File,
}

/// A channel that has been "opened" for reading.
pub struct OpenedInaChannel {
    pub label: String,
    pub description: String,
    pub metrics: Vec<OpenedInaMetric>,
}

/// A sensor that has been "opened" for reading.
pub struct OpenedInaSensor {
    pub i2c_id: String,
    pub channels: Vec<OpenedInaChannel>,
}

/// Measurement source that queries the embedded INA3221 sensor of a Jetson device.
pub struct JetsonInaSource {
    pub opened_sensors: Vec<OpenedInaSensor>,
}
