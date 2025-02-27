use std::fmt::{Debug, Display};

use super::naming::ElementName;

/// Add this context to errors that originate from a pipeline element.
///
/// Using this type instead of `ElementName` directly or a custom message allows
/// to obtain the name of the element later, when handling the error.
/// It also prints nicer error messages.
#[derive(Debug)]
pub(crate) struct ElementErrorContext(ElementName);

/// At least one error occured in the pipeline.
///
/// If multiple pipeline elements fail, only the most recent error is stored in this struct.
#[derive(thiserror::Error)]
pub struct PipelineError(#[source] anyhow::Error);

impl Display for ElementErrorContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("error in ")?;
        Display::fmt(&self.0, f)
    }
}

impl PipelineError {
    pub(crate) fn for_element(element: impl Into<ElementName>, error: anyhow::Error) -> Self {
        Self(error.context(ElementErrorContext(element.into())))
    }
    
    pub(crate) fn internal(error: anyhow::Error) -> Self {
        Self(error)
    }

    /// If the error was created by a pipeline element (source, transform, output),
    /// returns the name of that element.
    ///
    /// # How it works
    /// If an [`ElementErrorContext`] has been attached to this error or to a parent error,
    /// returns the underlying [`ElementName`].
    ///
    /// Attaching an [`ElementErrorContext`] means wrapping the error by using
    /// [`anyhow::Context::context`] or [`anyhow::Context::with_context`].
    pub fn element(&self) -> Option<&ElementName> {
        // Use anyhow downcasting, which works for error types _and_ for contexts.
        // Try self first.
        let maybe_ctx = self.0.downcast_ref::<ElementErrorContext>();
        match maybe_ctx {
            Some(ctx) => Some(&ctx.0),
            None => {
                // Walk up the chain of errors.
                for parent in self.0.chain() {
                    // Unfortunately, we cannot downcast `&dyn Error` to `anyhow::Error`.
                    // Best effort solution: try to downcast to an error type that we know.
                    if let Some(p) = parent.downcast_ref::<PipelineError>() {
                        if let Some(ctx) = p.0.downcast_ref::<ElementErrorContext>() {
                            return Some(&ctx.0);
                        }
                    }
                }
                None
            }
        }
    }

    /// Returns true if the error was caused by a pipeline element.
    pub fn is_element(&self) -> bool {
        self.element().is_some()
    }

    /// Returns true if the error was caused by an internal operation of the Alumet pipeline.
    pub fn is_internal(&self) -> bool {
        self.element().is_none()
    }
}

impl From<anyhow::Error> for PipelineError {
    fn from(value: anyhow::Error) -> Self {
        Self(value.context("error in pipeline"))
    }
}

impl Display for PipelineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.0, f)
    }
}

impl Debug for PipelineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Debug::fmt(&self.0, f)
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error;

    use anyhow::anyhow;

    use crate::pipeline::{
        error::ElementErrorContext,
        naming::{ElementKind, ElementName},
    };

    use super::PipelineError;

    #[test]
    fn check_types() {
        fn assert_is_error<T: std::error::Error>() {}

        assert_is_error::<PipelineError>();
    }

    #[test]
    fn downcasting() {
        let name = ElementName {
            kind: ElementKind::Source,
            plugin: String::from("plugin"),
            element: String::from("source-1"),
        };
        let err = PipelineError(anyhow!("abcd"));
        assert_eq!(err.element(), None);

        let err = PipelineError(anyhow!("abcd").context(ElementErrorContext(name.clone())));
        assert_eq!(err.element(), Some(&name));

        let nested = err.0.context("more context");
        let nested = PipelineError::from(nested);
        println!("nested error: {:#}", nested);
        println!("nested error source: {:?}", nested.source());
        assert_eq!(nested.element(), Some(&name));

        let nested = nested.0.context("more context");
        let nested = PipelineError::from(nested);
        println!("nested2 error: {:#}", nested);
        println!("nested2 error source: {:?}", nested.source());
        assert_eq!(nested.element(), Some(&name));

        let wrapped = anyhow::Error::new(nested);
        let wrapped = PipelineError::from(wrapped);
        println!("wrapped error: {:#}", wrapped);
        println!("wrapped error source: {:#}", wrapped.0.source().unwrap());
        assert_eq!(wrapped.element(), Some(&name));
    }
}
