use std::{
    fs::{self, File},
    io::{self, BufRead, Read, Seek},
    path::Path,
};

use rustc_hash::{FxBuildHasher, FxHashMap};

use crate::measure::{
    bitset::BitSet128,
    v2::io::{IoPressureCollectorSettings, IoPressureStats},
};

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

#[derive(Debug, PartialEq, Eq)]
pub enum U64MaxResult {
    U64(u64),
    Max,
}

/// Parses a single `u64` value or the string "max" from `io_buf`.
///
/// Returns U64MaxResult::Max if the content is "max", otherwise parses as u64 and returns U64MaxResult::U64(value).
///
/// # Safety
/// The bytes passed in must be valid UTF-8.
pub unsafe fn parse_single_u64_or_max(io_buf: &[u8]) -> io::Result<U64MaxResult> {
    let content = unsafe { std::str::from_utf8_unchecked(io_buf.trim_ascii()) };

    if content == "max" {
        Ok(U64MaxResult::Max)
    } else {
        let value: u64 = content
            .parse()
            .map_err(|_| io::Error::from(io::ErrorKind::InvalidData))?;
        Ok(U64MaxResult::U64(value))
    }
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

/// Helper for reading a file that contains a single `u64` value.
pub struct ValueFile {
    file: File,
    pub(crate) maximum_value: Option<f64>,
}

impl ValueFile {
    pub fn new(file: File) -> Self {
        Self {
            file,
            maximum_value: None,
        }
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
    pub unsafe fn read(&mut self, io_buf: &mut Vec<u8>) -> io::Result<U64MaxResult> {
        read_fully(&mut self.file, io_buf)?;
        unsafe { parse_single_u64_or_max(io_buf) }
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

/// Parser for io.pressure files, which use a different format than standard stat files.
///
/// io.pressure format:
/// ```text
/// some avg10=0.00 avg60=0.00 avg300=0.00 total=91487491
/// full avg10=0.00 avg60=0.00 avg300=0.00 total=84675542
/// ```
///
/// Unlike standard stat files (space-separated key-value pairs),
/// io.pressure uses lines containing multiple space-separated key=value pairs,
/// and the line prefix (some/full) indicates the pressure type.

pub struct IoPressureFile {
    file: File,
    settings: IoPressureCollectorSettings,
}

impl IoPressureFile {
    pub fn new(file: File, settings: IoPressureCollectorSettings) -> Self {
        Self { file, settings }
    }
    // pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
    //     let f = File::open(path)?;
    //     Ok(Self::new(f))
    // }

    /// Reads the io.pressure file and extracts the total values ​​for 'some' and 'full'
    pub unsafe fn read(&mut self, io_buf: &mut Vec<u8>) -> io::Result<IoPressureStats> {
        read_fully(&mut self.file, io_buf)?;
        unsafe { parse_io_pressure(io_buf, self.settings) }
    }
}

unsafe fn parse_io_pressure(io_buf: &[u8], settings: IoPressureCollectorSettings) -> io::Result<IoPressureStats> {
    let mut res = IoPressureStats::default();
    let content = unsafe { std::str::from_utf8_unchecked(io_buf) };

    for line in content.lines() {
        let mut fields = line.split_whitespace();

        let pressure_type = match fields.next() {
            Some(kind) => kind,
            None => continue,
        };

        if (pressure_type == "some" && !settings.some_total) || (pressure_type == "full" && !settings.full_total) {
            continue;
        }

        for field in fields {
            if let Some(("total", value)) = field.split_once('=') {
                let total = value
                    .parse::<u64>()
                    .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid total value"))?;

                match pressure_type {
                    "some" => res.some_total = Some(total),
                    "full" => res.full_total = Some(total),
                    _ => {}
                }

                break;
            }
        }
    }

    Ok(res)
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

    mod io_pressure {
        use super::*;
        use pretty_assertions::assert_eq;

        fn settings() -> IoPressureCollectorSettings {
            IoPressureCollectorSettings::default()
        }

        #[test]
        fn parses_some_and_full_totals() {
            let input = br#"some avg10=0.00 avg60=0.00 avg300=0.00 total=91487491
            full avg10=0.00 avg60=0.00 avg300=0.00 total=84675542"#;

            let stats = unsafe { parse_io_pressure(input, settings()).unwrap() };

            assert_eq!(stats.some_total, Some(91_487_491));
            assert_eq!(stats.full_total, Some(84_675_542));
        }

        #[test]
        fn parses_only_some() {
            let input = br#"some avg10=0.00 avg60=0.00 avg300=0.00 total=12345"#;

            let stats = unsafe { parse_io_pressure(input, settings()).unwrap() };

            assert_eq!(stats.some_total, Some(12_345));
            assert_eq!(stats.full_total, None);
        }

        #[test]
        fn parses_only_full() {
            let input = br#"full avg10=0.00 avg60=0.00 avg300=0.00 total=67890"#;

            let stats = unsafe { parse_io_pressure(input, settings()).unwrap() };
            assert_eq!(stats.some_total, None);
            assert_eq!(stats.full_total, Some(67_890));
        }

        #[test]
        fn parses_only_some_configured() {
            let input = br#"some avg10=0.00 avg60=0.00 avg300=0.00 total=12345
            full avg10=0.00 avg60=0.00 avg300=0.00 total=84675542"#;

            let conf = IoPressureCollectorSettings {
                some_total: true,
                full_total: false,
            };
            let stats = unsafe { parse_io_pressure(input, conf).unwrap() };
            assert_eq!(stats.some_total, Some(12_345));
            assert_eq!(stats.full_total, None);
        }

        #[test]
        fn parses_only_full_configured() {
            let input = br#"full avg10=0.00 avg60=0.00 avg300=0.00 total=67890
            some avg10=0.00 avg60=0.00 avg300=0.00 total=12345"#;

            let conf = IoPressureCollectorSettings {
                some_total: false,
                full_total: true,
            };
            let stats = unsafe { parse_io_pressure(input, conf).unwrap() };
            assert_eq!(stats.some_total, None);
            assert_eq!(stats.full_total, Some(67_890));
        }

        #[test]
        fn parses_only_full_configured_messy() {
            let input = br#"full avg10=0.00 avg60=0.00 total=67890 avg300=0.00
            some total=12345 avg10=0.00 avg60=0.00 avg300=0.00"#;

            let conf = IoPressureCollectorSettings {
                some_total: false,
                full_total: true,
            };
            let stats = unsafe { parse_io_pressure(input, conf).unwrap() };
            assert_eq!(stats.some_total, None);
        }

        #[test]
        fn ignores_unknown_pressure_type() {
            let input = br#"invalid avg10=0.00 avg60=0.00 avg300=0.00 total=999"#;

            let stats = unsafe { parse_io_pressure(input, settings()).unwrap() };
            assert_eq!(stats.some_total, None);
            assert_eq!(stats.full_total, None);
        }

        #[test]
        fn returns_error_on_invalid_total() {
            let input = br#"some avg10=0.00 avg60=0.00 avg300=0.00 total=not_a_number"#;

            assert!(unsafe { parse_io_pressure(input, settings()) }.is_err());
        }

        #[test]
        fn handles_empty_input() {
            let stats = unsafe { parse_io_pressure(b"", settings()).unwrap() };

            assert_eq!(stats.some_total, None);
            assert_eq!(stats.full_total, None);
        }

        #[test]
        fn ignores_lines_without_total() {
            let input = br#"some avg10=0.00 avg60=0.00 avg300=0.00
            full avg10=0.00 avg60=0.00 avg300=0.00"#;

            let stats = unsafe { parse_io_pressure(input, settings()).unwrap() };
            assert_eq!(stats.some_total, None);
            assert_eq!(stats.full_total, None);
        }
    }

    mod parse_single_u64_or_max {
        use super::*;
        use pretty_assertions::assert_eq;
        use std::assert;
        use std::io::ErrorKind;

        #[test]
        fn parse_valid_numeric_values() {
            let test_cases: Vec<(&[u8], u64)> = vec![
                (b"0", 0),
                (b"19", 19),
                (b"85858585", 85858585),
                (b"18446744073709551615", u64::MAX),
            ];

            for (input, expected) in test_cases {
                let result = unsafe { parse_single_u64_or_max(input) };
                assert_eq!(
                    result.unwrap(),
                    U64MaxResult::U64(expected),
                    "Failed for input: {:?}",
                    String::from_utf8_lossy(input)
                );
            }
            let result = unsafe { parse_single_u64_or_max(b"max") };
            assert_eq!(result.unwrap(), U64MaxResult::Max)
        }

        #[test]
        fn parse_with_whitespace() {
            let test_cases: Vec<(&[u8], u64)> = vec![
                (b"42\n", 42),
                (b"  42  ", 42),
                (b"\t42\t", 42),
                (b"42  \n", 42),
                (b"  42  \n", 42),
            ];

            for (input, expected) in test_cases {
                let result = unsafe { parse_single_u64_or_max(input) };
                self::assert_eq!(
                    result.unwrap(),
                    U64MaxResult::U64(expected),
                    "Failed for input: {:?}",
                    String::from_utf8_lossy(input)
                );
            }
        }

        #[test]
        fn parse_max_with_provided_maximum() {
            let test_cases: Vec<&[u8]> = vec![b"max", b"max", b"max", b"max"];

            for input in test_cases {
                let result = unsafe { parse_single_u64_or_max(input) };
                self::assert!(
                    matches!(result.unwrap(), U64MaxResult::Max),
                    "Failed for input: {:?}. Expected Max",
                    String::from_utf8_lossy(input),
                );
            }
        }

        #[test]
        fn parse_max_with_whitespace() {
            let test_cases: Vec<&[u8]> = vec![b"max\n", b"  max  ", b"\tmax\t", b"max  \n"];

            for input in test_cases {
                let result = unsafe { parse_single_u64_or_max(input) };
                self::assert!(
                    matches!(result.unwrap(), U64MaxResult::Max),
                    "Failed for input: {:?}. Expected Max",
                    String::from_utf8_lossy(input),
                );
            }
        }

        #[test]
        fn parse_invalid_numeric_values() {
            let test_cases: Vec<&[u8]> = vec![
                b"",
                b"not_a_number",
                b"abc",
                b"42.5",                           // Decimal not allowed
                b"-1",                             // Negative not allowed
                b"  ",                             // Only whitespace
                b"\n\n",                           // Only newlines
                b"999999999999999999999999999999", // Overflow
            ];

            for input in test_cases {
                let result = unsafe { parse_single_u64_or_max(input) };
                assert!(
                    result.is_err(),
                    "Should fail for input: {:?}",
                    String::from_utf8_lossy(input)
                );
                assert_eq!(result.unwrap_err().kind(), ErrorKind::InvalidData);
            }
        }

        #[test]
        fn parse_mixed_case_max() {
            // "max" should be case-sensitive, all except last below should fail
            let test_cases: Vec<&[u8]> = vec![b"MAX", b"Max", b"mAx", b"max"];

            for input in test_cases {
                let result = unsafe { parse_single_u64_or_max(input) };
                if input == b"max" {
                    assert!(result.is_ok(), "Should succeed for 'max'");
                } else {
                    assert!(result.is_err(), "Should fail for: {:?}", String::from_utf8_lossy(input));
                }
            }
        }

        #[test]
        fn parse_edge_cases() {
            // Test various edge cases
            let test_cases: Vec<(&[u8], Option<u64>)> = vec![
                (b"1", Some(1)),                           // Minimum positive value
                (b"18446744073709551615", Some(u64::MAX)), // Maximum u64 value
                (b"00000000000000000042", Some(42)),       // Leading zeros
                (b"0x2A", None),                           // Hexadecimal should fail
                (b"0o52", None),                           // Octal should fail
                (b"0b101010", None),                       // Binary should fail
            ];

            for (input, expected) in test_cases {
                let result = unsafe { parse_single_u64_or_max(input) };
                match expected {
                    Some(exp) => {
                        assert_eq!(
                            result.unwrap(),
                            U64MaxResult::U64(exp),
                            "Failed for input: {:?}",
                            String::from_utf8_lossy(input)
                        );
                    }
                    None => {
                        assert!(
                            result.is_err(),
                            "Should fail for input: {:?}",
                            String::from_utf8_lossy(input)
                        );
                    }
                }
            }
        }

        #[test]
        fn parse_with_special_characters() {
            // Test that special characters are handled correctly
            let test_cases: Vec<(&[u8], Option<u64>)> = vec![
                (b"42!", None),      // Exclamation mark should fail
                (b"42$", None),      // Dollar sign should fail
                (b"42%", None),      // Percent sign should fail
                (b" 42 ", Some(42)), // Spaces should be trimmed
            ];

            for (input, expected) in test_cases {
                let result = unsafe { parse_single_u64_or_max(input) };
                match expected {
                    Some(exp) => {
                        assert_eq!(
                            result.unwrap(),
                            U64MaxResult::U64(exp),
                            "Failed for input: {:?}",
                            String::from_utf8_lossy(input)
                        );
                    }
                    None => {
                        assert!(
                            result.is_err(),
                            "Should fail for input: {:?}",
                            String::from_utf8_lossy(input)
                        );
                    }
                }
            }
        }

        #[test]
        fn parse_empty_buffer() {
            let result = unsafe { parse_single_u64_or_max(b"") };
            assert!(result.is_err());
            assert_eq!(result.unwrap_err().kind(), ErrorKind::InvalidData);
        }

        #[test]
        fn parse_very_long_number() {
            // Test with a number that has many digits
            let long_number = b"123456789012345678901234567890";
            let result = unsafe { parse_single_u64_or_max(long_number) };
            // This should fail due to overflow
            assert!(result.is_err());
            assert_eq!(result.unwrap_err().kind(), ErrorKind::InvalidData);
        }
    }
}
