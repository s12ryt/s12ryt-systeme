//! 固定窗口速率限制器 — 每來源 (unit/identifier) 獨立限流。
//!
//! journald 語義: 窗口內最多 burst 條,窗口滾動後計數歸零。

/// 固定窗口速率限制器。
///
/// 窗口起點為首次 (或上次滾動) 的時間戳,窗口長度 `interval_ms`。
#[derive(Debug, Clone)]
pub struct RateLimiter {
    burst: usize,
    interval_ms: u64,
    window_start_ms: u64,
    used: usize,
}

impl RateLimiter {
    /// 建立限制器:每 `interval_ms` 毫秒窗口最多允許 `burst` 次。
    pub fn new(burst: usize, interval_ms: u64) -> Self {
        Self {
            burst,
            interval_ms,
            window_start_ms: 0,
            used: 0,
        }
    }

    /// 嘗試在 `now_ms` 時刻通過一次。
    ///
    /// 若當前時間已超出窗口 (`now >= window_start + interval`) 則滾動窗口並重置計數;
    /// 否則窗口內剩餘額度時放行。
    pub fn allow(&mut self, now_ms: u64) -> bool {
        if now_ms >= self.window_start_ms.saturating_add(self.interval_ms) {
            self.window_start_ms = now_ms;
            self.used = 0;
        }
        if self.used < self.burst {
            self.used += 1;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RateLimiter;

    #[test]
    fn zero_burst_rejects_everything() {
        let mut rl = RateLimiter::new(0, 1_000);
        assert!(!rl.allow(0));
        assert!(!rl.allow(5_000));
    }
}
