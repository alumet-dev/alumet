use std::time::Duration;

#[derive(Clone)]
pub struct ExponentialRetryPolicy {
    pub max_retrys: u16,
    pub initial_delay: Duration,
    pub max_delay: Duration,
    pub multiplier: u8,
}

pub struct RetryState {
    policy: ExponentialRetryPolicy,
    n_retrys: u16,
    delay: Duration,
}

impl RetryState {
    pub fn new(policy: &ExponentialRetryPolicy) -> Self {
        let initial_delay = policy.initial_delay.clone();
        Self {
            policy: policy.clone(),
            n_retrys: 0,
            delay: initial_delay,
        }
    }

    pub fn can_retry(&self) -> bool {
        self.n_retrys <= self.policy.max_retrys
    }

    fn count_and_increase_delay(&mut self) {
        self.n_retrys += 1;
        self.delay = (self.delay * self.policy.multiplier.into()).min(self.policy.max_delay);
    }

    pub async fn after_attempt(&mut self) {
        tokio::time::sleep(self.delay).await;
        self.count_and_increase_delay();
    }

    #[allow(unused)]
    pub fn blocking_after_attempt(&mut self) {
        std::thread::sleep(self.delay);
        self.count_and_increase_delay();
    }
}
