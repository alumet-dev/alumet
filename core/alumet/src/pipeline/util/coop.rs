//! Cooperative async scheduling for sources.

use std::{
    pin::{Pin, pin},
    task::{Context, Poll},
};

use tokio::sync::Notify;

use crate::pipeline::elements::source::trigger::{Trigger, TriggerLoopParams, TriggerReason};

/// Maximum number of times that a trigger can be immediately ready with the same TriggerReason.
const BUDGET_SAME_TRIGGER: u32 = 2;

/// Maximum number of times that a trigger can be immediately ready for any reason.
const BUDGET_ANY_TRIGGER: u32 = 5;

/// Cooperative wrapper around a source [`Trigger`].
///
/// Avoids starvation in case the same trigger is immediately ready too many times in a row.
/// Unlike [`tokio::task::coop`], `TriggerCoop` has several distinct budgets and a very small limit.
pub(crate) struct TriggerCoop<'i> {
    inner: Trigger,
    interrupt: &'i Notify,
    budget: TriggerBudget,
    previously_ready: Option<TriggerReason>,
}

pub(crate) struct TriggerCoopNext<'a> {
    inner: &'a mut Trigger,
    interrupt: &'a Notify,
    budget: &'a mut TriggerBudget,
    previously_ready: &'a mut Option<TriggerReason>,
}

#[derive(Debug)]
struct TriggerBudget {
    triggered: u32,
    interrupted: u32,
    any: u32,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RemainingBudget {
    Good,
    Exhausted,
}

impl TriggerBudget {
    fn consume(&mut self, reason: TriggerReason) -> RemainingBudget {
        // decrement the "any" budget
        if self.any > 0 {
            self.any -= 1;
        } else {
            return RemainingBudget::Exhausted;
        }

        // decrement the budget that corresponds to the trigger reason,
        // and reset the budget of the other reason
        match reason {
            TriggerReason::Triggered => {
                self.interrupted = BUDGET_SAME_TRIGGER;
                if self.triggered > 0 {
                    self.triggered -= 1;
                    RemainingBudget::Good
                } else {
                    RemainingBudget::Exhausted
                }
            }
            TriggerReason::Interrupted => {
                self.triggered = BUDGET_SAME_TRIGGER;
                if self.interrupted > 0 {
                    self.interrupted -= 1;
                    RemainingBudget::Good
                } else {
                    RemainingBudget::Exhausted
                }
            }
        }
    }

    fn reset(&mut self) {
        *self = Default::default();
    }
}

impl Default for TriggerBudget {
    fn default() -> Self {
        Self {
            triggered: BUDGET_SAME_TRIGGER,
            interrupted: BUDGET_SAME_TRIGGER,
            any: BUDGET_ANY_TRIGGER,
        }
    }
}

impl<'i> TriggerCoop<'i> {
    pub fn new(trigger: Trigger, interrupt: &'i Notify) -> Self {
        Self {
            inner: trigger,
            interrupt,
            budget: TriggerBudget::default(),
            previously_ready: None,
        }
    }

    pub fn next(&mut self) -> TriggerCoopNext<'_> {
        TriggerCoopNext {
            inner: &mut self.inner,
            interrupt: self.interrupt,
            budget: &mut self.budget,
            previously_ready: &mut self.previously_ready,
        }
    }

    /// Replaces the inner `Trigger`, but does not reset the budget.
    pub fn replace_trigger(&mut self, new_trigger: Trigger) {
        self.inner = new_trigger;
    }

    pub fn params(&self) -> &TriggerLoopParams {
        &self.inner.config
    }
}

impl<'a> Future for TriggerCoopNext<'a> {
    type Output = anyhow::Result<TriggerReason>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // SAFETY: we never use the fields directly after this block,
        // and we never move the data out of `this`.
        let (mut inner, interrupt, budget, previously_ready) = unsafe {
            let this = self.get_unchecked_mut();
            (
                Pin::new_unchecked(&mut this.inner),
                this.interrupt,
                &mut this.budget,
                &mut this.previously_ready,
            )
        };

        // pin the future so that we can poll() it
        let trigger_next = pin!(inner.next(interrupt));

        // If we were ready last time, but throttled by the budget, we are ready now.
        if let Some(res) = previously_ready.take() {
            budget.consume(res);
            return Poll::Ready(Ok(res));
        }

