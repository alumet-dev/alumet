use std::{
    fs::File,
    io::{self, BufRead, Read, Seek},
    path::Path,
};

use rustc_hash::{FxBuildHasher, FxHashMap};

use crate::measure::bitset::BitSet128;

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
    indices: &BitSet128,
    mut on_ikv: impl FnMut(u8, &str, u64),
) -> io::Result<()> {
    let content = unsafe { std::str::from_utf8_unchecked(io_buf) };
    for (i, line) in content.split('\n').enumerate() {
        let i = i as u8;
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
        let f = File::open(path)?;
        Ok(Self::new(f))
    }

    /// Reads the file into `io_buf` and parses its content.
    ///
    /// # Safety
    /// The content of the file must be valid UTF-8.
    ///
    /// If this file comes from the kernel's cgroupfs, then its content is always valid ASCII, hence valid UTF-8.
    pub unsafe fn read(&mut self, io_buf: &mut Vec<u8>) -> io::Result<u64> {
        read_fully(&mut self.file, io_buf)?;
        unsafe { parse_single_u64(io_buf) }
    }
}

/// Helper for reading a file in the "stat" format, that is, a file that contains one key-value pair per line, with a space between the string key and the u64 value.
///
/// # Index cache optimization
/// To speed up the parsing of the file, we remember the index of the line of each key.
///
/// Even though the kernel documentation warns about using the line index, it should work
/// because we detect the indices for each file, and its content only change depending on:
/// - the configuration of the kernel
/// - the configuration of the cgroup filesystem
///
/// IMPORTANT: If this assumption is proven false in the future, we will need to rework this.
///
/// See:
/// - https://docs.kernel.org/admin-guide/cgroup-v2.html
/// - https://github.com/torvalds/linux/blob/488ef3560196ee10fc1c5547e1574a87068c3494/mm/memcontrol.c#L1482 (for memory.stat)
pub struct SelectiveStatFile {
    file: File,
    cached_indices: BitSet128,
}

pub struct SelectiveStatMapping {
    key_to_line: FxHashMap<String, u8>,
    not_found: Vec<String>,
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
    /// The file is read into `io_mut`, which is cleared by this function.
    ///
    /// This makes [`SelectiveStatFile::read`] faster (compared to a non-cached version, which is not provided).
    pub fn build(mut self, io_buf: &mut Vec<u8>) -> io::Result<(SelectiveStatFile, SelectiveStatMapping)> {
        // read the file into the buffer
        read_fully(&mut self.file, io_buf)?;

        // this is initialization time, we can afford to check that the file is valid to avoid problems later (even though there should not be any issue)
        std::str::from_utf8(io_buf).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        if io_buf.lines().count() >= BitSet128::LIMIT.into() {
            return Err(io::Error::other("too many lines in file, the BitSet will not work"));
        }

        // find the line numbers that correspond to the keys we want, to avoid comparing the keys in subsequent reads
        let mut cached_indices = BitSet128::default();
        let mut key_to_line = FxHashMap::with_capacity_and_hasher(self.keys_to_get.len(), FxBuildHasher);
        // SAFETY: we have checked that the file is valid utf-8
        unsafe {
            parse_space_kv(io_buf, |i, k, _| {
                if let Some(pos) = self.keys_to_get.iter().position(|key| key == k) {
                    // update the bitset to ignore the lines we don't want very quickly
                    cached_indices.add(i as u8);
                    // remember the mapping key -> line index
                    key_to_line.insert(k.to_owned(), i as u8);
                    // remove the key from the list of keys we want (useful to know which ones were not found)
                    self.keys_to_get.swap_remove(pos);
                }
            })
        }?;

        // find the keys that were not found (the content of the stat file may vary depending on kernel parameters, and between the root cgroup and the child cgroups)
        let not_found = self.keys_to_get;

        // done
        let file = SelectiveStatFile {
            file: self.file,
            cached_indices,
        };
        let mapping = SelectiveStatMapping { key_to_line, not_found };
        Ok((file, mapping))
    }
}

