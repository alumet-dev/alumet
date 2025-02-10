use crate::pipeline::{
    elements::source::{
        self,
        builder::{AutonomousSourceBuilder, ManagedSourceBuilder, SendSourceBuilder},
        CreateManyMessage,
    },
    naming::SourceName,
    trigger, Source,
};

use super::{error::ControlError, ControlMessage, ScopedControlHandle};

/// A buffer that allows to create multiple sources with only one control message.
///
/// # Example
/// ```no_run
/// use alumet::pipeline::control::{ScopedControlHandle, SourceCreationBuffer};
/// use alumet::pipeline::Source;
///
/// // Assume that we have:
/// // - a `control_handle: ScopedControlHandle`.
/// // - a function `create_source()` that returns a `Box<dyn Source>`
///
/// # let control_handle: ScopedControlHandle = todo!();
/// # fn create_source() -> Box<dyn Source> { todo!() }
/// #
/// // create the buffer
/// let mut buf: SourceCreationBuffer<'_> = control_handle.source_buffer();
///
/// // add multiple sources, here 10
/// for i in 1..=10 {
///     let source = create_source();
///     let trigger = todo!();
///     buf.add_source(&format!("source-{i}"), source, trigger);
/// }
///
/// // register all the sources at once
/// buf.flush().expect("failed to create sources");
/// ```
///
/// # Flushing
/// The buffer is automatically flushed on drop, but **you should call flush**
/// in order to handle the errors. The drop implementation silently ignores flushing errors.
pub struct SourceCreationBuffer<'a> {
    pub(super) handle: &'a mut ScopedControlHandle,
    pub(super) buffer: Vec<(SourceName, SendSourceBuilder)>,
}

impl SourceCreationBuffer<'_> {
    /// Flushes the buffer: creates all the sources.
    ///
    /// `flush` sends a [`CreateMany`](source::ControlMessage::CreateMany) message that, when processed,
    ///  will create all the sources in this buffer.
    pub fn flush(&mut self) -> Result<(), ControlError> {
        self.handle
            .inner
            .try_send(ControlMessage::Source(source::ControlMessage::CreateMany(
                CreateManyMessage {
                    builders: std::mem::take(&mut self.buffer),
                },
            )))
            .map_err(|e| e.into())
    }

    /// Adds a managed source to the buffer.
    pub fn add_source(&mut self, name: &str, source: Box<dyn Source>, trigger: trigger::TriggerSpec) {
        let builder = self.handle.managed_source_builder(trigger, source);
        let name = SourceName::new(self.handle.plugin.0.clone(), name.to_owned());
        self.add_source_builder(name, builder)
    }

    /// Adds a source to the buffer, with an explicit builder.
    pub fn add_source_builder<F: ManagedSourceBuilder + Send + 'static>(&mut self, name: SourceName, builder: F) {
        let builder = SendSourceBuilder::Managed(Box::new(builder));
        self.buffer.push((name, builder));
    }

    /// Adds an autonomous source builder to the buffer.
    pub fn add_autonomous_source_builder<F: AutonomousSourceBuilder + Send + 'static>(
        &mut self,
        name: SourceName,
        builder: F,
    ) {
        let builder = SendSourceBuilder::Autonomous(Box::new(builder));
        self.buffer.push((name, builder));
    }
}

impl Drop for SourceCreationBuffer<'_> {
    fn drop(&mut self) {
        // prevent forgotten flushes
        if !self.buffer.is_empty() {
            let _ = self.flush(); // ignore errors
        }
    }
}
