use std::fmt::Debug;

/// A fast bitset, limited to values between 0 and 63.
#[derive(PartialEq, Eq)]
pub struct BitSet64(u64);

impl BitSet64 {
    /// Creates a new BitSet with the given values.
    pub fn new(set: &[u8]) -> Self {
        let mut bits = 0;
        for v in set {
            assert!(*v < 64, "invalid value: {v} is outside of the range for BitSet64");
            bits |= 1 << v;
        }
        Self(bits)
    }

    /// Adds a value to the set.
    pub fn add(&mut self, v: u8) {
        assert!(v < 64, "invalid value: {v} is outside of the range for BitSet64");
        self.0 |= 1 << v
    }

    /// Checks whether the set contains a value.
    pub fn contains(&self, v: u8) -> bool {
        debug_assert!(v < 64, "invalid value: {v} is outside of the range for BitSet64");
        self.0 & (1 << v) != 0
    }
}

impl Default for BitSet64 {
    /// Creates an empty bitset.
    fn default() -> Self {
        Self(0)
    }
}

impl Debug for BitSet64 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // print the value in binary, so that we can see each "flag"
        f.debug_tuple("BitSet64").field(&format_args!("{:b}", self.0)).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitset() {
        let empty = BitSet64::default();
        assert_eq!(empty.0, 0);
        for i in 0..64 {
            assert!(!empty.contains(i));
        }

        let mut set = BitSet64::new(&[0, 1, 2, 25, 32]);
        assert_ne!(set.0, 0);
        for i in &[0, 1, 2, 25, 32] {
            assert!(set.contains(*i));
        }
        for i in &[3, 4, 5, 6, 7, 8, 63] {
            assert!(!set.contains(*i));
        }
        assert!(!set.contains(12));
        set.add(12);
        assert!(set.contains(12));
        assert_eq!(set, BitSet64::new(&[0, 1, 2, 12, 25, 32]))
    }
}
