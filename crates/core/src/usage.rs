//! 用量事件幂等键契约（plan.md 1.00i）。
//!
//! 用量事件必须带稳定幂等键，供第六阶段计费去重：
//! - 同一事件的重试用**同一键**（不会重复计费）。
//! - 不同采样区间用**不同键**（各自入账）。

/// 用量事件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageEvent {
    pub resource_id: String,
    pub meter: String,
    /// 采样区间起止（unix 秒），决定键的区间维度。
    pub window_start: u64,
    pub window_end: u64,
    pub quantity: u64,
}

impl UsageEvent {
    /// 稳定幂等键：由资源、计量项和采样区间决定，与重试次数无关。
    pub fn idempotency_key(&self) -> String {
        format!(
            "{}|{}|{}-{}",
            self.resource_id, self.meter, self.window_start, self.window_end
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(start: u64, end: u64, q: u64) -> UsageEvent {
        UsageEvent {
            resource_id: "ws-1".into(),
            meter: "cpu_seconds".into(),
            window_start: start,
            window_end: end,
            quantity: q,
        }
    }

    // 1.00i: 同事件重试（即便 quantity 重算一致）键相同。
    #[test]
    fn retry_has_same_key() {
        assert_eq!(
            ev(0, 60, 60).idempotency_key(),
            ev(0, 60, 60).idempotency_key()
        );
    }

    // 1.00i: 不同采样区间键不同。
    #[test]
    fn different_window_has_different_key() {
        assert_ne!(
            ev(0, 60, 60).idempotency_key(),
            ev(60, 120, 60).idempotency_key()
        );
    }
}
