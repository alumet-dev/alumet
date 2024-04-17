//! Definition of measurement units.

use std::{
    fmt::{self, Debug, Display},
    str::FromStr,
};
use anyhow::anyhow;

/// A unit of measurement.
///
/// Some common units of the SI are provided as plain enum variants, such as `Unit::Second`.
#[derive(PartialEq, Eq, Clone)]
pub enum Unit {
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
        unique_name: String,
        /// The display (print) name of the unit, as specified by the UCUM.
        display_name: String,
    },
}

/// A base unit and a scale.
///
/// # Example
/// ```
/// use alumet::units::{Unit, PrefixedUnit};
///
/// let milliA = PrefixedUnit::milli(Unit::Ampere);
/// let nanoSec = PrefixedUnit::nano(Unit::Second);
/// ```
#[derive(Debug, Clone)]
pub struct PrefixedUnit {
    pub base_unit: Unit,
    pub prefix: UnitPrefix,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnitPrefix {
    Nano,
    Micro,
    Milli,
    Plain,
    Kilo,
    Mega,
    Giga,
}

impl Unit {
    /// Returns the unique name of the unit, as specified by the Unified Code for Units of Measure (UCUM).
    ///
    /// See https://ucum.org/ucum#section-Base-Units and https://ucum.org/ucum#si
    pub fn unique_name(&self) -> &str {
        match self {
            Unit::Unity => "1",
            Unit::Second => "s",
            Unit::Watt => "W",
            Unit::Joule => "J",
            Unit::Volt => "V",
            Unit::Ampere => "A",
            Unit::Hertz => "Hz",
            Unit::DegreeCelsius => "Cel",
            Unit::DegreeFahrenheit => "[degF]",
            Unit::WattHour => "W.h",
            Unit::Custom {
                unique_name,
                display_name: _,
            } => unique_name,
        }
    }

    /// Returns the name to use when displaying (aka printing) the unit, as specified by the Unified Code for Units of Measure (UCUM).
    ///
    /// See https://ucum.org/ucum#section-Base-Units and https://ucum.org/ucum#si
    fn display_name(&self) -> &str {
        match self {
            Unit::Unity => "",
            Unit::Second => "s",
            Unit::Watt => "W",
            Unit::Joule => "J",
            Unit::Volt => "V",
            Unit::Ampere => "A",
            Unit::Hertz => "Hz",
            Unit::DegreeCelsius => "°C",
            Unit::DegreeFahrenheit => "°F",
            Unit::WattHour => "Wh",
            Unit::Custom {
                unique_name: _,
                display_name,
            } => display_name,
        }
    }

    fn with_prefix(self, scale: UnitPrefix) -> PrefixedUnit {
        PrefixedUnit {
            base_unit: self,
            prefix: scale,
        }
    }
}

impl Debug for Unit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unity => write!(f, "Unity"),
            Self::Second => write!(f, "Second"),
            Self::Watt => write!(f, "Watt"),
            Self::Joule => write!(f, "Joule"),
            Self::Volt => write!(f, "Volt"),
            Self::Ampere => write!(f, "Ampere"),
            Self::Hertz => write!(f, "Hertz"),
            Self::DegreeCelsius => write!(f, "DegreeCelsius"),
            Self::DegreeFahrenheit => write!(f, "DegreeFahrenheit"),
            Self::WattHour => write!(f, "WattHour"),
            Self::Custom {
                unique_name,
                display_name: _,
            } => write!(f, "Custom({})", unique_name),
        }
    }
}
impl Display for Unit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.display_name())
    }
}

impl FromStr for Unit {
    // TODO more precise error type
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let res = match s {
            "1" => Unit::Unity,
            "s" => Unit::Second,
            "W" => Unit::Watt,
            "J" => Unit::Joule,
            "V" => Unit::Volt,
            "A" => Unit::Ampere,
            "Hz" => Unit::Hertz,
            "Cel" => Unit::DegreeCelsius,
            "[degF]" => Unit::DegreeFahrenheit,
            "W.h" => Unit::WattHour,
            _ => return Err(anyhow!("Unknown or non standard Unit")),
        };
        Ok(res)
    }
}

impl PrefixedUnit {
    // scale down

    pub fn milli(unit: Unit) -> PrefixedUnit {
        unit.with_prefix(UnitPrefix::Milli)
    }

    pub fn micro(unit: Unit) -> PrefixedUnit {
        unit.with_prefix(UnitPrefix::Micro)
    }

    pub fn nano(unit: Unit) -> PrefixedUnit {
        unit.with_prefix(UnitPrefix::Nano)
    }

    // scale up

    pub fn kilo(unit: Unit) -> PrefixedUnit {
        unit.with_prefix(UnitPrefix::Kilo)
    }

    pub fn mega(unit: Unit) -> PrefixedUnit {
        unit.with_prefix(UnitPrefix::Mega)
    }

    pub fn giga(unit: Unit) -> PrefixedUnit {
        unit.with_prefix(UnitPrefix::Giga)
    }

    // methods
    pub fn unique_name(&self) -> String {
        let prefix = match self.prefix {
            UnitPrefix::Nano => "nano",
            UnitPrefix::Micro => "micro",
            UnitPrefix::Milli => "milli",
            UnitPrefix::Plain => "",
            UnitPrefix::Kilo => "kilo",
            UnitPrefix::Mega => "mega",
            UnitPrefix::Giga => "giga",
        };
        format!("{prefix}{}", self.base_unit.unique_name())
    }

    pub fn display_name(&self) -> String {
        format!("{self}")
    }
}

impl From<Unit> for PrefixedUnit {
    fn from(value: Unit) -> Self {
        value.with_prefix(UnitPrefix::Plain)
    }
}

impl Display for PrefixedUnit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.prefix, self.base_unit)
    }
}

impl UnitPrefix {
    /// Returns the unique name of the unit, as specified by the Unified Code for Units of Measure (UCUM).
    ///
    /// See https://ucum.org/ucum#section-Prefixes
    pub fn unique_name(&self) -> &str {
        match self {
            UnitPrefix::Nano => "nano",
            UnitPrefix::Micro => "micro",
            UnitPrefix::Milli => "milli",
            UnitPrefix::Plain => "",
            UnitPrefix::Kilo => "kilo",
            UnitPrefix::Mega => "mega",
            UnitPrefix::Giga => "giga",
        }
    }

    /// Returns the name to use when displaying (aka printing) the prefix, as specified by the Unified Code for Units of Measure (UCUM).
    ///
    /// See https://ucum.org/ucum#section-Prefixes
    pub fn display_name(&self) -> &str {
        match self {
            UnitPrefix::Nano => "n",
            UnitPrefix::Micro => "μ",
            UnitPrefix::Milli => "m",
            UnitPrefix::Plain => "",
            UnitPrefix::Kilo => "k",
            UnitPrefix::Mega => "M",
            UnitPrefix::Giga => "G",
        }
    }
}

impl Display for UnitPrefix {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.display_name())
    }
}

impl FromStr for UnitPrefix {
    // TODO more precise error type
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let res = match s {
            "n" => UnitPrefix::Nano,
            "μ" => UnitPrefix::Micro,
            "m" => UnitPrefix::Milli,
            "" => UnitPrefix::Plain,
            "k" => UnitPrefix::Kilo,
            "M" => UnitPrefix::Mega,
            "G" => UnitPrefix::Giga,
            _ => return Err(anyhow!("Unknown prefix")),
        };
        Ok(res)
    }
}
