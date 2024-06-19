//! A thread-safe versioned value with mutable access.

use std::{
    future::Future,
    ops,
    sync::{Arc, Mutex, MutexGuard},
};

use tokio::sync::{futures::Notified, Notify};

use crate::pipeline::Output;

pub struct Versioned<T> {
    /// Thread-safe shared state.
    shared: Arc<Shared<T>>,
    /// The last version that has been seen by this Versioned.
    local_version: Version,
}

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
struct Version(u32);

struct Shared<T> {
    /// The actual state.
    state: Mutex<State<T>>,
    /// Notify to wake up the waiters.
    notify: Notify,
}

/// Holds the content of a [`Versioned`].
struct State<T> {
    /// The value of the Versioned.
    value: T,
    /// The latest version.
    ///
    /// A Versioned instance is up to date if and only if its `local_version`
    /// is equal to the latest version.
    version: Version,
}

impl Version {
    fn initial() -> Self {
        Self(1)
    }

    fn increment(self) -> Version {
        Version(self.0.wrapping_add(1))
    }

    fn decrement(self) -> Version {
        Version(self.0.wrapping_sub(1))
    }
}

impl<T> Versioned<T> {
    pub fn new(initial_value: T) -> Self {
        Self::new_with_notified(|_| initial_value)
    }

    pub fn new_with_notified<F: FnOnce(Notified<'_>) -> T>(f: F) -> Self {
        let notify = Notify::new();
        Self {
            shared: Arc::new(Shared {
                state: Mutex::new(State {
                    value: f(notify.notified()),
                    version: Version::initial(),
                }),
                notify,
            }),
            local_version: Version::initial(),
        }
    }

    pub fn clone_unseen(&self) -> Self {
        let state = self.shared.state.lock().unwrap();
        let last_version = state.version;
        Self {
            shared: self.shared.clone(),
            local_version: last_version.decrement(),
        }
    }

    /// Modifies the versioned value and increments the version.
    ///
    /// Also updates the local version.
    pub fn update<F: FnOnce(&mut T)>(&mut self, f: F) {
        let new_version = {
            let mut state = self.shared.state.lock().unwrap();
            f(&mut state.value);

            let new_version = state.version.increment();
            state.version = new_version;
            new_version
            // unlock the mutex
        };
        self.local_version = new_version; // local is up to date
        self.shared.notify.notify_waiters();
    }

    /// Replaces the versioned value and increments the version.
    ///
    /// Also updates the local version.
    pub fn set(&mut self, value: T) {
        let new_version = {
            let mut state = self.shared.state.lock().unwrap();
            state.value = value;

            let new_version = state.version.increment();
            state.version = new_version;
            new_version
            // unlock the mutex
        };
        self.local_version = new_version; // local is up to date
        self.shared.notify.notify_waiters();
    }

    /// Returns a future that is waken up when the version is incremented.
    pub fn change_notif<'a>(&'a self) -> impl Future + 'a {
        self.shared.notify.notified()
    }

    pub async fn read_changed(&mut self) -> Ref<T> {
        self.change_notif().await;
        self.read()
    }

    /// If the value has changed, calls a function on it and returns its result.
    ///
    /// Updates the local version.
    pub fn map_if_changed<F: FnOnce(&T) -> R, R>(&mut self, f: F) -> Option<R> {
        let state = self.shared.state.lock().unwrap();
        let last_version = state.version;

        if self.local_version != last_version {
            // We only read the value, update the local version.
            self.local_version = last_version;
            Some(f(&state.value))
        } else {
            None
        }
    }

    /// If the value has changed, modifies it with the provided function and returns its result.
    ///
    /// Updates the local and shared version.
    pub fn update_if_changed<F: FnOnce(&mut T) -> R, R>(&mut self, f: F) -> Option<R> {
        let mut state = self.shared.state.lock().unwrap();
        let last_version = state.version;

        if self.local_version != last_version {
            // We update the value, update the local and shared version.
            let new_version = last_version.increment();
            self.local_version = new_version;
            state.version = new_version;
            Some(f(&mut state.value))
        } else {
            None
        }
    }

    pub fn has_changed(&self) -> bool {
        let last_version = self.shared.state.lock().unwrap().version;
        self.local_version != last_version
    }

    pub fn seek(&self) -> Ref<'_, T> {
        Ref {
            guard: self.shared.state.lock().unwrap(),
            local_version: &self.local_version,
        }
    }

    pub fn read(&mut self) -> Ref<'_, T> {
        let guard = self.shared.state.lock().unwrap();
        self.local_version = guard.version; // update the local version
        let local_version = &self.local_version;
        Ref { guard, local_version }
    }

    pub fn borrow_mut(&mut self) -> RefMut<'_, T> {
        RefMut::new(self)
    }
}

impl<T> Clone for Versioned<T> {
    fn clone(&self) -> Self {
        Self {
            shared: self.shared.clone(),
            local_version: self.local_version.clone(),
        }
    }
}

pub struct Ref<'a, T> {
    guard: MutexGuard<'a, State<T>>,
    local_version: &'a Version,
}

impl<'a, T> Ref<'a, T> {
    fn new(v: &'a Versioned<T>) -> Self {
        let guard = v.shared.state.lock().unwrap();
        let local_version = &v.local_version;
        Self { guard, local_version }
    }

    pub fn has_changed(&self) -> bool {
        *self.local_version != self.guard.version
    }
}

impl<'a, T> ops::Deref for Ref<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.guard.value
    }
}

pub struct RefMut<'a, T> {
    guard: MutexGuard<'a, State<T>>,
    local_version: &'a mut Version,
}

impl<'a, T> RefMut<'a, T> {
    fn new(v: &'a mut Versioned<T>) -> Self {
        let guard = v.shared.state.lock().unwrap();
        let local_version = &mut v.local_version;
        Self { local_version, guard }
    }

    pub fn has_changed(&self) -> bool {
        *self.local_version != self.guard.version
    }
}

impl<'a, T> ops::Deref for RefMut<'a, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        &self.guard.value
    }
}

impl<'a, T> ops::DerefMut for RefMut<'a, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        let new_version = self.guard.version.increment();
        self.guard.version = new_version;
        *self.local_version = new_version;
        &mut self.guard.value
    }
}
