use std::{
    fs::File,
    io::{self, BufRead, Read, Seek},
    path::Path,
};

use rustc_hash::{FxBuildHasher, FxHashMap};

use crate::measure::bitset::BitSet64;

/// Reads `file` from the beginning to the end into `io_buf`.
///
/// The buffer `io_buf` is cleared first.
/// After this call, it only contains the data that has been read from `file`.
pub fn read_fully(file: &mut File, io_buf: &mut Vec<u8>) -> io::Result<()> {
    io_buf.clear();
    file.rewind()?;
    file.read_to_end(io_buf)?;
    Ok(())
}

/// Parses a single `u64` value from `io_buf`.
///
/// # Safety
/// The bytes passed in must be valid UTF-8.
pub unsafe fn parse_single_u64(io_buf: &[u8]) -> io::Result<u64> {
    let content = unsafe { std::str::from_utf8_unchecked(io_buf.trim_ascii_end()) };
    let value: u64 = content
        .parse()
        .map_err(|_| io::Error::from(io::ErrorKind::InvalidData))?;
    Ok(value)
}

/// Parses a list of key-values from `io_buf`.
///
/// Calls `on_ikv` for every key-value pair found, with `(line_index, key, value)`.
/// Empty lines and lines that do not contain a key and value, separated by a space, are ignored.
///
/// # Input format
/// ```text
/// key 123
/// other 0
/// ```
///
/// # Safety
/// The bytes passed in must be valid UTF-8.
pub unsafe fn parse_space_kv(io_buf: &[u8], mut on_ikv: impl FnMut(usize, &str, u64)) -> io::Result<()> {
    let content = unsafe { std::str::from_utf8_unchecked(io_buf) };
    for (i, line) in content.split('\n').enumerate() {
        if let Some((key, value)) = line.split_once(' ') {
            let value: u64 = value.parse().map_err(|_| io::Error::from(io::ErrorKind::InvalidData))?;
            on_ikv(i, key, value)
        }
    }
    Ok(())
}

/// Parses a list of key-values from `io_buf`, but only consider the lines
/// whose number is contained in `indices`.
///
/// Calls `on_ikv` for every key-value pair found, with `(line_index, key, value)`.
/// Empty lines and lines that do not contain a key and value, separated by a space, are ignored.
///
/// # Input format
/// ```text
/// key 123
/// other 0
/// ```
///
/// # Safety
/// The bytes passed in must be valid UTF-8.
pub unsafe fn parse_space_kv_at_lines(
    io_buf: &[u8],
    indices: &BitSet64,
    mut on_ikv: impl FnMut(usize, &str, u64),
) -> io::Result<()> {
    let content = unsafe { std::str::from_utf8_unchecked(io_buf) };
    for (i, line) in content.split('\n').enumerate() {
        if indices.contains(i) {
            if let Some((key, value)) = line.split_once(' ') {
                let value: u64 = value.parse().map_err(|_| io::Error::from(io::ErrorKind::InvalidData))?;
                on_ikv(i, key, value)
            }
        }
    }
    Ok(())
}

/// Helper for reading a file that contains a single `u64` value.
pub struct U64File {
    file: File,
}

impl U64File {
    pub fn new(file: File) -> Self {
        Self { file }
    }

    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        Ok(Self {
            file: File::open(path)?,
        })
    }

    /// Reads the file into `io_buf` and parses its content.
    pub fn read(&mut self, io_buf: &mut Vec<u8>) -> io::Result<u64> {
        // SAFETY: the file comes from the kernel (it's not an actual file) and its content is always valid ASCII (hence valid UTF-8)
        read_fully(&mut self.file, io_buf)?;
        unsafe { parse_single_u64(io_buf) }
    }
}

/// Helper for reading a file in the "stat" format, that is, a file that contains one key-value pair per line, with a space between the string key and the u64 value.
pub struct SelectiveStatFile {
    file: File,
    cached_indices: BitSet64,
}

pub struct SelectiveStatMapping {
    key_to_line: FxHashMap<String, u8>,
}

/// Builder for [`SelectiveStatFile`].
pub struct StatFileBuilder {
    file: File,
    keys_to_get: Vec<String>,
}

impl StatFileBuilder {
    /// Initializes a new builder with a list of `keys` that we are interested in.
    pub fn new<S: AsRef<str>>(file: File, keys: &[S]) -> Self {
        Self {
            file,
            keys_to_get: keys.into_iter().map(|s| s.as_ref().to_owned()).collect(),
        }
    }

    /// Reads the file and finds the position of the lines that must be read to obtain the keys we want.
    ///
    /// This makes [`SelectiveStatFile::read`] faster (compared to a non-cached version, which is not provided).
    pub fn cache_line_indices(mut self, io_buf: &mut Vec<u8>) -> io::Result<(SelectiveStatFile, SelectiveStatMapping)> {
        // read the file into the buffer
        read_fully(&mut self.file, io_buf)?;

        // this is initialization time, we can afford to check that the file is valid to avoid problems later (even though there should not be any issue)
        std::str::from_utf8(&io_buf).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        if io_buf.lines().count() >= 64 {
            return Err(io::Error::other("too many lines in file, the BitSet will not work"));
        }

        // find the line numbers that correspond to the keys we want, to avoid comparing the keys in subsequent reads
        let mut cached_indices = BitSet64::default();
        let mut key_to_line = FxHashMap::with_capacity_and_hasher(self.keys_to_get.len(), FxBuildHasher);
        unsafe {
            parse_space_kv(&io_buf, |i, k, _| {
                if self.keys_to_get.iter().any(|key| key == k) {
                    cached_indices.add(i);
                    key_to_line.insert(k.to_owned(), i as u8);
                };
            })
        }?;
        let file = SelectiveStatFile {
            file: self.file,
            cached_indices,
        };
        let mapping = SelectiveStatMapping { key_to_line };
        Ok((file, mapping))
    }
}

impl SelectiveStatFile {
    /// Reads the stat file, parses its content and call the provided closure `on_kv` for each key-value pair
    /// that we are interested in.
    ///
    /// Only the keys that were given to [`StatFileBuilder::new`] are returned.
    pub fn read(&mut self, io_buf: &mut Vec<u8>, on_ikv: impl FnMut(usize, &str, u64)) -> io::Result<()> {
        // SAFETY: the file comes from the kernel (it's not an actual file) and its content is always valid ASCII (hence valid UTF-8).
        // Furthermore, this is asserted in `cache_line_indices`.
        read_fully(&mut self.file, io_buf)?;
        unsafe { parse_space_kv_at_lines(io_buf, &self.cached_indices, on_ikv) }
    }

    /*
    TODO I think that we can do even better, by using the line index to directly store the value in a struct of u64 fields.

    A derive macro would work on a struct with:
    - one field per stat field we're interested in
    - u64 values everywhere
    - field names that match the key in the stat file (or are given the proper key with an annotation)

    The derive macro would generate:
    - A map name -> field offset
    - A function set(offset)
    - A helper line index -> field offset (?)
    */
}

impl SelectiveStatMapping {
    pub fn line_index(&self, key: &str) -> Option<u8> {
        self.key_to_line.get(key).cloned()
    }
}