        // poll the trigger and see what happens
        match trigger_next.poll(cx) {
            Poll::Ready(err @ Err(_)) => {
                // propagate the error immediately
                Poll::Ready(err)
            }
            Poll::Ready(Ok(reason)) => {
                match budget.consume(reason) {
                    RemainingBudget::Good => {
                        // We can proceed normally.
                        Poll::Ready(Ok(reason))
                    }
                    RemainingBudget::Exhausted => {
                        // The trigger has been ready too many times in a row,
                        // return back to the async runtime and come back to this
                        // future later, so that other futures (e.g. other Alumet sources) can run.

                        // Important: make sure that this trigger will be ready next time, with the same result.
                        // For example, if the underlying trigger was triggered by Notify::notify_one(), this notification
                        // has been consumed by trigger_next.poll() above, and the future will never progress if we don't
                        // remember that it was ready.
                        **previously_ready = Some(reason);

                        budget.reset();
                        tokio_yield(cx).map(|_| Ok(reason))
                    }
                }
            }
            Poll::Pending => {
                // the trigger is not ready yet (typically because there's no incoming config change and the polling interval has not expired yet)
                budget.reset();
                Poll::Pending
            }
        }
    }
}

fn tokio_yield(cx: &mut Context<'_>) -> Poll<()> {
    let y = std::pin::pin!(tokio::task::yield_now());
    y.poll(cx)
}

#[cfg(test)]
mod tests {
    use super::{BUDGET_ANY_TRIGGER, BUDGET_SAME_TRIGGER, Trigger, TriggerCoop, TriggerReason};
    use std::pin::pin;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::task::{Context, Poll, Waker};
    use tokio::sync::Notify;

    #[test]
    fn cooop_triggered_same_manual() {
        let mut cx = Context::from_waker(Waker::noop());
        let interrupt = Notify::new();
        let trigger = Trigger::new_manual(false);
        let manual_trigger = trigger.manual_trigger().unwrap();
        let mut t = TriggerCoop::new(trigger, &interrupt);

        // before the trigger action, we should not be ready
        assert!(
            pin!(t.next()).poll(&mut cx).is_pending(),
            "should be pending before it is triggered"
        );

        // until we reach the budget, we can be ready multiple times in a row
        for _ in 0..BUDGET_SAME_TRIGGER {
            manual_trigger.trigger_now(); // trigger
            let next = pin!(t.next());
            let res = next.poll(&mut cx);
            assert!(
                matches!(res, Poll::Ready(Ok(TriggerReason::Triggered))),
                "should be ready immediately after manual trigger, but was {res:?}"
            );
        }

        // the budget has now expired
        manual_trigger.trigger_now();
        assert!(
            pin!(t.next()).poll(&mut cx).is_pending(),
            "budget has expired, should be pending (cool down)"
        );

        // the budget has been reset
        for _ in 0..BUDGET_SAME_TRIGGER {
            manual_trigger.trigger_now(); // trigger
            let next = pin!(t.next());
            let res = next.poll(&mut cx);
            assert!(
                matches!(res, Poll::Ready(Ok(TriggerReason::Triggered))),
                "should be ready immediately after manual trigger, but was {res:?}"
            );
        }

        // the budget has expired again
        manual_trigger.trigger_now();
        assert!(
            pin!(t.next()).poll(&mut cx).is_pending(),
            "budget has expired, should be pending (cool down)"
        );
    }

    #[test]
    fn cooop_triggered_same_interrupt() {
        let mut cx = Context::from_waker(Waker::noop());
        let interrupt = Notify::new();
        let trigger = Trigger::new_manual(true);
        let mut t = TriggerCoop::new(trigger, &interrupt);

        // before the trigger action, we should not be ready
        assert!(
            pin!(t.next()).poll(&mut cx).is_pending(),
            "should be pending before it is triggered"
        );

        // until we reach the budget, we can be ready multiple times in a row
        for _ in 0..BUDGET_SAME_TRIGGER {
            interrupt.notify_one(); // trigger
            let next = pin!(t.next());
            let res = next.poll(&mut cx);
            assert!(
                matches!(res, Poll::Ready(Ok(TriggerReason::Interrupted))),
                "should be ready immediately after interrupt, but was {res:?}"
            );
        }

        // the budget has now expired
        interrupt.notify_one();
        assert!(
            pin!(t.next()).poll(&mut cx).is_pending(),
            "budget has expired, should be pending (cool down)"
        );

        // the budget has been reset
        for _ in 0..BUDGET_SAME_TRIGGER {
            interrupt.notify_one(); // trigger
            let next = pin!(t.next());
            let res = next.poll(&mut cx);
            assert!(
                matches!(res, Poll::Ready(Ok(TriggerReason::Interrupted))),
                "should be ready immediately after interrupt, but was {res:?}"
            );
        }

        // the budget has expired again
        interrupt.notify_one();
        assert!(
            pin!(t.next()).poll(&mut cx).is_pending(),
            "budget has expired, should be pending (cool down)"
        );
    }

