//! Event system for inter-plugin communication.
//!
//! # Example: detection of new processes
//!
//! Event publisher (the detector):
//! ```no_run
//! use alumet::plugin::event;
//! use alumet::resources::ResourceConsumer;
//!
//! /// Internal notification when a new process is detected.
//! struct NewProcessNotif { pid: u32 }
//!
//! /// Calls `f` every time a new process is detected.
//! /// Replace this function by a detection mechanism of your choice.
//! fn watch_new_processes(f: impl Fn(NewProcessNotif) + Send + 'static) {
//!     todo!()
//! }
//!
//! // Send an event to the bus for each new process.
//! let event_bus = event::start_consumer_measurement();
//! watch_new_processes(|notif| {
//!     let process = ResourceConsumer::Process { pid: notif.pid };
//!     let event = event::StartConsumerMeasurement(vec![process]);
//!     event_bus.publish(event);
//! });
//! ```
//!
//! Event receiver:
//! ```no_run
//! use alumet::plugin::event;
//!
//! // Get notified on each event that gets sent to the bus.
//! let event_bus = event::start_consumer_measurement();
//! event_bus.subscribe(move |event| {
//!     let processes = event.0;
//!     for p in processes {
//!         todo!()
//!     }
//!     Ok(())
//! });
//! ```

use std::{
    ops::Deref,
    sync::{Mutex, OnceLock},
};

use crate::resources::{Resource, ResourceConsumer};

/// Trait for constraining event types.
pub trait Event: Clone {}

/// An event bus.
pub struct EventBus<E: Event> {
    /// The listeners, in a Mutex.
    ///
    /// We use a Mutex here, not a RwLock, because we don't want to impose a Sync
    /// bound on the listener functions.
    listeners: Mutex<Vec<Box<dyn Fn(E) -> anyhow::Result<()> + Send>>>,
}

impl<E: Event> Default for EventBus<E> {
    fn default() -> Self {
        Self {
            listeners: Mutex::new(Vec::with_capacity(4)),
        }
    }
}

impl<E: Event> EventBus<E> {
    /// Subscribe to the event bus.
    ///
    /// `listener` will be called on future events.
    ///
    /// # Performance caveats
    ///
    /// Event listeners are called in same thread as the publisher, one after the other.
    /// Therefore, **each listener should only perform a minimal amount of work**.
    /// To execute large tasks in response to an event, consider sending a message
    /// to another thread (or async future) through a [`channel`](tokio::sync::mpsc::channel).
    pub fn subscribe<F: Fn(E) -> anyhow::Result<()> + Send + 'static>(&self, listener: F) {
        let mut listeners = self.listeners.lock().unwrap();
        listeners.push(Box::new(listener));
    }

    /// Publish an event to the bus.
    ///
    /// All the `listeners` will be called with the event.
    pub fn publish(&self, event: E) {
        for listener in self.listeners.lock().unwrap().deref() {
            if let Err(e) = listener(event.clone()) {
                log::error!("Error in event handler: {e:?}")
            }
        }
    }

    /// If someone is listening for an event, create the event with the provided closure
    /// and publish it to the bus.
    ///
    /// All the `listeners` will be called with the event.
    pub fn publish_lazy(&self, create_event: impl FnOnce() -> E) {
        let listeners = self.listeners.lock().unwrap();
        match &listeners[..] {
            [] => (),
            [listener] => {
                if let Err(e) = listener(create_event()) {
                    log::error!("Error in event handler: {e:?}")
                }
            }
            listeners => {
                let event = create_event();
                for listener in listeners {
                    if let Err(e) = listener(event.clone()) {
                        log::error!("Error in event handler: {e:?}")
                    }
                }
            }
        }
    }
}

// ====== Global events and buses ======

/// Contains all the global event buses.
#[derive(Default)]
struct EventBuses {
    start_consumer_measurement: EventBus<StartConsumerMeasurement>,
    start_resource_measurement: EventBus<StartResourceMeasurement>,
    end_consumer_measurement: EventBus<EndConsumerMeasurement>,
}

/// Global variable, initialized only once, containing the event buses.
static GLOBAL_EVENT_BUSES: OnceLock<EventBuses> = OnceLock::new();

/// Returns the global event bus for the event [`StartConsumerMeasurement`].
pub fn start_consumer_measurement() -> &'static EventBus<StartConsumerMeasurement> {
    &GLOBAL_EVENT_BUSES
        .get_or_init(EventBuses::default)
        .start_consumer_measurement
}

/// Returns the global event bus for the event [`StartResourceMeasurement`].
pub fn start_resource_measurement() -> &'static EventBus<StartResourceMeasurement> {
    &GLOBAL_EVENT_BUSES
        .get_or_init(EventBuses::default)
        .start_resource_measurement
}

/// Returns the global event bus for the event [`EndConsumerMeasurement`].
pub fn end_consumer_measurement() -> &'static EventBus<EndConsumerMeasurement> {
    &GLOBAL_EVENT_BUSES
        .get_or_init(EventBuses::default)
        .end_consumer_measurement
}

/// Event occurring when new [resource consumers](ResourceConsumer) are detected
/// and should be measured.
#[derive(Clone)]
pub struct StartConsumerMeasurement(pub Vec<ResourceConsumer>);

/// Event occurring when new [resources](Resource) are detected
/// and should be measured.
#[derive(Clone)]
pub struct StartResourceMeasurement(pub Vec<Resource>);

/// Event occurring when measurements should be performed at the end of the consumer experiment.
#[derive(Clone)]
pub struct EndConsumerMeasurement;

impl Event for StartConsumerMeasurement {}
impl Event for StartResourceMeasurement {}
impl Event for EndConsumerMeasurement {}

#[cfg(test)]
mod tests {
    use std::sync::{
        atomic::{AtomicU32, Ordering},
        Arc,
    };

    use super::{Event, EventBus};

    #[derive(Clone)]
    struct TestEvent(u32);

    impl Event for TestEvent {}

    #[test]
    fn test() {
        let bus: EventBus<TestEvent> = EventBus::default();

        let event_count = Arc::new(AtomicU32::new(0));
        let cloned_count = event_count.clone();

        bus.publish(TestEvent(123));
        assert_eq!(
            0,
            event_count.load(Ordering::SeqCst),
            "count should remain 0 because there's no listener yet"
        );

        bus.subscribe(move |event| {
            cloned_count.fetch_add(event.0, Ordering::SeqCst);
            Ok(())
        });

        bus.publish(TestEvent(1));
        assert_eq!(1, event_count.load(Ordering::SeqCst));
        bus.publish(TestEvent(10));
        assert_eq!(11, event_count.load(Ordering::SeqCst));
    }
}
