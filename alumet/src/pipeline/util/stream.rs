//! Stream-related utilities.

use std::pin::Pin;
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Arc;
use std::task::{Context, Poll};

use futures::task::AtomicWaker;
use num_enum::{FromPrimitive, IntoPrimitive};
use tokio_stream::Stream;

/// A [`Stream`] wrapper that allows to pause, unpause and stop the stream.
///
/// Use [`state()`](Self::state) to obtain an `Arc` that points to the state of the stream,
/// then [`SharedState::set`] to update it.
pub struct ControlledStream<S: Stream> {
    inner: S,
    state: Arc<SharedStreamState>,
}

pub struct SharedStreamState {
    waker: AtomicWaker,
    state: AtomicU8,
}

/// State of a (managed) output task.
#[derive(Clone, Debug, PartialEq, Eq, Copy, IntoPrimitive, FromPrimitive)]
#[repr(u8)]
pub enum StreamState {
    Run,
    Pause,
    #[num_enum(default)]
    Stop,
}

impl SharedStreamState {
    /// Updates the state of the stream.
    pub fn set(&self, state: StreamState) {
        self.state.store(state as u8, Ordering::Relaxed);
        self.waker.wake();
    }
}

impl<S: Stream> ControlledStream<S> {
    /// Creates new controlled stream with the initial state [`StreamState::Run`].
    pub fn new(inner: S) -> Self {
        Self::with_initial_state(inner, StreamState::Run)
    }

    /// Wraps a stream in a new controlled stream with the given initial state.
    pub fn with_initial_state(inner: S, state: StreamState) -> Self {
        Self {
            inner,
            state: Arc::new(SharedStreamState {
                waker: AtomicWaker::new(),
                state: AtomicU8::new(state as u8),
            }),
        }
    }

    /// Returns a shared pointer to the shared stream state.
    ///
    /// Use this to update the state from any thread.
    pub fn state(&self) -> Arc<SharedStreamState> {
        self.state.clone()
    }
}

impl<S: Stream> Stream for ControlledStream<S> {
    type Item = S::Item;

