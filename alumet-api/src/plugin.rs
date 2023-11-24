use core::fmt;
use std::{error::Error, time::Duration, collections::HashMap};

use crate::{metric::{MeasurementBuffer, MetricRegistry}, config};

pub struct PluginInfo {
    pub name: String,
    pub version: String,
    pub init: fn(config: &mut config::ConfigTable) -> Result<Box<dyn Plugin>, PluginError>
}

pub trait Plugin {
    fn name(&self) -> &str;
    fn version(&self) -> &str;
    fn start(&mut self, metrics: &mut MetricRegistry, sources: &mut SourceRegistry, outputs: &mut OutputRegistry) -> Result<(), PluginError>;
    fn stop(&mut self) -> Result<(), PluginError>;
}

pub trait MetricSource: Send {
    fn poll(&mut self, into: &mut MeasurementBuffer) -> Result<(), MetricSourceError>;
}

pub trait MetricOutput: Send {
    fn write(&mut self, measurements: &MeasurementBuffer) -> Result<(), MetricOutputError>;
}

pub struct SourceRegistry {
    sources: HashMap<RegisteredSourceKey, Vec<Box<dyn MetricSource>>>
}

#[derive(PartialEq, Eq, Hash)]
pub struct RegisteredSourceKey {
    pub poll_interval: Duration,
    pub source_type: RegisteredSourceType,
}

#[derive(PartialEq, Eq, Hash)]
pub enum RegisteredSourceType {
    Normal,
    Blocking,
    Priority,
}

impl SourceRegistry {
    pub fn new() -> SourceRegistry {
        SourceRegistry { sources: HashMap::new() }
    }
    
    pub fn len(&self) -> usize {
        self.sources.len()
    }
    
    pub fn register(&mut self, source: Box<dyn MetricSource>, source_type: RegisteredSourceType, poll_interval: Duration) {
        self.sources
            .entry(RegisteredSourceKey { poll_interval, source_type })
            .or_default().push(source);
    }
    
    pub fn grouped(self) ->HashMap<RegisteredSourceKey, Vec<Box<dyn MetricSource>>> {
        self.sources
    }
}

pub struct OutputRegistry {
    pub outputs: Vec<Box<dyn MetricOutput>>
}

impl OutputRegistry {
    pub fn new() -> OutputRegistry {
        OutputRegistry { outputs: Vec::new() }
    }
    
    pub fn len(&self) -> usize {
        self.outputs.len()
    }
}

// ====== Errors ======

#[derive(Debug)]
pub enum PluginError {
    Io { description: String, source: std::io::Error },
    Config { description: String },
    Internal(),
}

impl Error for PluginError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            PluginError::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl fmt::Display for PluginError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "plugin initialization failed")
    }
}

#[derive(Debug)]
pub enum MetricSourceError {
    Io { description: String, source: std::io::Error },
    Internal()
}

impl Error for MetricSourceError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            MetricSourceError::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl fmt::Display for MetricSourceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "failed to poll measurements")
    }
}

#[derive(Debug)]
pub enum MetricOutputError {
    Io { description: String, source: std::io::Error },
    Internal()
}

impl Error for MetricOutputError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            MetricOutputError::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

impl fmt::Display for MetricOutputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "failed to write measurements")
    }
}

// ====== FFI API for C ======
pub mod ffi {
    use std::ffi::c_void;
    use crate::config;
    use super::SourceRegistry;
    
    pub type ExternPluginInitFn = extern fn(config: *const config::ConfigTable) -> *const c_void;

    #[no_mangle]
    pub extern fn metric_register(registry: &mut SourceRegistry) {
        todo!()
    }
}
