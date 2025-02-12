use crate::pipeline::elements::{output, source, transform};

/// A message that can be sent "to the pipeline" (I'm simplifying here) in order to control it.
#[derive(Debug)]
pub enum ControlMessage {
    /// Controls sources.
    Source(source::control::ControlMessage),
    /// Controls transforms.
    Transform(transform::control::ControlMessage),
    /// Controls outputs.
    Output(output::control::ControlMessage),
}

pub mod matching {
    use crate::pipeline::{
        control::key::{OutputKey, SourceKey, TransformKey},
        matching::{OutputNamePattern, SourceNamePattern, TransformNamePattern},
        naming::{OutputName, SourceName, TransformName},
    };

    /// Matches some sources of the pipeline.
    #[derive(Debug, Clone)]
    pub enum SourceMatcher {
        Key(SourceKey),
        Name(SourceNamePattern),
    }

    /// Matches some outputs of the pipeline.
    #[derive(Debug, Clone)]
    pub enum OutputMatcher {
        Key(OutputKey),
        Name(OutputNamePattern),
    }

    /// Matches some transforms of the pipeline.
    #[derive(Debug, Clone)]
    pub enum TransformMatcher {
        Key(TransformKey),
        Name(TransformNamePattern),
    }

    impl SourceMatcher {
        pub(crate) fn matches(&self, name: &SourceName) -> bool {
            match self {
                SourceMatcher::Key(source_key) => &source_key.0 == name,
                SourceMatcher::Name(source_name_pattern) => source_name_pattern.matches(&name),
            }
        }
    }

    impl TransformMatcher {
        #[allow(unused)] // for later
        pub(crate) fn matches(&self, name: &TransformName) -> bool {
            match self {
                TransformMatcher::Key(transform_key) => &transform_key.0 == name,
                TransformMatcher::Name(transform_name_pattern) => transform_name_pattern.matches(&name),
            }
        }
    }

    impl OutputMatcher {
        #[allow(unused)] // for later
        pub(crate) fn matches(&self, name: &OutputName) -> bool {
            match self {
                OutputMatcher::Key(output_key) => &output_key.0 == name,
                OutputMatcher::Name(output_name_pattern) => output_name_pattern.matches(&name),
            }
        }
    }

    impl From<SourceKey> for SourceMatcher {
        fn from(value: SourceKey) -> Self {
            Self::Key(value)
        }
    }

    impl From<SourceNamePattern> for SourceMatcher {
        fn from(value: SourceNamePattern) -> Self {
            Self::Name(value)
        }
    }

    impl From<TransformKey> for TransformMatcher {
        fn from(value: TransformKey) -> Self {
            Self::Key(value)
        }
    }

    impl From<TransformNamePattern> for TransformMatcher {
        fn from(value: TransformNamePattern) -> Self {
            Self::Name(value)
        }
    }

    impl From<OutputKey> for OutputMatcher {
        fn from(value: OutputKey) -> Self {
            Self::Key(value)
        }
    }

    impl From<OutputNamePattern> for OutputMatcher {
        fn from(value: OutputNamePattern) -> Self {
            Self::Name(value)
        }
    }
}
