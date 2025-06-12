use std::fmt::Debug;

/// A fast bitset, limited to values between 0 and 127.
#[derive(PartialEq, Eq)]
pub struct BitSet128(u128);

// TODO optimization (if needed): make a 64-bits version and an unlimited version, and use generics with a BitSet trait.

impl BitSet128 {
    pub const LIMIT: u8 = 128;

    /// Creates a new BitSet with the given values.
    pub fn new(set: &[u8]) -> Self {
        let mut bits = 0;
        for v in set {
            assert!(
                *v < Self::LIMIT,
                "invalid value: {v} is outside of the range for BitSet128"
            );
            bits |= 1 << v;
        }
        Self(bits)
    }

    /// Adds a value to the set.
    pub fn add(&mut self, v: u8) {
        assert!(
            v < Self::LIMIT,
            "invalid value: {v} is outside of the range for BitSet128"
        );
        self.0 |= 1 << v
    }

    /// Checks whether the set contains a value.
    pub fn contains(&self, v: u8) -> bool {
        debug_assert!(
            v < Self::LIMIT,
            "invalid value: {v} is outside of the range for BitSet128"
        );
        self.0 & (1 << v) != 0
    }
}

impl Default for BitSet128 {
    /// Creates an empty bitset.
    fn default() -> Self {
        Self(0)
    }
}

impl Debug for BitSet128 {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // print the value in binary, so that we can see each "flag"
        f.debug_tuple("BitSet128").field(&format_args!("{:b}", self.0)).finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bitset_128() {
        let empty = BitSet128::default();
        assert_eq!(empty.0, 0);
        for i in 0..128 {
            assert!(!empty.contains(i));
        }

        let mut set = BitSet128::new(&[0, 1, 2, 25, 32, 64, 111, 127]);
        assert_ne!(set.0, 0);
        for i in &[0, 1, 2, 25, 32, 64, 111, 127] {
            assert!(set.contains(*i));
        }
        for i in &[3, 4, 5, 6, 7, 8, 62, 65, 99, 112, 110] {
            assert!(!set.contains(*i));
        }
        assert!(!set.contains(12));
        set.add(12);
        assert!(set.contains(12));
        assert_eq!(set, BitSet128::new(&[0, 1, 2, 12, 25, 32, 64, 111, 127]))
    }
}
