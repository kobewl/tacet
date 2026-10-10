//! 会议跟踪器 —— 把「麦克风此刻有没有被占用」这个瞬时信号，
//! 变成「是不是该把提醒往后放一放」这个**稳定**的判断。
//!
//! ## 为什么需要它，而不是直接读麦克风
//!
//! 参考 LookAway（「会议 / 通话中自动暂停」）和 Stretchly 的 DND 联动：
//! 在会上被整屏盖住，是这类工具最典型的社死场景。但把原始信号直接拿来
//! 做决策会出三类问题：
//!
//! 1. **抖动**。Teams、飞书这类应用静音 / 取消静音时会短暂释放麦克风，
//!    原始信号会 true→false→true 地跳。直接用就是会议中途弹提醒。
//! 2. **会刚散就弹**。会议结束的那一刻，人通常在收尾、记待办、切回工作；
//!    此时立刻整屏提醒并不比会中友好多少。路线图 v0.3 要求「会后 3~5 分钟」。
//! 3. **永久失灵**。某些后台程序（降噪虚拟声卡、语音输入、录屏软件）会
//!    长期占着麦克风。如果「麦克风被占用 = 不提醒」没有上限，
//!    健康提醒就会**悄无声息地永远失效** —— 这是比「会上弹窗」更严重的故障，
//!    因为用户完全察觉不到。
//!
//! 所以这里用一个小状态机一次性解决三件事：
//!
//! ```text
//!   麦克风占用 ──► InCall ──(释放)──► Settling ──(满 3 分钟没再占用)──► Clear
//!                    │                    │
//!                    └──(持续推迟满 90 分钟)──► GaveUp ──(释放满 3 分钟)──► Clear
//! ```
//!
//! - `Settling` 同时承担**防抖**和**会后缓冲**：释放后 3 分钟内再次占用，
//!   仍算同一场会议；3 分钟都安静，才认为散会了。
//! - `GaveUp` 是**保险丝**：同一次占用推迟满 90 分钟就不再推迟，
//!   提醒照常发出。宁可会中弹一次，也不让核心功能被一个后台进程静默废掉。
//!
//! ## 隐私
//!
//! 本模块只接触一个布尔量（占用 / 未占用）和一个「前台是不是会议类应用」
//! 的布尔量。它不录音、不读音频、不读窗口标题。

use tacet_core::time::{Timestamp, MINUTE};

/// 会议结束后继续静默的缓冲时间。
///
/// 兼作防抖窗口：释放麦克风后这么久都没再占用，才算散会。
pub const SETTLE_MS: i64 = 3 * MINUTE;

/// 单次占用最多推迟多久。
///
/// 超过它仍在占用，多半不是会议而是某个后台程序占着麦克风，
/// 此时必须放行提醒（见模块说明的第 3 点）。
pub const MAX_DEFER_MS: i64 = 90 * MINUTE;

/// 会议所处的阶段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MeetingPhase {
    /// 没有会议迹象。
    Clear,
    /// 麦克风正被占用（会议 / 通话进行中）。
    InCall,
    /// 刚释放麦克风，处于会后缓冲期。
    Settling,
    /// 占用持续太久，已放弃推迟（保险丝熔断），提醒照常发出。
    GaveUp,
}

impl MeetingPhase {
    /// 这个阶段要不要把提醒往后推。
    pub const fn defers_reminders(self) -> bool {
        matches!(self, MeetingPhase::InCall | MeetingPhase::Settling)
    }

    /// 给界面的稳定字符串；不需要推迟时为 `None`。
    pub const fn ui_label(self) -> Option<&'static str> {
        match self {
            MeetingPhase::InCall => Some("in_call"),
            MeetingPhase::Settling => Some("settling"),
            MeetingPhase::Clear | MeetingPhase::GaveUp => None,
        }
    }
}

/// 一次观测的结论。
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MeetingVerdict {
    /// 当前阶段。
    pub phase: MeetingPhase,
    /// 「此刻在开会」的概率估计（0~1），写进上下文快照供解释与诊断。
    pub probability: f64,
}

/// 会议跟踪器。
///
/// 有状态，但状态只有两个时间点；一次睡眠唤醒之后自然过期，不需要显式重置。
#[derive(Debug, Clone, Default)]
pub struct MeetingTracker {
    /// 本次占用「发作」的起点；`None` 表示当前没有会议迹象。
    episode_start: Option<Timestamp>,
    /// 最近一次观测到麦克风被占用的时刻。
    last_active_at: Option<Timestamp>,
}

impl MeetingTracker {
    /// 新建一个跟踪器。
    pub fn new() -> Self {
        Self::default()
    }

