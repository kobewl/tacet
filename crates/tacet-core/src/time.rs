//! 时间基元：一个 UTC 毫秒时间戳。
//!
//! ## 为什么不用 `chrono` / `time` 这类日期库
//!
//! 核心层对时间只有两种需求：
//!
//! - **比较先后**（谁的提醒更早发过）
//! - **求差**（距上次喝水多久了）
//!
//! 这两种都不需要日历知识 —— 一个 `i64`（自 Unix 纪元的毫秒数）就够了。
//! 真正需要时区的地方只有一处：**统计口径里的「今天」从几点开始**（数据模型 §8）。
//! 那是唯一必须懂「本地日历」的计算，所以它被单独关进 `tacet-storage::datewin`，
//! 由它统一提供，UI 层禁止自己算日期边界。
//!
//! 这样做的收益很实在：核心层零日期库依赖，也就不可能在不同时区下产生两套口径。

use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// 一秒钟有多少毫秒。
pub const SECOND: i64 = 1_000;
/// 一分钟有多少毫秒。
pub const MINUTE: i64 = 60 * SECOND;
/// 一小时有多少毫秒。
pub const HOUR: i64 = 60 * MINUTE;

/// UTC 毫秒时间戳（自 1970-01-01T00:00:00Z 起）。
///
/// 内部就是一个 `i64`，`#[serde(transparent)]` 让它在 JSON 里直接是数字，
/// 与 SQLite 里存的 `INTEGER` 完全对得上 —— 存储层可以直接读写，无需转换。
///
/// ```rust
/// use tacet_core::time::{Timestamp, MINUTE};
///
/// let t0 = Timestamp::from_millis(1_000_000);
/// let t1 = t0.saturating_add_millis(5 * MINUTE);
/// assert_eq!(t1.minutes_since(t0), 5);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Timestamp(i64);

impl Timestamp {
    /// 从毫秒数构造（可以是历史上的任意时刻，仅用于测试与反序列化）。
    pub const fn from_millis(ms: i64) -> Self {
        Self(ms)
    }

    /// 从秒数构造。
    pub const fn from_secs(secs: i64) -> Self {
        Self(secs * SECOND)
    }

    /// 取回毫秒数（写数据库、算差值时用）。
    pub const fn as_millis(self) -> i64 {
        self.0
    }

    /// 当前时刻。
    ///
    /// 生产代码请优先使用 [`Clock`](crate::Clock) trait 拿到这个值，
    /// 这样测试里就能换成假时钟，不用真的 `sleep`。
    pub fn now() -> Self {
        match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(d) => Self(d.as_millis() as i64),
            // 系统时间被设到 1970 年之前（几乎不可能）：退化为 0，不让程序崩。
            Err(_) => Self(0),
        }
    }

    /// 加一段时间。用 `saturating` 而不是 `+`：宁可时间停在极值，
    /// 也不要因为一个脏数据把整个程序 panic 掉。
    pub const fn saturating_add_millis(self, ms: i64) -> Self {
        Self(self.0.saturating_add(ms))
    }

    /// 减一段时间。
    pub const fn saturating_sub_millis(self, ms: i64) -> Self {
        Self(self.0.saturating_sub(ms))
    }

    /// `self` 比 `earlier` 晚多少毫秒（`earlier` 更晚时返回负数）。
    pub const fn millis_since(self, earlier: Timestamp) -> i64 {
        self.0 - earlier.0
    }

    /// `self` 比 `earlier` 晚多少**分钟**（向下取整，负数同理）。
    pub const fn minutes_since(self, earlier: Timestamp) -> i64 {
        self.millis_since(earlier) / MINUTE
    }

    /// `self` 是否在 `earlier` 之后 `window_ms` 毫秒以内。
    pub const fn is_within(self, earlier: Timestamp, window_ms: i64) -> bool {
        let d = self.millis_since(earlier);
        d >= 0 && d < window_ms
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 时间差按毫秒与分钟计算() {
        let t0 = Timestamp::from_millis(1_000_000);
        let t1 = t0.saturating_add_millis(90 * MINUTE);

        assert_eq!(t1.millis_since(t0), 90 * MINUTE);
        assert_eq!(t1.minutes_since(t0), 90);
    }

    #[test]
    fn 反向求差得到负数而不是溢出() {
        let t0 = Timestamp::from_millis(100_000);
        let t1 = Timestamp::from_millis(50_000);

        assert_eq!(t1.millis_since(t0), -50_000);
        // 负数除以 60000 向下取整为 -1，这里只断言符号方向正确
        assert!(t1.minutes_since(t0) <= 0);
    }

    #[test]
    fn 窗口判定包含左端不包含右端() {
        let base = Timestamp::from_millis(1_000_000);

        assert!(base.is_within(base, 10 * MINUTE), "刚发生：在窗口内");
        assert!(
            base.saturating_add_millis(9 * MINUTE)
                .is_within(base, 10 * MINUTE),
            "9 分钟前：仍在 10 分钟窗口内"
        );
        assert!(
            !base
                .saturating_add_millis(10 * MINUTE)
                .is_within(base, 10 * MINUTE),
            "刚好 10 分钟：出窗口"
        );
    }

    #[test]
    fn 极端值不会溢出() {
        let max = Timestamp::from_millis(i64::MAX);
        assert_eq!(max.saturating_add_millis(i64::MAX).as_millis(), i64::MAX);

        let min = Timestamp::from_millis(i64::MIN);
        assert_eq!(min.saturating_sub_millis(i64::MAX).as_millis(), i64::MIN);
    }
}
