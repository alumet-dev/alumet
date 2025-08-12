use std::{
    borrow::Cow,
    io::{self, Write},
};

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
    use crate::csv::CsvHelper;
    use pretty_assertions::assert_eq;

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
