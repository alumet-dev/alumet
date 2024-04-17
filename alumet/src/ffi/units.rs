use crate::units::Unit;

use super::string::AString;

#[repr(u8)]
pub enum FfiUnit {
    /// Indicates a dimensionless value. This is suitable for counters.
    Unity,

    /// Standard unit of **time**.
    Second,

    /// Standard unit of **power**.
    Watt,

    /// Standard unit of **energy**.
    Joule,

    /// Electric tension (aka voltage)
    Volt,

    /// Intensity of an electric current
    Ampere,

    /// Frequency (1 Hz = 1/second)
    Hertz,

    /// Temperature in °C
    DegreeCelsius,

    /// Temperature in °F
    DegreeFahrenheit,

    /// Energy in Watt-hour (1 W⋅h = 3.6 kiloJoule = 3.6 × 10^3 Joules)
    WattHour,

    /// A custom unit
    Custom {
        /// The unique name of the unit, as specified by the UCUM.
        unique_name: AString,
        /// The display (print) name of the unit, as specified by the UCUM.
        display_name: AString,
    },
}

impl From<FfiUnit> for Unit {
    fn from(value: FfiUnit) -> Self {
        match value {
            FfiUnit::Unity => Unit::Unity,
            FfiUnit::Second => Unit::Second,
            FfiUnit::Watt => Unit::Watt,
            FfiUnit::Joule => Unit::Joule,
            FfiUnit::Volt => Unit::Volt,
            FfiUnit::Ampere => Unit::Ampere,
            FfiUnit::Hertz => Unit::Hertz,
            FfiUnit::DegreeCelsius => Unit::DegreeCelsius,
            FfiUnit::DegreeFahrenheit => Unit::DegreeFahrenheit,
            FfiUnit::WattHour => Unit::WattHour,
            FfiUnit::Custom {
                unique_name,
                display_name,
            } => Unit::Custom {
                unique_name: unique_name.into(),
                display_name: display_name.into(),
            },
        }
    }
}
