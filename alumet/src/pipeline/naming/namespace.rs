use std::collections::{hash_map::Entry, HashMap};

use thiserror::Error;

#[derive(Debug, Error)]
#[error("duplicate name: {key}/{subkey}")]
pub struct DuplicateNameError {
    key: String,
    subkey: String,
}

/// Nested namespaces: stores values of type `V` by two levels of keys: the `key` and the `subkey`.
pub struct Namespaces<V> {
    map: HashMap<(String, String), V>,
}

impl<V> Namespaces<V> {
    /// Creates an empty hierarchy of namespaces.
    pub fn new() -> Self {
        Self { map: HashMap::new() }
    }

    /// Adds a new value to the `key` namespace with the `subkey` name.
    ///
    /// Returns an error if an element with the same key and subkey already exists.
    pub fn add(&mut self, key: String, subkey: String, value: V) -> Result<(), DuplicateNameError> {
        match self.map.entry((key.clone(), subkey.clone())) {
            Entry::Occupied(_) => return Err(DuplicateNameError { key, subkey }),
            Entry::Vacant(vacant) => vacant.insert(value),
        };
        Ok(())
    }

    /// Gets a value from a namespace.
    pub fn get(&self, key: &str, subkey: &str) -> Option<&V> {
        self.map.get(&(String::from(key), String::from(subkey)))
    }

    /// Removes a value from a namespace.
    pub fn remove(&mut self, key: &str, subkey: &str) -> Option<V> {
        self.map.remove(&(String::from(key), String::from(subkey)))
    }

    /// Gets the total number of values in all namespaces.
    pub fn total_count(&self) -> usize {
        self.map.len()
    }

    /// Gets the number of values in a given namespace.
    pub fn count_in(&self, key: &str) -> usize {
        self.map.keys().filter(|(k, _)| k == key).count()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}

impl<V> IntoIterator for Namespaces<V> {
    type Item = ((String, String), V);

    type IntoIter = std::collections::hash_map::IntoIter<(String, String), V>;

    fn into_iter(self) -> Self::IntoIter {
        self.map.into_iter()
    }
}
