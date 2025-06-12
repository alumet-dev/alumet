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