    #[test]
    fn cooop_triggered_any() {
        let mut cx = Context::from_waker(Waker::noop());
        let interrupt = Notify::new();
        let trigger = Trigger::new_manual(true);
        let manual_trigger = trigger.manual_trigger().unwrap();
        let mut t = TriggerCoop::new(trigger, &interrupt);

        // before the trigger action, we should not be ready
        assert!(
            pin!(t.next()).poll(&mut cx).is_pending(),
            "should be pending before it is triggered"
        );

        // until we reach the budget, we can be ready multiple times in a row
        for i in 0..BUDGET_ANY_TRIGGER {
            // manual trigger or interruption
            if i % 2 == 0 {
                manual_trigger.trigger_now();
            } else {
                interrupt.notify_one();
            }
            // poll the future
            let next = pin!(t.next());
            let res = next.poll(&mut cx);
            assert!(
                matches!(res, Poll::Ready(Ok(_))),
                "should be ready immediately after interrupt, but was {res:?}"
            );
        }

        // the budget has now expired
        interrupt.notify_one();
        assert!(
            pin!(t.next()).poll(&mut cx).is_pending(),
            "budget has expired, should be pending (cool down)"
        );

        // the budget has been reset
        interrupt.notify_one();
        assert!(pin!(t.next()).poll(&mut cx).is_ready(), "budget should have been reset");

        manual_trigger.trigger_now();
        assert!(pin!(t.next()).poll(&mut cx).is_ready(), "budget should have been reset");
    }

    #[test]
    fn coop_in_situ() {
        // builder for a busy-loop task
        async fn busy_loop(label: &'static str, counter: Arc<AtomicU32>, limit: u32) -> u32 {
            let interrupt = Notify::new();
            let trigger = Trigger::new_manual(true);
            let manual = trigger.manual_trigger().unwrap();
            let mut trigger = TriggerCoop::new(trigger, &interrupt);
            loop {
                println!("{label}");
                manual.trigger_now();
                let next = trigger.next().await;
                assert_eq!(next.unwrap(), TriggerReason::Triggered, "bad trigger reason");
                let v = counter.fetch_add(1, Ordering::Relaxed) + 1;
                println!("{label}: {v}");
                if v >= limit {
                    break;
                }
            }
            counter.load(Ordering::Relaxed)
        }

        // counters
        let counter_a = Arc::new(AtomicU32::new(0));
        let counter_b = Arc::new(AtomicU32::new(0));
        let counter_c = Arc::new(AtomicU32::new(0));

        // tasks
        const LIMIT_MAX: u32 = 64;
        let task_a = busy_loop("a", Arc::clone(&counter_a), LIMIT_MAX);
        let task_b = busy_loop("b", Arc::clone(&counter_b), 2);
        let task_c = busy_loop("c", Arc::clone(&counter_c), 4);
        // let's run all the tasks
        let rt = tokio::runtime::Builder::new_current_thread().build().unwrap();
        let handle_a = rt.spawn(task_a);
        let handle_b = rt.spawn(task_b);
        let handle_c = rt.spawn(task_c);

        rt.block_on(async {
            let res_b = handle_b.await.unwrap();
            let res_c = handle_c.await.unwrap();
            let res_a = counter_a.load(Ordering::Relaxed);
            assert!(
                res_a < LIMIT_MAX,
                "task_a should let the other tasks run before reaching its limit"
            );
            // stop a now
            counter_a.store(LIMIT_MAX, Ordering::Relaxed);
            let res_a = handle_a.await.unwrap();
        });
    }
}
