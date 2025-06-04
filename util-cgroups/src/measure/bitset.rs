/// A fast bitset, limited to values between 0 and 63.
pub struct BitSet64(u64);

impl BitSet64 {
    /// Creates a new BitSet with the given values.
    pub fn new(set: &[usize]) -> Self {
        let mut bits = 0;
        for v in set {
            assert!(*v < 64, "invalid value: {v} is outside of the range for BitSet64");
            bits |= 1 << v;
        }
        Self(bits)
    }

    /// Adds a value to the set.
    pub fn add(&mut self, v: usize) {
        assert!(v < 64, "invalid value: {v} is outside of the range for BitSet64");
        self.0 |= 1 << v
    }

    /// Checks whether the set contains a value.
    pub fn contains(&self, i: usize) -> bool {
        self.0 & (1 << i) != 0
    }
}

impl Default for BitSet64 {
    /// Creates an empty bitset.
    fn default() -> Self {
        Self(0)
    }
}
