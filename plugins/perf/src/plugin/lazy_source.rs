//! A source whose counters are opened on its first poll.
//!
//! A source added in pause state may never run (eg: a job that is not monitored).
use alumet::{
    measurement::{MeasurementAccumulator, Timestamp},
    pipeline::{Source, elements::error::PollError},
};

use crate::group::OpenError;

/// A factory to create a new source, called at first poll.
pub(super) type SourceFactory = Box<dyn FnMut() -> Result<Box<dyn Source>, OpenError> + Send>;

/// An source wrapper that call the source factory at first poll.
pub(super) struct LazySource {
    open: SourceFactory,
    /// `None` until the first poll.
    source: Option<Box<dyn Source>>,
}

impl LazySource {
    pub(super) fn new(open: SourceFactory) -> Self {
        Self { open, source: None }
    }
}

impl Source for LazySource {
    fn poll(&mut self, measurements: &mut MeasurementAccumulator, timestamp: Timestamp) -> Result<(), PollError> {
        if self.source.is_none() {
            let source = match (self.open)() {
                Ok(source) => source,
                // The process or cgroup ended before the source was resumed: nothing to measure,
                // it is expected.
                Err(OpenError::TargetGone(e)) => {
                    log::warn!("the measured process or cgroup no longer exists: {e}");
                    return Err(PollError::NormalStop);
                }
                // E.g. an event not available on this node.
                Err(OpenError::Other(e)) => return Err(PollError::Fatal(e)),
            };
            self.source = Some(source);
            // Read on the next poll.
            return Ok(());
        }
        self.source
            .as_mut()
            .expect("opened above")
            .poll(measurements, timestamp)
    }

    /// Called on every pause: close the counters so a paused source counts nothing, as if it had
    /// never been opened. The next poll after a resume opens them again.
    fn reset(&mut self) -> anyhow::Result<()> {
        self.source = None;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alumet::measurement::MeasurementBuffer;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use std::time::SystemTime;

    struct Polled(Arc<AtomicUsize>);

    impl Source for Polled {
        fn poll(&mut self, _: &mut MeasurementAccumulator, _: Timestamp) -> Result<(), PollError> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    fn poll(source: &mut LazySource) -> Result<(), PollError> {
        let mut buffer = MeasurementBuffer::new();
        source.poll(&mut buffer.as_accumulator(), SystemTime::now().into())
    }

    #[test]
    fn opens_once_on_the_first_poll() {
        let opened = Arc::new(AtomicUsize::new(0));
        let polled = Arc::new(AtomicUsize::new(0));
        let (o, p) = (opened.clone(), polled.clone());
        let mut source = LazySource::new(Box::new(move || {
            o.fetch_add(1, Ordering::SeqCst);
            Ok(Box::new(Polled(p.clone())) as Box<dyn Source>)
        }));
        assert_eq!(opened.load(Ordering::SeqCst), 0, "not opened before being polled");

        poll(&mut source).unwrap();
        poll(&mut source).unwrap();
        assert_eq!(opened.load(Ordering::SeqCst), 1);
        // The opening poll does not read the counters it has just enabled.
        assert_eq!(polled.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_pause_closes_the_source_and_the_next_poll_reopens_it() {
        let opened = Arc::new(AtomicUsize::new(0));
        let polled = Arc::new(AtomicUsize::new(0));
        let (o, p) = (opened.clone(), polled.clone());
        let mut source = LazySource::new(Box::new(move || {
            o.fetch_add(1, Ordering::SeqCst);
            Ok(Box::new(Polled(p.clone())) as Box<dyn Source>)
        }));

        poll(&mut source).unwrap();
        source.reset().unwrap();
        poll(&mut source).unwrap();
        assert_eq!(opened.load(Ordering::SeqCst), 2);
        // Neither opening poll reads the counters.
        assert_eq!(polled.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_failed_opening_is_fatal_unless_the_consumer_is_gone() {
        let mut source = LazySource::new(Box::new(|| Err(anyhow::anyhow!("event not available").into())));
        assert!(matches!(poll(&mut source), Err(PollError::Fatal(_))));

        let mut source = LazySource::new(Box::new(|| {
            Err(OpenError::TargetGone(std::io::Error::from_raw_os_error(libc::ESRCH)))
        }));
        assert!(matches!(poll(&mut source), Err(PollError::NormalStop)));
    }
}
