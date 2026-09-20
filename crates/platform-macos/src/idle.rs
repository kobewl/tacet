//! 空闲检测 —— 基于 CoreGraphics。
//!
//! ## 为什么用 `CGEventSourceSecondsSinceLastEventType`
//!
//! 这是 macOS 上最轻量的空闲检测方式，比它更简单的只有
//! `IOHIDGetModifierLockState` 之类的特例 API。它的三个优点：
//!
//! 1. **不需要任何权限**。辅助功能、输入监控、屏幕录制统统不需要
//! 2. **不记录任何内容**。它只回答「距离上次输入过了多少秒」，
//!    无法得知用户按了哪个键、点了哪里 —— 这正是我们想要的
//! 3. **代价极低**。这是一个纯查询，没有回调、没有常驻开销
//!
//! ## 为什么同时保留 ActivityMonitor 和 IdleMonitor 两个实现
//!
//! 它们背后是同一个 API，但语义不同（见 `tacet-platform` 里两个 trait 的文档）。
//! 分开实现而不是互相委托，是因为将来很可能分道扬镳：
//! 比如「合盖算离开」只需要改 IdleMonitor，不影响输入活跃度的原始读数。

use tacet_platform::{PlatformError, Result};

// CoreGraphics 的「上一次输入距今多少秒」。
//
// `kCGAnyInputEventType` 覆盖了键盘、鼠标、触控板等所有输入方式，
// 用 `u32::MAX` 表示「任何输入类型」（CoreGraphics 头文件里写作 `~0`）。
//
// 返回值单位是**秒**，双精度浮点 —— 我们向下取整成整数秒，
// 因为业务上不需要亚秒精度（5 分钟的空闲阈值，差 0.3 秒没有意义）。
#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGEventSourceSecondsSinceLastEventType(state_id: u32, event_type: u32) -> f64;
}

/// `kCGEventSourceStateCombinedSessionState`：只统计当前登录会话的输入。
///
/// 也就是说，切换用户之后另一个用户的输入不会影响本用户的计时。
/// 另一个候选是 `kCGEventSourceStateHIDSystemState`（整个系统的输入），
/// 那个在多用户场景下会串味。
const COMBINED_SESSION_STATE: u32 = 0;

/// `kCGAnyInputEventType`：任何输入事件。
///
/// 这个值在 CoreGraphics 头文件里写作 `~0`（即全 1）。
const ANY_INPUT_EVENT_TYPE: u32 = u32::MAX;

/// 读取空闲秒数。
fn idle_seconds_raw() -> Result<u32> {
    // 这个调用不会失败（没有错误返回），但返回值可能是负数：
    // 极少数情况下系统时钟被调整会让它短暂变成 -0.0001 之类。
    let seconds = unsafe {
        CGEventSourceSecondsSinceLastEventType(COMBINED_SESSION_STATE, ANY_INPUT_EVENT_TYPE)
    };

    if !seconds.is_finite() {
        // 理论上不会发生，但真发生时要如实报告而不是返回 0
        // —— 返回 0 意味着「用户刚刚在输入」，会让状态机以为人一直在。
        return Err(PlatformError::Unexpected(
            "CoreGraphics 返回了非有限的空闲时长".to_string(),
        ));
    }

    if seconds < 0.0 {
        // 时钟微调导致的轻微负值：当作「刚刚有输入」处理。
        // 这里不用返回错误 —— 它是一个已知的、无害的边界情况。
        return Ok(0);
    }

    // 上限保护：u32 秒 ≈ 136 年，实际不可能达到，但避免转换时的未定义行为。
    Ok(seconds.min(u32::MAX as f64) as u32)
}

/// 输入活跃度监测。
#[derive(Debug, Clone, Copy, Default)]
pub struct CgActivityMonitor {
    _private: (),
}

impl CgActivityMonitor {
    /// 新建（无状态）。
    pub fn new() -> Self {
        Self { _private: () }
    }
}

impl tacet_platform::ActivityMonitor for CgActivityMonitor {
    fn seconds_since_last_input(&self) -> Result<u32> {
        idle_seconds_raw()
    }
}

/// 空闲状态监测。
#[derive(Debug, Clone, Copy, Default)]
pub struct CgIdleMonitor {
    _private: (),
}

impl CgIdleMonitor {
    /// 新建（无状态）。
    pub fn new() -> Self {
        Self { _private: () }
    }
}

impl tacet_platform::IdleMonitor for CgIdleMonitor {
    fn is_idle(&self, threshold_seconds: u32) -> Result<bool> {
        Ok(idle_seconds_raw()? >= threshold_seconds)
    }

    fn idle_seconds(&self) -> Result<u32> {
        idle_seconds_raw()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tacet_platform::{ActivityMonitor, IdleMonitor};

    #[test]
    fn 能读到空闲秒数() {
        // 这个测试在 CI 上要能跑过去：真实调用应当成功返回一个数。
        let monitor = CgIdleMonitor::new();
        let seconds = monitor.idle_seconds().expect("应当能读到空闲时长");

        // 只要能正常读到就行，不断言具体值（CI 机器上可能有人在敲键盘，
        // 也可能完全空闲）。
        assert!(seconds < 60 * 60 * 24 * 365, "空闲时长不可能是几十年");
    }

    #[test]
    fn 阈值判定与读数一致() {
        let monitor = CgIdleMonitor::new();
        let seconds = monitor.idle_seconds().expect("应当能读到");

        // 用刚刚读到的值当阈值：结果必然为真（读数和判定之间最多差几毫秒）
        assert!(monitor.is_idle(seconds).expect("应当能判定"));

        // 用一个远超当前读数的阈值：必然为假
        if seconds < u32::MAX - 1 {
            assert!(!monitor.is_idle(seconds + 1).expect("应当能判定"));
        }
    }

    #[test]
    fn 活跃度与空闲度读数一致() {
        let activity = CgActivityMonitor::new();
        let idle = CgIdleMonitor::new();

        let a = activity.seconds_since_last_input().expect("活跃度");
        let b = idle.idle_seconds().expect("空闲度");

        // 两者背后是同一个 API，连续两次调用之间差了不过毫秒级，
        // 所以读数应当相等或相差 1 秒以内。
        let diff = a.abs_diff(b);
        assert!(diff <= 1, "两个读数相差过大：{a} vs {b}");
    }

    #[test]
    fn 监测器可以跨线程共享() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<CgIdleMonitor>();
        assert_send_sync::<CgActivityMonitor>();
    }
}
