use std::{fmt::Display, fmt::{Debug, Formatter}, error::Error};
use std::fmt::Result as FmtResult;

#[derive(Debug)]
pub(crate) struct GenericError<K: Display + Debug + Send> {
    pub(crate) kind: K,
    pub(crate) cause: Option<Box<dyn Error + Send>>,
    pub(crate) description: Option<String>,
}

impl<K: Display + Debug + Send> Error for GenericError<K> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.cause.as_ref().map(|e| e.as_ref() as _)
    }
}

impl<K: Display + Debug + Send> Display for GenericError<K> {
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

impl<K: Display + Debug + Send> GenericError<K> {
    pub fn new(kind: K) -> GenericError<K> {
        GenericError {
            kind,
            cause: None,
            description: None,
        }
    }
    
    pub fn with_description(kind: K, description: &str) -> GenericError<K> {
        GenericError {
            kind,
            cause: None,
            description: Some(description.to_owned()),
        }
    }
    
    pub fn with_source<E: Error + Send + 'static>(kind: K, description: &str, source: E) -> GenericError<K> {
        GenericError {
            kind,
            cause: Some(Box::new(source)),
            description: Some(description.to_owned()),
        }
    }
        
}
