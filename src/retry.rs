use std::time::Duration;

#[derive(Clone, Debug)]
pub struct RetryPolicy {
    pub max_attempts: Option<u32>,
    pub initial_delay: Duration,
    pub backoff: f64,
    pub max_delay: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: None,
            initial_delay: Duration::from_millis(1000),
            backoff: 2.0,
            max_delay: Duration::from_millis(30000),
        }
    }
}

impl RetryPolicy {
    pub fn delay_for_attempt(&self, attempt: u32) -> Duration {
        let delay_ms = self.initial_delay.as_millis() as f64 * self.backoff.powi(attempt as i32);

        Duration::from_millis(delay_ms.min(self.max_delay.as_millis() as f64) as u64)
    }

    pub fn should_retry(&self, attempt: u32) -> bool {
        match self.max_attempts {
            Some(max) => attempt < max,
            None => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backoff_calculation() {
        let policy = RetryPolicy::default();

        assert_eq!(policy.delay_for_attempt(0), Duration::from_millis(1000));
        assert_eq!(policy.delay_for_attempt(1), Duration::from_millis(2000));
        assert_eq!(policy.delay_for_attempt(2), Duration::from_millis(4000));
        assert_eq!(policy.delay_for_attempt(3), Duration::from_millis(8000));
    }

    #[test]
    fn test_max_delay_cap() {
        let policy = RetryPolicy::default();

        assert_eq!(policy.delay_for_attempt(10), Duration::from_millis(30000));
    }

    #[test]
    fn test_max_attempts() {
        let policy = RetryPolicy {
            max_attempts: Some(3),
            ..Default::default()
        };

        assert!(policy.should_retry(0));
        assert!(policy.should_retry(1));
        assert!(policy.should_retry(2));
        assert!(!policy.should_retry(3));
    }

    #[test]
    fn test_infinite_retry() {
        let policy = RetryPolicy::default();

        assert!(policy.should_retry(0));
        assert!(policy.should_retry(100));
        assert!(policy.should_retry(1000));
    }
}
