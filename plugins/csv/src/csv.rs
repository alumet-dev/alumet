use std::{
    borrow::Cow,
    fs::File,
    io::{self, Write},
};

use rustc_hash::FxHashMap;

pub struct CsvWriter {
    file: File,
    /// Columns of the header.
    header: Vec<String>,
}

impl CsvWriter {
    pub fn new(file: File) -> Self {
        Self {
            file,
            header: Vec::new(),
        }
    }

    pub fn is_initialized(&self) -> bool {
        !self.header.is_empty()
    }

    pub fn write_header(&mut self, header: Vec<String>) -> anyhow::Result<()> {
        assert!(self.header.is_empty());
        for column in &header {
            write!(&mut self.file, "{column};")?;
        }
        writeln!(&mut self.file, "__late_attributes")?;
        self.header = header;
        Ok(())
    }

    pub fn write_line(&mut self, data: &mut FxHashMap<String, String>) -> anyhow::Result<()> {
        assert!(!self.header.is_empty());

        // Write the data in the columns that we know
        for column in &self.header {
            if let Some(value) = data.remove(column) {
                write!(&mut self.file, "{value}")?;
            }
            write!(&mut self.file, ";")?;
        }

        // Write the data in the "late attributes" column
        let last = data.len().saturating_sub(1);
        for (i, (k, v)) in data.iter().enumerate() {
            write!(&mut self.file, "{k}={v}")?;
            if i != last {
                write!(&mut self.file, ",")?;
            }
        }
        write!(&mut self.file, "\n")?;

        Ok(())
    }

    pub fn flush(&mut self) -> anyhow::Result<()> {
        self.file.flush()?;
        Ok(())
    }

    // fn write2(&mut self, columns: &[&str], value: &[&str]) -> anyhow::Result<()> {
    //     if self.header.is_empty() {}
    //     Ok(())
    // }
}

pub struct CsvHelper {
    /// The CSV delimiter, such as `';'`.
    delimiter: char,

    /// Same as `delimiter` but in a string.
    delimiter_string: String,

    /// How to escape quotes in values, example `'\\"'`
    escaped_quote: String,
}

impl CsvHelper {
    pub fn new(delimiter: char, escaped_quote: String) -> Self {
        Self {
            delimiter,
            delimiter_string: delimiter.to_string(),
            escaped_quote,
        }
    }

    /// Escape a string for CSV formatting.
    ///
    /// See <https://www.ietf.org/rfc/rfc4180.txt>.
    pub fn escape_string<'a>(&self, s: &'a str) -> Cow<'a, str> {
        if s.contains([self.delimiter, '"', '\n', '\r']) {
            let escaped = s.replace('"', &self.escaped_quote);
            let quoted = format!("\"{escaped}\"");
            Cow::Owned(quoted)
        } else {
            Cow::Borrowed(s)
        }
    }

    pub fn writeln<R: IntoIterator<Item = S>, S: AsRef<str>>(&self, w: &mut impl Write, record: R) -> io::Result<()> {
        // TODO avoid allocations in there
        let csv_record: Vec<String> = record
            .into_iter()
            .map(|elem| self.escape_string(elem.as_ref()).to_string())
            .collect();
        writeln!(w, "{}", csv_record.join(&self.delimiter_string))
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs::File,
        io::{Read, Write},
    };

    use crate::csv::{CsvHelper, CsvWriter};
    use indoc::indoc;
    use pretty_assertions::assert_eq;
    use rustc_hash::FxHashMap;

    #[test]
    fn csv_writer() -> anyhow::Result<()> {
        let tmp = tempfile::tempdir()?;
        let path = tmp.path().join("test.csv");
        let mut writer = CsvWriter::new(File::create(&path)?);
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
            ("sensor".to_owned(), "petits pois".to_owned()),
            ("pc".to_owned(), "thinkpad".to_owned()),
        ]))?;
        writer.write_line(&mut FxHashMap::from_iter(vec![
            ("value".to_owned(), "7".to_owned()),
            ("metric".to_owned(), "testtttt".to_owned()),
            ("sensor".to_owned(), "local".to_owned()),
            ("gpu".to_owned(), "H200".to_owned()),
            ("cpu".to_owned(), "EPYC".to_owned()),
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
            123456;test;25;petits pois;thinkpad;
            ;testtttt;7;local;;gpu=H200,cpu=EPYC
            42;amd;7;carottes;;
        "}
        );

        Ok(())
    }

    #[test]
    fn csv_escape() {
        let helper: CsvHelper = CsvHelper::new(',', "\"\"".into());
        assert_eq!("abcdefg", helper.escape_string("abcdefg"));
        assert_eq!("\"abcd\"\"efg\"", helper.escape_string("abcd\"efg"));
        assert_eq!("\"abcd,efg\"", helper.escape_string("abcd,efg"));
        assert_eq!("abcd;efg", helper.escape_string("abcd;efg"));
        assert_eq!("", helper.escape_string(""));

        let helper: CsvHelper = CsvHelper::new(';', "\\\"".into());
        assert_eq!("abcdefg", helper.escape_string("abcdefg"));
        assert_eq!("\"abcd\\\"efg\"", helper.escape_string("abcd\"efg"));
        assert_eq!("\"abcd;efg\"", helper.escape_string("abcd;efg"));
        assert_eq!("abcd,efg", helper.escape_string("abcd,efg"));
        assert_eq!("", helper.escape_string(""));
    }

    #[test]
    fn csv_write() {
        let helper: CsvHelper = CsvHelper::new(',', "\"\"".into());

        let mut res = Vec::new();
        helper.writeln(&mut res, vec!["a", "b", "c"]).unwrap();
        assert_eq!("a,b,c\n", String::from_utf8(res).unwrap());

        let mut res = Vec::new();
        helper.writeln(&mut res, vec![" a", "b  b", "c "]).unwrap();
        assert_eq!(" a,b  b,c \n", String::from_utf8(res).unwrap());

        let mut res = Vec::new();
        helper.writeln(&mut res, vec!["a", "b,b,b", "c"]).unwrap();
        assert_eq!("a,\"b,b,b\",c\n", String::from_utf8(res).unwrap());

        let helper: CsvHelper = CsvHelper::new(';', "\"\"".into());

        let mut res = Vec::new();
        helper.writeln(&mut res, vec!["a", "b,b,b", "c"]).unwrap();
        assert_eq!("a;b,b,b;c\n", String::from_utf8(res).unwrap());
    }
}
