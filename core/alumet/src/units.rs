//! Definition of measurement units.

use anyhow::anyhow;
use std::{
    fmt::{self, Debug, Display},
    str::FromStr,
};

/// A unit of measurement.
///
/// Some common units of the SI are provided as plain enum variants, such as `Unit::Second`.
/// Use [`PrefixedUnit`] to create a standard multiple of a unit.
///
/// # Example
/// ```
/// use alumet::units::{Unit, PrefixedUnit};
///
/// let seconds = Unit::Second;
/// let kilobytes = PrefixedUnit::kilo(Unit::Byte);
/// ```
#[derive(PartialEq, Eq, Clone, Debug)]
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

    /// Amount of information (1 byte = 8 bits).
    Byte,

    /// Percent, between 0% and 100%. (note: actual value is between 0 and 100 - eg: 5% would be 5, not 0.05)
    Percent,

    /// A custom unit
    Custom {
        /// The unique name (case sensitive) of the unit, as specified by the UCUM.
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
#[derive(Debug, Clone, PartialEq, Eq)]
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
    /// See <https://ucum.org/ucum#section-Base-Units> and <https://ucum.org/ucum#si>
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
            Unit::Byte => "By",
            Unit::Percent => "%",
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
            Unit::Byte => "B",
            Unit::Percent => "%",
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
            "By" => Unit::Byte,
            "%" => Unit::Percent,
            _ => return Err(anyhow!("Unknown or non standard Unit {s}")),
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

impl FromStr for PrefixedUnit {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // List of known prefixes, sorted from longest to shortest to avoid ambiguity.
        // Each entry is a prefix_str
        let prefixes = [
            "giga", "mega", "kilo", "milli", "micro", "nano", "G", "M", "k", "m", "μ", "n", "",
        ];

        // Try to find a valid prefix
        for prefix_str in prefixes {
            if s.starts_with(prefix_str) {
                let unit_str = &s[prefix_str.len()..];
                let prefix = UnitPrefix::from_str(prefix_str)?;
                let base_unit = Unit::from_str(unit_str)?;
                return Ok(PrefixedUnit { base_unit, prefix });
            }
        }

        Err(anyhow!("Unknown prefix or invalid unit in '{}'", s))
    }
}

impl UnitPrefix {
    /// Returns the unique name of the unit, as specified by the Unified Code for Units of Measure (UCUM).
    ///
    /// See <https://ucum.org/ucum#section-Prefixes>
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
    /// See <https://ucum.org/ucum#section-Prefixes>
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
            "nano" | "n" => UnitPrefix::Nano,
            "micro" | "μ" => UnitPrefix::Micro,
            "milli" | "m" => UnitPrefix::Milli,
            "" => UnitPrefix::Plain,
            "kilo" | "k" => UnitPrefix::Kilo,
            "mega" | "M" => UnitPrefix::Mega,
            "giga" | "G" => UnitPrefix::Giga,
            _ => return Err(anyhow!("Unknown prefix")),
        };
        Ok(res)
    }
}

#[cfg(test)]
mod tests {
    use super::{PrefixedUnit, Unit, UnitPrefix};

    #[test]
    fn unit_serde() {
        fn parse_self(u: Unit) -> Unit {
            let name = u.unique_name();
            name.parse()
                .unwrap_or_else(|_| panic!("failed to parse {u:?} unique name {name:?}"))
        }
        assert_eq!(parse_self(Unit::Unity), Unit::Unity);
        assert_eq!(parse_self(Unit::Second), Unit::Second);
        assert_eq!(parse_self(Unit::Watt), Unit::Watt);
        assert_eq!(parse_self(Unit::Joule), Unit::Joule);
        assert_eq!(parse_self(Unit::Volt), Unit::Volt);
        assert_eq!(parse_self(Unit::Ampere), Unit::Ampere);
        assert_eq!(parse_self(Unit::Hertz), Unit::Hertz);
        assert_eq!(parse_self(Unit::DegreeCelsius), Unit::DegreeCelsius);
        assert_eq!(parse_self(Unit::DegreeFahrenheit), Unit::DegreeFahrenheit);
        assert_eq!(parse_self(Unit::WattHour), Unit::WattHour);
        assert_eq!(parse_self(Unit::Byte), Unit::Byte);
        assert_eq!(parse_self(Unit::Percent), Unit::Percent);
    }

    #[test]
    fn prefix_serde() {
        fn parse_self(p: UnitPrefix) -> UnitPrefix {
            let name = p.unique_name();
            name.parse()
                .unwrap_or_else(|_| panic!("failed to parse {p:?} unique name {name:?}"))
        }
        assert_eq!(parse_self(UnitPrefix::Nano), UnitPrefix::Nano);
        assert_eq!(parse_self(UnitPrefix::Micro), UnitPrefix::Micro);
        assert_eq!(parse_self(UnitPrefix::Milli), UnitPrefix::Milli);
        assert_eq!(parse_self(UnitPrefix::Plain), UnitPrefix::Plain);
        assert_eq!(parse_self(UnitPrefix::Kilo), UnitPrefix::Kilo);
        assert_eq!(parse_self(UnitPrefix::Mega), UnitPrefix::Mega);
        assert_eq!(parse_self(UnitPrefix::Giga), UnitPrefix::Giga);
    }

    #[test]
    fn prefixed_unit_serde() {
        fn parse_self(s: &str, expected_unit: Unit, expected_prefix: UnitPrefix) {
            let parsed = s
                .parse::<PrefixedUnit>()
                .unwrap_or_else(|_| panic!("failed to parse '{s}' as PrefixedUnit"));
            assert_eq!(parsed.base_unit, expected_unit);
            assert_eq!(parsed.prefix, expected_prefix);
        }
        // Valid inputs
        parse_self("kW", Unit::Watt, UnitPrefix::Kilo);
        parse_self("mA", Unit::Ampere, UnitPrefix::Milli);
        parse_self("μs", Unit::Second, UnitPrefix::Micro);
        parse_self("W", Unit::Watt, UnitPrefix::Plain);
        // Invalid inputs
        assert!("kX".parse::<PrefixedUnit>().is_err()); // unknown units
        assert!("XW".parse::<PrefixedUnit>().is_err()); // unknown units
        assert!("".parse::<PrefixedUnit>().is_err()); // malformed strings
        assert!("k".parse::<PrefixedUnit>().is_err()); // malformed strings
        assert!("KW".parse::<PrefixedUnit>().is_err()); // mixed case
        assert!("dW".parse::<PrefixedUnit>().is_err()); // non-standard prefixes
        assert!(" kW".parse::<PrefixedUnit>().is_err()); // whitespace
    }
}
