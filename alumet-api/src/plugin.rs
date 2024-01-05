use core::fmt;
use std::{
    collections::HashMap,
    error::Error,
    fmt::{Debug, Display},
    time::{Duration, SystemTime},
};

use crate::{
    config,
    metric::{MeasurementBuffer, MetricRegistry},
};

pub struct PluginInfo {
    pub name: String,
    pub version: String,
    // todo try to avoid boxing here?
    pub init: Box<dyn FnOnce(&mut config::ConfigTable) -> Result<Box<dyn Plugin>, PluginError>>,
}

/// Structure given to a plugin when it starts, and enables it to register metrics, sources and more.
/// The advantage of using a struct here is that it allows to provide new capabilities to plugins
/// without changing the plugin interface.
pub struct AlumetStart<'a> {
    pub metrics: &'a mut MetricRegistry,
    pub sources: &'a mut SourceRegistry,
    pub transforms: &'a mut TransformRegistry,
    pub outputs: &'a mut OutputRegistry,
}

/// The ALUMET plugin trait.
///
/// Plugins are a central part of ALUMET, because they produce, transform and export the measurements.
/// Please refer to the module documentation.
pub trait Plugin {
    /// The name of the plugin. It must be unique: two plugins cannot have the same name.
    fn name(&self) -> &str;

    /// The version of the plugin, for instance `"1.2.3"`. It should adhere to semantic versioning.
    fn version(&self) -> &str;

    /// Starts the plugin, allowing it to register metrics, sources and outputs.
    ///
    /// ## Plugin restart
    /// A plugin can be started and stopped multiple times, for instance when ALUMET switches from monitoring to profiling mode.
    /// [`Plugin::stop`] is guaranteed to be called between two calls of [`Plugin::start`].
    fn start(&mut self, alumet: &mut AlumetStart) -> Result<(), PluginError>;

    /// Stops the plugin.
    ///
    /// This method is called _after_ all the metrics, sources and outputs previously registered
    /// by [`Plugin::start`] have been stopped and unregistered.
    fn stop(&mut self) -> Result<(), PluginError>;
}

/// Produces measurements related to some metrics.
pub trait Source: Send {
    fn poll(&mut self, into: &mut MeasurementBuffer, time: SystemTime) -> Result<(), PollError>;
}

/// Exports measurements to an external entity, like a file or a database.
pub trait Output: Send {
    fn write(&mut self, measurements: &MeasurementBuffer) -> Result<(), WriteError>;
}

pub struct SourceRegistry {
    sources: HashMap<RegisteredSourceKey, Vec<Box<dyn Source>>>,
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
        SourceRegistry {
            sources: HashMap::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.sources.len()
    }

    pub fn register(&mut self, source: Box<dyn Source>, source_type: RegisteredSourceType, poll_interval: Duration) {
        let key = RegisteredSourceKey {
            poll_interval,
            source_type,
        };
        self.sources.entry(key).or_default().push(source);
    }

    pub fn grouped(self) -> HashMap<RegisteredSourceKey, Vec<Box<dyn Source>>> {
        self.sources
    }
}

pub struct OutputRegistry {
    pub outputs: Vec<Box<dyn Output>>,
}

impl OutputRegistry {
    pub fn new() -> OutputRegistry {
        OutputRegistry { outputs: Vec::new() }
    }

    pub fn len(&self) -> usize {
        self.outputs.len()
    }
}

pub struct TransformRegistry {
    // todo
}

// ====== Errors ======
pub struct PluginError(GenericError<PluginErrorKind>);
pub struct PollError(GenericError<PollErrorKind>);
pub struct WriteError(GenericError<WriteErrorKind>);

impl PluginError {
    pub fn new(kind: PluginErrorKind) -> PluginError {
        PluginError(GenericError {
            kind,
            cause: None,
            description: None,
        })
    }

    pub fn with_description(kind: PluginErrorKind, description: &str) -> PluginError {
        PluginError(GenericError {
            kind,
            cause: None,
            description: Some(description.to_owned()),
        })
    }

