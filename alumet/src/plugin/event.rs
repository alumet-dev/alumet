//! Event system for inter-plugin communication.
//!
//! ## Example: detection of new processes
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
//! let event_bus = event::new_consumers(); 
//! watch_new_processes(|pid| {
//!     let process = ResourceConsumer::Process { pid };
//!     let event = event::NewResourceConsumers(vec![process]);
//!     event_bus.publish(event);
//! });
//! ```
//!
//! Event receiver:
//! ```no_run
//! use alumet::plugin::event;
//!
//! // Get notified on each event that gets sent to the bus.
//! let event_bus = event::new_consumers();
//! event_bus.subscribe(move |event| {
//!     let processes = event.0;
//!     for p in processes {
//!         todo!()
//!     }
//! });
//! ```

use std::{
    ops::Deref,
    sync::{Mutex, OnceLock},
};

use crate::resources::ResourceConsumer;

/// Trait for constraining event types.
pub trait Event: Clone {}

/// An event bus.
pub struct EventBus<E: Event> {
    /// The listeners, in a Mutex.
    ///
    /// We use a Mutex here, not a RwLock, because we don't want to impose a Sync
    /// bound on the listener functions.
    listeners: Mutex<Vec<Box<dyn Fn(E) + Send>>>,
}

impl<E: Event> Default for EventBus<E> {
    fn default() -> Self {
        Self {
            listeners: Mutex::new(Vec::with_capacity(4)),
        }
    }
}

impl<E: Event> EventBus<E> {
    /// Subscribes to the event bus.
    ///
    /// `listener` will be called on future events.
    pub fn subscribe<F: Fn(E) + Send + 'static>(&self, listener: F) {
        let mut listeners = self.listeners.lock().unwrap();
        listeners.push(Box::new(listener));
    }

    /// Publishes an event to the bus.
    ///
    /// All the `listeners` will be called with the event.
    pub fn publish(&self, event: E) {
        for listener in self.listeners.lock().unwrap().deref() {
            listener(event.clone());
        }
    }
}

// ====== Global events and buses ======

/// Contains all the global event buses.
#[derive(Default)]
struct EventBuses {
    new_consumers: EventBus<NewResourceConsumers>,
}

/// Global variable, initialized only once, containing the event buses.
static GLOBAL_EVENT_BUSES: OnceLock<EventBuses> = OnceLock::new();

/// Event occuring when new [resource consumers](ResourceConsumer) are detected
/// and should be measured.
#[derive(Clone)]
pub struct NewResourceConsumers(pub Vec<ResourceConsumer>);

impl Event for NewResourceConsumers {}

/// Returns the global event bus for the event [`NewResourceConsumers`].
pub fn new_consumers() -> &'static EventBus<NewResourceConsumers> {
    &GLOBAL_EVENT_BUSES.get_or_init(|| EventBuses::default()).new_consumers
}

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
        });

        bus.publish(TestEvent(1));
        assert_eq!(1, event_count.load(Ordering::SeqCst));
        bus.publish(TestEvent(10));
        assert_eq!(11, event_count.load(Ordering::SeqCst));
    }
}