    fn poll_next(self: std::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> std::task::Poll<Option<Self::Item>> {
        // pin-project the fields
        // SAFETY: we never use the fields directly after the unsafe block,
        // and we never move the data out of self.get_unchecked_mut().
        let (inner, state) = unsafe {
            let this = self.get_unchecked_mut();
            (Pin::new_unchecked(&mut this.inner), Pin::new_unchecked(&mut this.state))
        };

        let cur_state = state.state.load(Ordering::Relaxed).into();
        match cur_state {
            StreamState::Run => {
                match Stream::poll_next(inner, cx) {
                    Poll::Ready(item) => Poll::Ready(item),
                    Poll::Pending => {
                        // stream is not ready, register the waker for wakeup
                        state.waker.register(cx.waker());
                        Poll::Pending
                    }
                }
            }
            StreamState::Pause => {
                // pause, wake up on config change
                state.waker.register(cx.waker());
                Poll::Pending
            }
            StreamState::Stop => {
                // definitely stop
                Poll::Ready(None)
            }
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

#[cfg(test)]
mod tests {
    use std::{
        pin::pin,
        sync::atomic::Ordering,
        task::Poll,
        time::{Duration, Instant},
    };

    use tokio_stream::StreamExt;

    use crate::pipeline::util::stream::{ControlledStream, StreamState};

    #[tokio::test]
    async fn empty_controlled_stream() {
        let values: Vec<&'static str> = vec![];
        let stream = tokio_stream::iter(values.clone());
        let mut stream = ControlledStream::new(stream);
        assert_eq!(stream.next().await, None);
    }

    #[tokio::test]
    async fn single_controlled_stream() {
        let values: Vec<&'static str> = vec!["the one"];
        let stream = tokio_stream::iter(values.clone());
        let mut stream = ControlledStream::new(stream);
        assert_eq!(stream.next().await, Some("the one"));
        assert_eq!(stream.next().await, None);
    }

    #[tokio::test]
    async fn multi_controlled_stream() {
        let values: Vec<&'static str> = vec!["a", "b", "c"];
        let stream = tokio_stream::iter(values.clone());
        let mut stream = ControlledStream::new(stream);
        assert_eq!(stream.next().await, Some("a"));
        assert_eq!(stream.next().await, Some("b"));
        assert_eq!(stream.next().await, Some("c"));
        assert_eq!(stream.next().await, None);
    }

    #[tokio::test]
    async fn collect_controlled_stream() {
        {
            let values = vec!["0", "1", "2", "3", "4", "5"];
            let stream = tokio_stream::iter(values.clone());
            let stream = ControlledStream::new(stream);
            let collected: Vec<_> = stream.collect().await;
            assert_eq!(collected, values);
        }

        {
            let values = vec!["abc"];
            let stream = tokio_stream::iter(values.clone());
            let stream = ControlledStream::new(stream);
            let collected: Vec<_> = stream.collect().await;
            assert_eq!(collected, values);
        }

        {
            let values: Vec<&'static str> = vec![];
            let stream = tokio_stream::iter(values.clone());
            let stream = ControlledStream::new(stream);
            let collected: Vec<_> = stream.collect().await;
            assert_eq!(collected, values);
        }
    }

    #[tokio::test]
    async fn pause_empty_controlled_stream() {
        let mut stream = ControlledStream::new(tokio_stream::iter(Vec::<u8>::new()));
        let stream_state = stream.state();
        stream_state.set(StreamState::Pause);
        let next = pin!(stream.next());
        let polled = futures::poll!(next);
        assert_eq!(Poll::Pending, polled);

        stream_state.set(StreamState::Run);
        let next = pin!(stream.next());
        let polled = futures::poll!(next);
        assert_eq!(Poll::Ready(None), polled);
    }

    #[tokio::test]
    async fn pause_empty_controlled_stream_from_other_thread() {
        let mut stream = ControlledStream::new(tokio_stream::iter(Vec::<u8>::new()));
        let stream_state = stream.state();
        stream_state.set(StreamState::Pause);
        let next = pin!(stream.next());
        let polled = futures::poll!(next);
        assert_eq!(Poll::Pending, polled);

        let sc = stream_state.clone();
        let thread = std::thread::spawn(move || {
            sc.set(StreamState::Run);
        });
        thread.join().unwrap();

        let next = pin!(stream.next());
        let polled = futures::poll!(next);
        assert_eq!(Poll::Ready(None), polled);
    }

    #[tokio::test]
    async fn pause_empty_controlled_stream_from_other_thread2() {
        let mut stream = ControlledStream::new(tokio_stream::iter(Vec::<u8>::new()));
        let stream_state = stream.state();
        stream_state.set(StreamState::Pause);
        let next = pin!(stream.next());
        let polled = futures::poll!(next);
        assert_eq!(Poll::Pending, polled);

        let sc = stream_state.clone();
        let thread = std::thread::spawn(move || {
            sc.set(StreamState::Run);
        });

        let fut = stream.next();
        let next = fut.await;
        assert_eq!(None, next);

        thread.join().unwrap();
    }

    #[tokio::test]
    async fn pause_empty_controlled_stream_from_other_thread3() {
        let mut stream = ControlledStream::with_initial_state(tokio_stream::iter(Vec::<u8>::new()), StreamState::Pause);
        let stream_state = stream.state();
        assert_eq!(stream_state.state.load(Ordering::Relaxed), StreamState::Pause as u8);

        let next = pin!(stream.next());
        let polled = futures::poll!(next);
        assert_eq!(Poll::Pending, polled);

        let sc = stream_state.clone();
        let thread = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            println!("&sc: {:p}", &sc);
            println!("&sc.atomic_state: {:p}", &sc.state);
            sc.set(StreamState::Run);
        });

        let fut = stream.next();
        let next = fut.await;
        assert_eq!(None, next);

        thread.join().unwrap();
    }

    #[tokio::test]
    async fn pause_nonempty_controlled_stream() {
        let values = vec![1, 2, 3, 4, 5];
        let mut stream = Box::pin(ControlledStream::with_initial_state(
            tokio_stream::iter(values.clone()),
            StreamState::Pause,
        ));
        let stream_state = stream.state();

        println!("stream with state pause");
        let polled = futures::poll!(pin!(stream.next()));
        assert_eq!(Poll::Pending, polled);
        println!("is pending");

        let t0 = Instant::now();

        let state_clone = stream_state.clone();
        let thread = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            println!("state.set(Run)");
            state_clone.set(StreamState::Run);

            std::thread::sleep(Duration::from_millis(100));
            println!("state.set(Pause)");
            state_clone.set(StreamState::Pause);

            std::thread::sleep(Duration::from_millis(100));
            println!("state.set(Run)");
            state_clone.set(StreamState::Run);

            std::thread::sleep(Duration::from_millis(100));
            println!("state.set(Stop)");
            state_clone.set(StreamState::Stop);
        });

        // paused, then run
        println!("next().await...");
        let value = stream.next().await;
        let t1 = Instant::now();
        println!("=> {value:?} after {:?}", t1.duration_since(t0));
        assert!(t1.duration_since(t0) > Duration::from_millis(100));
        assert_eq!(Some(1), value);

        // run
        println!("next().await...");
        let t0 = Instant::now();
        let value = stream.next().await;
        let t1 = Instant::now();
        println!("=> {value:?} after {:?}", t1.duration_since(t0));
        assert!(t1.duration_since(t0) < Duration::from_millis(100));
        assert_eq!(Some(2), value);

        // run, then paused, then unpause
        std::thread::sleep(Duration::from_millis(105));
        println!("next().await...");
        let t0 = Instant::now();
        let last_value = stream.next().await;
        let t1 = Instant::now();
        println!("=> {last_value:?} after {:?}", t1.duration_since(t0));
        assert!(t1.duration_since(t0) > Duration::from_millis(90));
        assert_eq!(Some(3), last_value);

        // run
        println!("next().await...");
        let t0 = Instant::now();
        let value = stream.next().await;
        let t1 = Instant::now();
        println!("=> {value:?} after {:?}", t1.duration_since(t0));
        assert!(t1.duration_since(t0) < Duration::from_millis(100));
        assert_eq!(Some(4), value);

        // stop
        std::thread::sleep(Duration::from_millis(105));
        println!("next().await...");
        let t0 = Instant::now();
        let last_value = stream.next().await;
        let t1 = Instant::now();
        println!("=> {last_value:?} after {:?}", t1.duration_since(t0));
        assert!(t1.duration_since(t0) < Duration::from_millis(90));
        assert_eq!(None, last_value);

        thread.join().unwrap();
    }
}
