use std::fmt::{Debug, Display};

#[derive(PartialEq, Eq)]
pub enum Unit {
    /// Time
    Second,
    /// Power
    Watt,
    /// Energy
    Joule,
    /// Electric tension (aka voltage)
    Volt,
    /// Intensity of an electric current
    Ampere,
    /// Frequency (1 Hz = 1 something per second)
    Hertz,
    /// Temperature in °C
    DegreeCelsius,
    /// Temperature in °F
    DegreeFahrenheit,
    /// Energy in Kilowatt-hour (1 kW⋅h = 3.6 megajoule = 3.6 × 10^6 Joules)
    KiloWattHour,
    /// A custom unit.
    Custom { unique_name: String, display_name: String },
    /// Indicates a dimensionless value. This is suitable for counters.
    Unity,
}

impl Unit {
    /// Returns the case sensitive name of the unit, for use in transmission and storage.
    pub fn unique_name(&self) -> &str {
        match self {
            Unit::Second => "s",
            Unit::Watt => "W",
            Unit::Joule => "J",
            Unit::Volt => "V",
            Unit::Ampere => "A",
            Unit::Hertz => "Hz",
            Unit::DegreeCelsius => "Cel",
            Unit::DegreeFahrenheit => "[degF]",
            Unit::KiloWattHour => "kW.h",
            Unit::Custom { unique_name, .. } => unique_name,
            Unit::Unity => "1", // the official name of the "default unit", which means "no unit"
        }
    }
}

impl Debug for Unit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.unique_name())
    }
}

impl Display for Unit {
    /// Prints the unit, possibly in a nicer way than name.
    /// For instance, the standard name of `DegreeCelsius` is `Cel`, but
    /// its display name is `°C`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let display_name = match self {
            Unit::DegreeCelsius => "°C",
            Unit::DegreeFahrenheit => "°F",
            Unit::Unity => "", // dimensionless
            Unit::Custom { display_name, .. } => &display_name,
            _ => self.unique_name(), // the unique_name is nice enough to be displayed as it is
        };
        write!(f, "{display_name}")
    }
}