    pub fn with_cause<E: Error + 'static>(kind: PluginErrorKind, description: &str, cause: E) -> PluginError {
        PluginError(GenericError {
            kind,
            cause: Some(Box::new(cause)),
            description: Some(description.to_owned()),
        })
    }
}

#[derive(Debug)]
#[non_exhaustive]
pub enum PluginErrorKind {
    /// The plugin's configuration could not be parsed or contains invalid entries.
    InvalidConfiguration,
    /// The plugin requires a sensor that could not be found.
    /// For example, the plugin fetches information from an internal wattmeter, but the host does not have one.
    SensorNotFound,
    /// The plugin attempted an IO operation, but failed.
    IoFailure,
}

impl Display for PluginErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PluginErrorKind::InvalidConfiguration => todo!(),
            PluginErrorKind::SensorNotFound => todo!(),
            PluginErrorKind::IoFailure => todo!(),
        }
    }
}

impl PollError {
    pub fn new(kind: PollErrorKind) -> PollError {
        PollError(GenericError {
            kind,
            cause: None,
            description: None,
        })
    }

    pub fn with_description(kind: PollErrorKind, description: &str) -> PollError {
        PollError(GenericError {
            kind,
            cause: None,
            description: Some(description.to_owned()),
        })
    }

    pub fn with_source<E: Error + 'static>(kind: PollErrorKind, description: &str, source: E) -> PollError {
        PollError(GenericError {
            kind,
            cause: Some(Box::new(source)),
            description: Some(description.to_owned()),
        })
    }
}

#[derive(Debug)]
pub enum PollErrorKind {
    /// The source of the data could not be read.
    /// For instance, when a file contains the measurements to poll, but reading
    /// it fails, `poll()` returns an error of kind [`ReadFailed`].
    ReadFailed,
    /// The raw data could be read, but turning it into a proper measurement failed.
    /// For instance, when a file contains the measurements in some format, but reading
    /// it does not give the expected value, which causes the parsing to fail,
    /// `poll()` returns an error of kind [`ParsingFailed`].
    ParsingFailed,
}
impl Display for PollErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            PollErrorKind::ReadFailed => "read failed",
            PollErrorKind::ParsingFailed => "parsing failed",
        };
        f.write_str(s)
    }
}

#[derive(Debug)]
pub enum WriteErrorKind {
    /// The data could not be written properly.
    /// For instance, the data was in the process of being sent over the network,
    /// but the connection was lost.
    WriteFailed,
    /// The data could not be transformed into a form that is appropriate for writing.
    /// For instance, the measurements lack some metadata, which causes the formatting
    /// to fail.
    FormattingFailed,
}
impl Display for WriteErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let s = match self {
            WriteErrorKind::WriteFailed => "write failed",
            WriteErrorKind::FormattingFailed => "formatting failed",
        };
        f.write_str(s)
    }
}

#[derive(Debug)]
struct GenericError<K: Display + Debug> {
    kind: K,
    cause: Option<Box<dyn Error>>,
    description: Option<String>,
}

impl<K: Display + Debug> Error for GenericError<K> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.cause.as_deref()
    }
}

impl<K: Display + Debug> fmt::Display for GenericError<K> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.kind)?;
        if let Some(desc) = &self.description {
            write!(f, ": {desc}")?;
        }
        if let Some(err) = &self.cause {
            write!(f, "\nCaused by: {err}")?;
        }
        Ok(())
    }
}
impl Display for PollError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "failed to poll measurements: {}", self.0)
    }
}

// ====== FFI API for C ======
pub mod ffi {
    use super::SourceRegistry;
    use crate::config;
    use std::ffi::c_void;

    pub type InitFn = extern "C" fn(config: *const config::ConfigTable) -> *mut c_void;
    pub type StartFn = extern "C" fn(instance: *mut c_void);
    pub type StopFn = extern "C" fn(instance: *mut c_void);
    pub type DropFn = extern "C" fn(instance: *mut c_void);

    #[no_mangle]
    pub extern "C" fn metric_register(registry: &mut SourceRegistry) {
        todo!()
    }
}
