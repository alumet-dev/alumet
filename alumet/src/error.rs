use std::{fmt::Display, fmt::{Debug, Formatter}, error::Error};
use std::fmt::Result as FmtResult;

#[derive(Debug)]
pub(crate) struct GenericError<K: Display + Debug> {
    pub(crate) kind: K,
    pub(crate) cause: Option<Box<dyn Error>>,
    pub(crate) description: Option<String>,
}

impl<K: Display + Debug> Error for GenericError<K> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.cause.as_deref()
    }
}

impl<K: Display + Debug> Display for GenericError<K> {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        write!(f, "{}", self.kind)?;
        if let Some(desc) = &self.description {
            write!(f, ": {desc}")?;
        }
        if let Some(err) = &self.cause {
            write!(f, "\nCaused by: {err}")?;
        }
        Ok(())
    }
}