    /// 喂入一次观测。
    ///
    /// - `mic_in_use`：麦克风是否被占用；`None` 表示这次**读不到**。
    ///   读不到按「没占用」处理 —— 但因为有会后缓冲，
    ///   一两次读取失败不会让一场进行中的会议被误判为散会。
    /// - `meeting_app_foreground`：前台是不是会议类应用，只用来修正概率。
    pub fn observe(
        &mut self,
        now: Timestamp,
        mic_in_use: Option<bool>,
        meeting_app_foreground: bool,
    ) -> MeetingVerdict {
        let active = mic_in_use == Some(true);

        if active {
            self.last_active_at = Some(now);
            if self.episode_start.is_none() {
                self.episode_start = Some(now);
            }
        } else if let (Some(_), Some(last)) = (self.episode_start, self.last_active_at) {
            // 安静满一个缓冲期：这场会议结束了
            if now.millis_since(last) >= SETTLE_MS {
                self.episode_start = None;
            }
        }

        let phase = match self.episode_start {
            None => MeetingPhase::Clear,
            Some(start) => {
                if now.millis_since(start) >= MAX_DEFER_MS {
                    MeetingPhase::GaveUp
                } else if active {
                    MeetingPhase::InCall
                } else {
                    MeetingPhase::Settling
                }
            }
        };

        let probability = match (phase, active, meeting_app_foreground) {
            (MeetingPhase::InCall, _, true) => 1.0,
            (MeetingPhase::InCall, _, false) => 0.9,
            (MeetingPhase::Settling, _, _) => 0.5,
            // 麦克风没占用，只是开着会议软件：不足以认定在开会
            (MeetingPhase::Clear, false, true) => 0.3,
            // 保险丝熔断后的占用：多半不是会议
            (MeetingPhase::GaveUp, true, _) => 0.2,
            _ => 0.0,
        };

        MeetingVerdict { phase, probability }
    }

