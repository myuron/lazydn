use std::time::{SystemTime, UNIX_EPOCH};

/// Source of the signed timestamp; swap it in tests to pin the time.
pub trait Clock: Send + Sync {
    /// Seconds since the Unix epoch.
    fn now_unix(&self) -> i64;
}

/// The OS clock, used in production.
pub struct SystemClock;

impl Clock for SystemClock {
    /// Seconds since the Unix epoch.
    fn now_unix(&self) -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64)
    }
}

/// A fixed value lets a test predict the signed timestamp.
pub struct FixedClock(pub i64);

impl Clock for FixedClock {
    /// Seconds since the Unix epoch.
    fn now_unix(&self) -> i64 {
        self.0
    }
}
