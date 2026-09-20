//! 时钟抽象。
//!
//! 状态机、需求评分、限流判断全都要「知道现在几点」。如果它们直接调用
//! [`Timestamp::now`]，测试就变成了灾难：想验证「连续工作 50 分钟会不会提醒」，
//! 就得真的等 50 分钟。
//!
//! 所以所有需要时间的地方都通过 [`Clock`] 拿时间。测试里换成 [`FakeClock`]，
//! 想跳到哪一秒就跳到哪一秒 —— 这也是架构原则 5「可测试」的具体落法。

use std::sync::atomic::{AtomicI64, Ordering};

use crate::time::Timestamp;

/// 提供「当前时刻」的东西。
pub trait Clock: Send + Sync {
    /// 现在几点（UTC）。
    fn now(&self) -> Timestamp;
}

/// 真实时钟 —— 生产环境用的就是它。
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        Timestamp::now()
    }
}

/// 可以手动拨动的假时钟，只在测试里用。
///
/// ```rust
/// use tacet_core::clock::{Clock, FakeClock};
/// use tacet_core::time::{Timestamp, MINUTE};
///
/// let clock = FakeClock::starting_at(Timestamp::from_millis(0));
/// clock.advance_millis(50 * MINUTE);
/// assert_eq!(clock.now().as_millis(), 50 * MINUTE);
/// ```
#[derive(Debug)]
pub struct FakeClock {
    /// 用原子类型是为了让 FakeClock 天然满足 `Send + Sync`，
    /// 从而能在多线程测试里当 `Arc<dyn Clock>` 用。
    now_ms: AtomicI64,
}

impl FakeClock {
    /// 从指定时刻开始走。
    pub fn starting_at(at: Timestamp) -> Self {
        Self {
            now_ms: AtomicI64::new(at.as_millis()),
        }
    }

    /// 往后拨一段时间。
    pub fn advance_millis(&self, ms: i64) {
        self.now_ms.fetch_add(ms, Ordering::SeqCst);
    }

    /// 往后拨若干分钟。
    pub fn advance_minutes(&self, minutes: i64) {
        self.advance_millis(minutes * crate::time::MINUTE);
    }

    /// 直接把时钟拨到某个时刻。
    pub fn set(&self, at: Timestamp) {
        self.now_ms.store(at.as_millis(), Ordering::SeqCst);
    }
}

impl Clock for FakeClock {
    fn now(&self) -> Timestamp {
        Timestamp::from_millis(self.now_ms.load(Ordering::SeqCst))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::MINUTE;

    #[test]
    fn 假时钟可以前进() {
        let clock = FakeClock::starting_at(Timestamp::from_millis(1_000));
        assert_eq!(clock.now().as_millis(), 1_000);

        clock.advance_minutes(30);
        assert_eq!(clock.now().as_millis(), 1_000 + 30 * MINUTE);
    }

    #[test]
    fn 假时钟可以直接拨到指定时刻() {
        let clock = FakeClock::starting_at(Timestamp::from_millis(0));
        clock.set(Timestamp::from_millis(123));
        assert_eq!(clock.now().as_millis(), 123);
    }

    #[test]
    fn 假时钟满足线程安全要求() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<FakeClock>();
    }
}
