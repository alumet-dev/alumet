use std::collections::hash_map::Entry;

use fxhash::FxHashMap;
use thiserror::Error;

#[derive(Debug, Error)]
#[error("duplicate name: {key}/{subkey}")]
pub struct DuplicateNameError {
    key: String,
    subkey: String,
}

/// 2-level namespace: stores values of type `V` by two levels of keys: the `key` and the `subkey`.
///
/// Iteration order is not guaranteed.
pub struct Namespace2<V> {
    map: FxHashMap<(String, String), V>,
}

impl<V> Namespace2<V> {
    /// Creates an empty hierarchy of namespaces.
    pub fn new() -> Self {
        Self {
            map: FxHashMap::default(),
        }
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

    pub fn iter(&self) -> impl Iterator<Item = (&(String, String), &V)> {
        self.map.iter()
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&(String, String), &mut V)> {
        self.map.iter_mut()
    }

    pub fn flat_keys(&self) -> impl Iterator<Item = &(String, String)> {
        self.map.keys()
    }

    pub fn replace_each(&mut self, mut f: impl FnMut(&(String, String), V) -> V) {
        self.map = std::mem::take(&mut self.map)
            .into_iter()
            .map(|(keys, v)| {
                let replaced = f(&keys, v);
                (keys, replaced)
            })
            .collect();
    }
}

impl<V> IntoIterator for Namespace2<V> {
    type Item = ((String, String), V);

    type IntoIter = std::collections::hash_map::IntoIter<(String, String), V>;

    fn into_iter(self) -> Self::IntoIter {
        self.map.into_iter()
    }
}

impl<'a, V> IntoIterator for &'a Namespace2<V> {
    type Item = (&'a (String, String), &'a V);

    type IntoIter = std::collections::hash_map::Iter<'a, (String, String), V>;

    fn into_iter(self) -> Self::IntoIter {
        self.map.iter()
    }
}