    /// 清空记忆（用户关掉会议感知时调用，避免重新打开后沿用旧状态）。
    pub fn reset(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(minutes: i64) -> Timestamp {
        Timestamp::from_millis(1_700_000_000_000 + minutes * MINUTE)
    }

    fn at_secs(secs: i64) -> Timestamp {
        Timestamp::from_millis(1_700_000_000_000 + secs * 1000)
    }

    #[test]
    fn 没有占用时一切照常() {
        let mut tracker = MeetingTracker::new();
        let verdict = tracker.observe(at(0), Some(false), false);

        assert_eq!(verdict.phase, MeetingPhase::Clear);
        assert!(!verdict.phase.defers_reminders());
        assert_eq!(verdict.probability, 0.0);
    }

    #[test]
    fn 麦克风被占用就推迟提醒() {
        let mut tracker = MeetingTracker::new();
        let verdict = tracker.observe(at(0), Some(true), false);

        assert_eq!(verdict.phase, MeetingPhase::InCall);
        assert!(verdict.phase.defers_reminders());
        assert!(
            verdict.probability >= 0.8,
            "占用麦克风应当被认定为很可能在开会"
        );
    }

    #[test]
    fn 前台是会议软件时概率更高() {
        let mut tracker = MeetingTracker::new();
        let with_app = tracker.observe(at(0), Some(true), true).probability;

        let mut tracker = MeetingTracker::new();
        let without_app = tracker.observe(at(0), Some(true), false).probability;

        assert!(with_app > without_app);
    }

    #[test]
    fn 只开着会议软件但没占麦克风不算开会() {
        let mut tracker = MeetingTracker::new();
        let verdict = tracker.observe(at(0), Some(false), true);

        assert_eq!(verdict.phase, MeetingPhase::Clear);
        assert!(
            !verdict.phase.defers_reminders(),
            "会议软件开着不等于在开会"
        );
        assert!(verdict.probability > 0.0 && verdict.probability < 0.8);
    }

    #[test]
    fn 会议结束后有三分钟缓冲() {
        let mut tracker = MeetingTracker::new();
        tracker.observe(at(0), Some(true), false);
        tracker.observe(at(30), Some(true), false);

        // 麦克风刚释放：进入缓冲期，仍然推迟
        let just_ended = tracker.observe(at_secs(30 * 60 + 10), Some(false), false);
        assert_eq!(just_ended.phase, MeetingPhase::Settling);
        assert!(just_ended.phase.defers_reminders());

        // 距最后一次占用 2 分 50 秒：还在缓冲
        let almost = tracker.observe(at_secs(30 * 60 + 170), Some(false), false);
        assert_eq!(almost.phase, MeetingPhase::Settling);

        // 距最后一次占用满 3 分钟：放行
        let settled = tracker.observe(at_secs(30 * 60 + 180), Some(false), false);
        assert_eq!(settled.phase, MeetingPhase::Clear);
        assert!(!settled.phase.defers_reminders());
    }

    #[test]
    fn 静音取消静音造成的抖动不会提前散会() {
        let mut tracker = MeetingTracker::new();
        tracker.observe(at_secs(0), Some(true), false);

        // 短暂释放 20 秒，随后又占用
        let gap = tracker.observe(at_secs(20), Some(false), false);
        assert_eq!(gap.phase, MeetingPhase::Settling, "短暂释放仍然推迟");

        let back = tracker.observe(at_secs(40), Some(true), false);
        assert_eq!(back.phase, MeetingPhase::InCall, "回到同一场会议");

        // 这一整段加起来没有超过缓冲，也没有开新的一轮计时
        let later = tracker.observe(at_secs(60), Some(true), false);
        assert_eq!(later.phase, MeetingPhase::InCall);
    }

    #[test]
    fn 读不到麦克风按没占用处理_但不会立刻散会() {
        let mut tracker = MeetingTracker::new();
        tracker.observe(at_secs(0), Some(true), false);

        // 一次读取失败
        let flaky = tracker.observe(at_secs(10), None, false);
        assert_eq!(
            flaky.phase,
            MeetingPhase::Settling,
            "一次读取失败不该把进行中的会议当成散会"
        );
    }

    #[test]
    fn 一直读不到时从不推迟() {
        // 平台不支持会议检测（读取恒失败）：功能静默缺失，提醒照常。
        let mut tracker = MeetingTracker::new();
        for i in 0..20 {
            let verdict = tracker.observe(at(i), None, false);
            assert_eq!(verdict.phase, MeetingPhase::Clear);
        }
    }

    #[test]
    fn 保险丝_持续占用满九十分钟后放行提醒() {
        let mut tracker = MeetingTracker::new();
        tracker.observe(at(0), Some(true), false);

        let at_89 = tracker.observe(at(89), Some(true), false);
        assert_eq!(at_89.phase, MeetingPhase::InCall);
        assert!(at_89.phase.defers_reminders());

        let at_90 = tracker.observe(at(90), Some(true), false);
        assert_eq!(
            at_90.phase,
            MeetingPhase::GaveUp,
            "后台进程长期占着麦克风时，提醒不能被永久吞掉"
        );
        assert!(!at_90.phase.defers_reminders());

        // 之后仍在占用：继续放行，不会又切回推迟
        let at_120 = tracker.observe(at(120), Some(true), false);
        assert_eq!(at_120.phase, MeetingPhase::GaveUp);
    }

    #[test]
    fn 保险丝熔断后释放满缓冲期才重新武装() {
        let mut tracker = MeetingTracker::new();
        tracker.observe(at(0), Some(true), false);
        tracker.observe(at(95), Some(true), false);

        // 释放但未满缓冲期：仍是 GaveUp（不会因为释放瞬间又开始推迟）
        let released = tracker.observe(at_secs(95 * 60 + 30), Some(false), false);
        assert_eq!(released.phase, MeetingPhase::GaveUp);

        // 满缓冲期：清空
        let cleared = tracker.observe(at_secs(95 * 60 + 200), Some(false), false);
        assert_eq!(cleared.phase, MeetingPhase::Clear);

        // 新的一场会议从头计 90 分钟
        let next = tracker.observe(at(100), Some(true), false);
        assert_eq!(next.phase, MeetingPhase::InCall);
    }

    #[test]
    fn 睡眠唤醒之后旧会议自然过期() {
        let mut tracker = MeetingTracker::new();
        tracker.observe(at(0), Some(true), false);

        // 合盖 8 小时后回来：麦克风没占用
        let after_sleep = tracker.observe(at(8 * 60), Some(false), false);
        assert_eq!(after_sleep.phase, MeetingPhase::Clear);
    }

    #[test]
    fn 重置后不沿用旧状态() {
        let mut tracker = MeetingTracker::new();
        tracker.observe(at(0), Some(true), false);
        tracker.reset();

        let verdict = tracker.observe(at_secs(5), Some(false), false);
        assert_eq!(verdict.phase, MeetingPhase::Clear);
    }

    #[test]
    fn 界面标签只在推迟时给出() {
        assert_eq!(MeetingPhase::InCall.ui_label(), Some("in_call"));
        assert_eq!(MeetingPhase::Settling.ui_label(), Some("settling"));
        assert_eq!(MeetingPhase::Clear.ui_label(), None);
        assert_eq!(MeetingPhase::GaveUp.ui_label(), None);
    }
}