impl SelectiveStatFile {
    /// Reads the stat file into `io_buf`, parses its content and
    /// call the provided closure `on_kv` for each key-value pair that we are interested in.
    ///
    /// Only the keys that were given to [`StatFileBuilder`] are returned.
    ///
    /// # Safety
    /// The content of the file must be valid UTF-8.
    ///
    /// If this file comes from the kernel's cgroupfs, then its content is always valid ASCII, hence valid UTF-8.
    pub unsafe fn read(&mut self, io_buf: &mut Vec<u8>, on_ikv: impl FnMut(u8, &str, u64)) -> io::Result<()> {
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
    /// Gets the line index (the first line is at index 0) of the given key.
    pub fn line_index(&self, key: &str) -> Option<u8> {
        self.key_to_line.get(key).cloned()
    }

    /// Returns the keys that were not found in the stat file.
    ///
    /// # Why is my key not found?
    /// The content of the stat file may vary depending on kernel parameters,
    /// and between the root cgroup and the child cgroups.
    pub fn keys_not_found(&self) -> &[String] {
        &self.not_found
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::io::{ErrorKind, Write};

    #[test]
    fn u64_read() -> anyhow::Result<()> {
        let tmp_dir = tempfile::tempdir()?;
        let tmp = tmp_dir.path().join("something.usage");

        // the file must exist to be opened for reading
        File::create_new(&tmp)?;

        // open the file
        let mut f = U64File::open(&tmp)?;
        let mut io_buf = Vec::new();

        // tests with various contents
        std::fs::write(&tmp, "1234")?;
        let parsed = unsafe { f.read(&mut io_buf) }?;
        assert_eq!(parsed, 1234);

        std::fs::write(&tmp, "1234\n")?;
        let parsed = unsafe { f.read(&mut io_buf) }?;
        assert_eq!(parsed, 1234);

        std::fs::write(&tmp, "0")?;
        let parsed = unsafe { f.read(&mut io_buf) }?;
        assert_eq!(parsed, 0);

        std::fs::write(&tmp, "abcd")?;
        let err = unsafe { f.read(&mut io_buf) }.expect_err("expected error");
        assert_eq!(err.kind(), ErrorKind::InvalidData);
        Ok(())
    }

    #[test]
    fn selective_stat() -> anyhow::Result<()> {
        // sample data from cgroup v2 "cpu.stat"
        const CPU_STAT: &str = "usage_usec 12849502000
user_usec 10191064000
system_usec 2658438000
core_sched.force_idle_usec 0
nr_periods 0
nr_throttled 0
throttled_usec 0
nr_bursts 12
burst_usec 0
";

        // write to the file
        let mut io_buf = Vec::new();
        let mut file = tempfile::tempfile()?;
        write!(&mut file, "{CPU_STAT}")?;

        // initialize the SelectiveStatFile
        let (mut stat_file, mapping) =
            StatFileBuilder::new(file, &["usage_usec", "nr_periods", "nr_bursts"]).build(&mut io_buf)?;

        // check the key/line mapping
        assert_eq!(
            mapping.key_to_line,
            FxHashMap::from_iter([
                ("usage_usec".to_string(), 0),
                ("nr_periods".to_string(), 4),
                ("nr_bursts".to_string(), 7)
            ])
        );
        let index_usage = mapping.line_index("usage_usec").unwrap();
        let index_periods = mapping.line_index("nr_periods").unwrap();
        let index_burst = mapping.line_index("nr_bursts").unwrap();
        assert_eq!(&[index_usage, index_periods, index_burst], &[0, 4, 7]);

        // check the bitset
        assert_eq!(stat_file.cached_indices, BitSet128::new(&[0, 4, 7]));

        // read
        // SAFETY: we have written utf-8 data in this test
        unsafe {
            stat_file.read(io_buf.as_mut(), |index, key, value| match index {
                i if i == index_usage => {
                    assert_eq!(key, "usage_usec");
                    assert_eq!(value, 12849502000);
                }
                i if i == index_periods => {
                    assert_eq!(key, "nr_periods");
                    assert_eq!(value, 0);
                }
                i if i == index_burst => {
                    assert_eq!(key, "nr_bursts");
                    assert_eq!(value, 12);
                }
                _ => panic!("unexpected line {index}: {key} {value}"),
            })
        }?;
        Ok(())
    }
}
