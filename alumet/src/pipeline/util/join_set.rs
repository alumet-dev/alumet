//! A JoinSet with additional features.

use std::future::Future;

use tokio::{
    runtime::Handle,
    sync::Notify,
    task::{AbortHandle, JoinError},
};

/// A collection of tasks spawned on a Tokio runtime.
///
/// This is a wrapper around [`tokio::task::JoinSet`] with additional notification capabilities.
/// In particular, it provides the [`join_next_completion`](JoinSet::join_next_completion) method,
/// wich we needed to implement the polling of failed tasks in the control loop properly.
///
/// If, in the future, tokio improves its API to allow the registration of custom panic
/// handlers and error handlers, it may no longer be necessary.
pub struct JoinSet<T> {
    inner: tokio::task::JoinSet<T>,
    non_empty_notify: Notify,
}

impl<T> JoinSet<T> {
    /// Creates a new JoinSet.
    pub fn new() -> Self {
        Self {
            inner: tokio::task::JoinSet::new(),
            non_empty_notify: Notify::new(),
        }
    }

    /// Returns the number of tasks currently in the `JoinSet`.
    #[allow(unused)]
    pub fn len(&self) -> usize {
        self.inner.len()
    }

    /// Returns whether the `JoinSet` is empty.
    #[allow(unused)]
    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }
}

impl<T: 'static> JoinSet<T> {
    /// Spawns the provided task on the runtime and store it in this set.
    ///
    /// See [`tokio::task::JoinSet::spawn_on`].
    pub fn spawn_on<F>(&mut self, task: F, handle: &Handle) -> AbortHandle
    where
        F: Future<Output = T> + Send + 'static,
        T: Send,
    {
        let was_empty = self.inner.is_empty();
        let res = self.inner.spawn_on(task, handle);
        if was_empty {
            self.non_empty_notify.notify_waiters();
        }
        res
    }

    /// Waits until one of the tasks completes and returns its output.
    ///
    /// ## Empty set
    /// Unlike [`tokio::task::JoinSet::join_next`], `join_next_completion` never returns None.
    ///
    /// If the set is empty, wait for a new task to be added to the set, then wait for the completion of a task.
    /// Therefore, **it may never complete if the set remains empty**.
    ///
    /// You should probably use this method in [`tokio::select!`].
    pub async fn join_next_completion(&mut self) -> Result<T, JoinError> {
        let mut task_result = self.inner.join_next().await;
        loop {
            match task_result {
                Some(res) => return res,
                None => {
                    // the JoinSet is empty, wait for a new task to come and finish
                    self.non_empty_notify.notified().await;
                    task_result = self.inner.join_next().await;
                }
            }
        }
    }

    /// Waits until one of the tasks in the set completes and returns its output.
    ///
    /// Returns `None` if the set is empty.
    ///
    /// See [`tokio::task::JoinSet::join_next`].
    pub async fn join_next(&mut self) -> Option<Result<T, JoinError>> {
        self.inner.join_next().await
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use tokio::time::timeout;

    use super::JoinSet;

    #[test]
    fn test() {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_time()
            .worker_threads(2)
            .build()
            .unwrap();
        let mut set: JoinSet<u32> = JoinSet::new();

        // One task
        set.spawn_on(async { 2 }, rt.handle());
        assert!(!set.is_empty());
        assert_eq!(1, set.len());
        let res = rt
            .block_on(async { timeout(Duration::from_secs(1), set.join_next_completion()).await })
            .unwrap()
            .unwrap();
        assert_eq!(2, res);

        // No task => join_next_completion does not return
        assert!(set.is_empty());
        assert_eq!(0, set.len());
        let should_timeout =
            rt.block_on(async { timeout(Duration::from_millis(100), set.join_next_completion()).await });
        assert!(should_timeout.is_err());

        // One task that completes while `join_next_completion` is waiting
        set.spawn_on(
            async {
                tokio::time::sleep(Duration::from_millis(50)).await;
                123
            },
            rt.handle(),
        );
        let should_not_timeout =
            rt.block_on(async { timeout(Duration::from_secs(1), set.join_next_completion()).await });
        assert_eq!(123, should_not_timeout.unwrap().unwrap());
    }
    
    // #[test]
    // fn tricky_test() {
    //     let rt = tokio::runtime::Builder::new_multi_thread()
    //         .enable_time()
    //         .worker_threads(2)
    //         .build()
    //         .unwrap();
    //     let mut set: Arc<JoinSet<u32>> = Arc::new(JoinSet::new());

    //     // One task that is spawned while `join_next_completion` is waiting
    //     set.spawn_on(async {
    //         timeout(Duration::from_secs(1), set.join_next_completion()).await.expect("timeout!");
    //         0
    //     }, rt.handle());
    // }
}
