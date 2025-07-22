/// An `u8` with a default value of `u8::MAX`.
///
/// # Why?
/// This type allows to `derive(Default)` for the mapping structs.
///
/// The stat file contains less than 64 lines, and SelectiveStatFile only supports up to 64 lines.
/// Use a value above 63 as the default value, so that it is never equal to a line index.
/// This is more efficient than using an Option: 1 byte instead of 2, and no additional branch when comparing to the line index.
pub(crate) struct LineIndex(pub u8);

impl Default for LineIndex {
    fn default() -> Self {
        Self(u8::MAX)
    }
}

impl From<u8> for LineIndex {
    fn from(value: u8) -> Self {
        Self(value)
    }
}

impl From<LineIndex> for u8 {
    fn from(value: LineIndex) -> Self {
        value.0
    }
}

#[cfg(test)]
mod tests {
    use crate::measure::v2::line_index::LineIndex;
  
    #[test]
    pub fn test_from() -> anyhow::Result<()> {
        let from_u8 = LineIndex::from(14 as u8);
        let from_lineindex = LineIndex::from(LineIndex(67 as u8));
        assert_eq!(from_u8.0, 14);
        assert_eq!(from_lineindex.0, 67);

        let default_lineindex = LineIndex::default();
        let value: u8 = default_lineindex.into();
        assert_eq!(value, u8::MAX);
        Ok(())
    }


}