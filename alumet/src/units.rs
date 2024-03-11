use std::{
    collections::HashMap,
    fmt::{Debug, Display},
    sync::OnceLock,
};

pub(crate) static GLOBAL_CUSTOM_UNITS: OnceLock<CustomUnitRegistry> = OnceLock::new();

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

impl Unit {
    pub fn unique_name(&self) -> &str {
        match self {
            Unit::Custom(id) => {
                if let Some(unit) = GLOBAL_CUSTOM_UNITS.get().and_then(|r| r.units_by_id.get(id)) {
                    &unit.unique_name
                } else {
                    "invalid?!"
                }
            },
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
            },
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
}

impl Debug for Unit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Custom(id) => {
                if let Some(unit) = GLOBAL_CUSTOM_UNITS.get().and_then(|r| r.units_by_id.get(id)) {
                    let debug_name = &unit.debug_name;
                    write!(f, "Custom(id {}: {})", id.0, debug_name)
                } else {
                    write!(f, "Custom(invalid id {})", id.0)
                }
            },
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

#[derive(Debug, PartialEq, Eq, Hash, Clone, Copy)]
#[repr(C)]
pub struct CustomUnitId(pub(crate) u32);

#[derive(Debug)]
pub(crate) struct CustomUnit {
    unique_name: String,
    display_name: String,
    debug_name: String,
}

impl CustomUnit {
    fn new_simple(unique_name: &str, debug_name: &str) -> Self {
        Self {
            unique_name: unique_name.to_owned(),
            display_name: unique_name.to_owned(),
            debug_name: debug_name.to_owned(),
        }
    }

    fn new(unique_name: &str, display_name: &str, debug_name: &str) -> Self {
        Self {
            unique_name: unique_name.to_owned(),
            display_name: display_name.to_owned(),
            debug_name: debug_name.to_owned(),
        }
    }
}

pub(crate) struct CustomUnitRegistry {
    pub(crate) units_by_id: HashMap<CustomUnitId, CustomUnit>,
    pub(crate) units_by_name: HashMap<String, CustomUnitId>,
}

impl CustomUnitRegistry {
    fn new() -> Self {
        Self {
            units_by_id: HashMap::new(),
            units_by_name: HashMap::new(),
        }
    }

    pub fn global() -> &'static CustomUnitRegistry {
        GLOBAL_CUSTOM_UNITS
            .get()
            .expect("The CustomUnitRegistry must be initialized before use")
    }

    pub(crate) fn init_global() {
        GLOBAL_CUSTOM_UNITS
            .set(CustomUnitRegistry::new())
            .unwrap_or_else(|_| panic!("The CustomUnitRegistry can be initialized only once"));
    }

    pub fn len(&self) -> usize {
        self.units_by_id.len()
    }

    pub fn create_unit(&mut self, unique_name: &str, display_name: &str, debug_name: &str) -> CustomUnitId {
        let id = CustomUnitId(u32::try_from(self.len()).unwrap());
        let unit = CustomUnit {
            unique_name: unique_name.to_owned(),
            display_name: display_name.to_owned(),
            debug_name: debug_name.to_owned(),
        };
        self.units_by_id.insert(id, unit);
        self.units_by_name.insert(unique_name.to_owned(), id);
        id
    }
}
