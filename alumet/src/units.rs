//! Definition of measurement units.
//!

use std::{
    collections::HashMap,
    error::Error,
    fmt::{self, Debug, Display},
    sync::OnceLock,
};

/// A unit of measurement.
///
/// Some common units of the SI are provided as plain enum variants, such as `Unit::Second`.
#[derive(PartialEq, Eq)]
#[repr(u8)]
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
    Custom(CustomUnitId),
    // We store the unit's id and put its fields in a registry, because
    // Strings are not repr(C)-compatible.
}

/// A base unit and a scale.
///
/// # Example
/// ```
/// let milliA = ScaledUnit::milli(Unit::Ampere);
/// let nanoSec = ScaledUnit::nano(Unit::Second);
/// ```
pub struct ScaledUnit {
    pub base_unit: Unit,
    pub scale: f64,
}

/// Id of a custom unit.
///
/// Custom units can be registered by plugins using [`AlumetStart::create_unit`](crate::plugin::AlumetStart::create_unit).
#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
#[repr(C)]
pub struct CustomUnitId(pub(crate) u32);

#[derive(Debug)]
pub struct CustomUnit {
    pub unique_name: String,
    pub display_name: String,
    pub debug_name: String,
}

pub struct CustomUnitRegistry {
    pub(crate) units_by_id: HashMap<CustomUnitId, CustomUnit>,
    pub(crate) units_by_name: HashMap<String, CustomUnitId>,
}

/// Global registry of custom units.
pub(crate) static GLOBAL_CUSTOM_UNITS: OnceLock<CustomUnitRegistry> = OnceLock::new();

impl Unit {
    pub fn unique_name(&self) -> &str {
        match self {
            Unit::Custom(id) => {
                if let Some(unit) = GLOBAL_CUSTOM_UNITS.get().and_then(|r| r.units_by_id.get(id)) {
                    &unit.unique_name
                } else {
                    "invalid?!"
                }
            }
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
        }
    }

    fn display_name(&self) -> &str {
        match self {
            Unit::Custom(id) => {
                if let Some(unit) = GLOBAL_CUSTOM_UNITS.get().and_then(|r| r.units_by_id.get(id)) {
                    &unit.display_name
                } else {
                    "invalid?!"
                }
            }
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
        }
    }

    fn scaled(self, scale: f64) -> ScaledUnit {
        ScaledUnit { base_unit: self, scale }
    }
}

impl Debug for Unit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Custom(id) => {
                if let Some(unit) = CustomUnitRegistry::global().with_id(*id) {
                    let debug_name = &unit.debug_name;
                    write!(f, "Custom(id {}: {})", id.0, debug_name)
                } else {
                    write!(f, "Custom(invalid id {})", id.0)
                }
            }
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
        }
    }
}
impl Display for Unit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.display_name())
    }
}

impl ScaledUnit {
    // scale down

    pub fn milli(unit: Unit) -> ScaledUnit {
        unit.scaled(1e-3)
    }

    pub fn micro(unit: Unit) -> ScaledUnit {
        unit.scaled(1e-6)
    }

    pub fn nano(unit: Unit) -> ScaledUnit {
        unit.scaled(1e-9)
    }

    // scale up

    pub fn kilo(unit: Unit) -> ScaledUnit {
        unit.scaled(1e+3)
    }

    pub fn mega(unit: Unit) -> ScaledUnit {
        unit.scaled(1e+6)
    }

    pub fn giga(unit: Unit) -> ScaledUnit {
        unit.scaled(1e+9)
    }
}

impl CustomUnitRegistry {
    pub(crate) fn new() -> Self {
        Self {
            units_by_id: HashMap::new(),
            units_by_name: HashMap::new(),
        }
    }

    pub(crate) fn global() -> &'static CustomUnitRegistry {
        GLOBAL_CUSTOM_UNITS
            .get()
            .expect("The CustomUnitRegistry must be initialized before use")
    }

    pub(crate) fn init_global(registry: CustomUnitRegistry) {
        GLOBAL_CUSTOM_UNITS
            .set(registry)
            .unwrap_or_else(|_| panic!("The CustomUnitRegistry can be initialized only once"));
    }

    pub fn len(&self) -> usize {
        self.units_by_id.len()
    }

    pub(crate) fn with_id(&self, id: CustomUnitId) -> Option<&CustomUnit> {
        self.units_by_id.get(&id)
    }

    pub(crate) fn register(&mut self, unit: CustomUnit) -> Result<CustomUnitId, UnitCreationError> {
        let id = CustomUnitId(u32::try_from(self.len()).unwrap());
        let name = unit.unique_name.clone();
        self.units_by_id.insert(id, unit);
        self.units_by_name.insert(name.to_owned(), id);
        // TODO check for duplicates
        Ok(id)
    }
}

// ====== Errors ======
#[derive(Debug)]
pub struct UnitCreationError {
    pub key: String,
}

impl UnitCreationError {
    pub fn new(unit_name: String) -> UnitCreationError {
        UnitCreationError { key: unit_name }
    }
}

impl Error for UnitCreationError {}

impl fmt::Display for UnitCreationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "This unit has already been registered: {}", self.key)
    }
}
