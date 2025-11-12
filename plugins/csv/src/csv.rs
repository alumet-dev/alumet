use std::{borrow::Cow, fs::File, io::Write};

use rustc_hash::FxHashMap;

pub struct CsvWriter {
    /// File, opened for writing.
    file: File,

    /// Columns of the header.
    /// If empty, the header has not been written yet.
    header: Vec<String>,

    /// CSV options,
    params: CsvParams,
}

pub struct CsvParams {
    pub delimiter: char,
    pub late_delimiter: char,
}

impl Default for CsvParams {
    fn default() -> Self {
        Self {
            delimiter: ';',
            late_delimiter: ',',
        }
    }
}

impl CsvWriter {
    pub fn new(file: File, params: CsvParams) -> Self {
        Self {
            file,
            header: Vec::new(),
            params,
        }
    }

    pub fn is_initialized(&self) -> bool {
        !self.header.is_empty()
    }

    pub fn write_header(&mut self, header: Vec<String>) -> anyhow::Result<()> {
        assert!(self.header.is_empty());
        for column in &header {
            write!(&mut self.file, "{column}{}", self.params.delimiter)?;
        }
        writeln!(&mut self.file, "__late_attributes")?;
        self.header = header;
        Ok(())
    }

    pub fn write_line(&mut self, data: &mut FxHashMap<String, String>) -> anyhow::Result<()> {
        use std::fmt::Write;

        assert!(!self.header.is_empty());

        // Write the data in the columns that we know
        for column in &self.header {
            if let Some(value) = data.remove(column) {
                let value = self.params.escape_string(&value);
                write!(&mut self.file, "{value}")?;
            }
            write!(&mut self.file, "{}", self.params.delimiter)?;
        }

        // Write the data in the "late attributes" column.
        // First, build a string. Then escape it if needed and write it into the last column.
        let last = data.len().saturating_sub(1);
        let mut late_value = String::new();
        for (i, (k, v)) in data.iter().enumerate() {
            let v = self.params.escape_string_late(v);
            write!(&mut late_value, "{k}={v}")?;
            if i != last {
                write!(&mut late_value, "{}", self.params.late_delimiter)?;
            }
        }
        let late_value = self.params.escape_string(&late_value);
        writeln!(&mut self.file, "{late_value}")?;

        Ok(())
    }

    pub fn flush(&mut self) -> anyhow::Result<()> {
        self.file.flush()?;
        Ok(())
    }
}

impl CsvParams {
    /// Escape a string for CSV formatting.
    ///
    /// See <https://www.ietf.org/rfc/rfc4180.txt>.
    pub fn escape_string<'a>(&self, s: &'a str) -> Cow<'a, str> {
        if s.contains([self.delimiter, '"', '\n', '\r']) {
            let escaped = s.replace('"', "\"\"");
            let quoted = format!("\"{escaped}\"");
            Cow::Owned(quoted)
        } else {
            Cow::Borrowed(s)
        }
    }

    /// Escape a string for late attributes formatting.
    pub fn escape_string_late<'a>(&self, s: &'a str) -> Cow<'a, str> {
        if s.contains(self.late_delimiter) {
            let escaped = s.replace(self.late_delimiter, &format!("\\{}", self.late_delimiter));
            Cow::Owned(escaped)
        } else {
            Cow::Borrowed(s)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{fs::File, io::Write};

    use crate::csv::{CsvParams, CsvWriter};
    use indoc::indoc;
    use pretty_assertions::assert_eq;
    use rustc_hash::FxHashMap;

    #[test]
    fn csv_writer() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let path = tmp.path().join("test.csv");
        let mut writer = CsvWriter::new(File::create(&path)?, CsvParams::default());
        println!("{path:?}");

        writer.write_header(vec![
            "timestamp".to_owned(),
            "metric".to_owned(),
            "value".to_owned(),
            "sensor".to_owned(),
            "pc".to_owned(),
        ])?;
        writer.write_line(&mut FxHashMap::from_iter(vec![
            ("timestamp".to_owned(), "123456".to_owned()),
            ("value".to_owned(), "25".to_owned()),
            ("metric".to_owned(), "test".to_owned()),
            ("sensor".to_owned(), "petits;pois".to_owned()),
            ("pc".to_owned(), "thinkpad".to_owned()),
        ]))?;
        writer.write_line(&mut FxHashMap::from_iter(vec![
            ("value".to_owned(), "7".to_owned()),
            ("metric".to_owned(), "testtttt".to_owned()),
            ("sensor".to_owned(), "local".to_owned()),
            ("gpu".to_owned(), "H200".to_owned()),
            ("cpu".to_owned(), "EPYC,AMD".to_owned()),
        ]))?;
        writer.write_line(&mut FxHashMap::from_iter(vec![
            ("timestamp".to_owned(), "42".to_owned()),
            ("value".to_owned(), "7".to_owned()),
            ("metric".to_owned(), "amd".to_owned()),
            ("sensor".to_owned(), "carottes".to_owned()),
        ]))?;
        writer.file.flush()?;

        let output = std::fs::read_to_string(path)?;
        assert_eq!(
            output,
            indoc! {"
            timestamp;metric;value;sensor;pc;__late_attributes
            123456;test;25;\"petits;pois\";thinkpad;
            ;testtttt;7;local;;gpu=H200,cpu=EPYC\\,AMD
            42;amd;7;carottes;;
        "}
        );

        Ok(())
    }

    #[test]
    fn csv_escape() {
        let helper = CsvParams {
            delimiter: ',',
            late_delimiter: ':',
        };
        assert_eq!("abcdefg", helper.escape_string("abcdefg"));
        assert_eq!("\"abcd\"\"efg\"", helper.escape_string("abcd\"efg"));
        assert_eq!("\"abcd,efg\"", helper.escape_string("abcd,efg"));
        assert_eq!("abcd;efg", helper.escape_string("abcd;efg"));
        assert_eq!("", helper.escape_string(""));

        let helper = CsvParams {
            delimiter: ';',
            late_delimiter: ',',
        };
        assert_eq!("abcdefg", helper.escape_string("abcdefg"));
        assert_eq!("\"abcd\"\"efg\"", helper.escape_string("abcd\"efg"));
        assert_eq!("\"abcd;efg\"", helper.escape_string("abcd;efg"));
        assert_eq!("abcd,efg", helper.escape_string("abcd,efg"));
        assert_eq!("", helper.escape_string(""));
    }
}
