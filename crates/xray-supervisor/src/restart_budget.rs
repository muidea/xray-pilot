use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

#[derive(Default)]
pub(super) struct RestartBudget {
    pub(super) attempts: VecDeque<Instant>,
}
impl RestartBudget {
    pub(super) fn available(&mut self, now: Instant) -> bool {
        while self
            .attempts
            .front()
            .is_some_and(|t| now.saturating_duration_since(*t) >= Duration::from_secs(60))
        {
            self.attempts.pop_front();
        }
        self.attempts.len() < 3
    }
    pub(super) fn charge(&mut self, now: Instant) -> bool {
        if !self.available(now) {
            return false;
        }
        self.attempts.push_back(now);
        true
    }
    pub(super) fn delay(&self) -> Duration {
        Duration::from_secs(2 << self.attempts.len())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn window_expires_per_attempt_and_rejected_attempts_do_not_extend_it() {
        let mut budget = RestartBudget::default();
        let now = Instant::now();
        for seconds in [0, 10, 20] {
            assert!(budget.charge(now + Duration::from_secs(seconds)));
        }
        assert!(!budget.charge(now + Duration::from_secs(59)));
        assert!(budget.charge(now + Duration::from_secs(60)));
        assert!(!budget.charge(now + Duration::from_secs(69)));
        assert!(budget.charge(now + Duration::from_secs(70)));
    }
}
