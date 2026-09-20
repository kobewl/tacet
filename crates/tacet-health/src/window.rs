//! 时机窗口 —— 「现在这个点，适合提醒吗？」
//!
//! 需求引擎回答「该不该喝水」，时机窗口回答「**现在是不是开口的好时候**」。
//! 这两件事必须分开，因为它们的答案经常相反：
//!
//! > 需求 0.95（严重超时），但现在凌晨两点半，用户在睡觉。
//!
//! 把这两件事混在一个函数里，迟早会写出「半夜弹窗提醒你喝水」这种代码。
//!
//! ## v0.1 的范围
//!
//! v0.1 只做一个基础版的窗口判断：时段（不在深夜）、以及连续的安静期
//! （用户刚被打扰过就别再开口）。完整的 Interruptibility 五因子模型属于 v0.2，
//! 那时会引入会议概率、输入强度、分应用策略。
//!
//! 这里刻意把接口设计成「策略对象」而不是一堆 if：v0.2 扩展时
//! 只需要往 [`WindowPolicy`] 里加因子，调用方不用改。

use serde::{Deserialize, Serialize};
use tacet_core::model::ContextSnapshot;
use tacet_core::time::{Timestamp, MINUTE};

/// 一天中的时段（相对本地时间的小时数）。
///
/// 注意：这里的「小时」是从 [`Timestamp`] 推算的 UTC 小时，**不是本地时间**。
/// 严格说这会有一处偏差，但 v0.1 的处理方式是：
///
/// 1. 时钟由壳层传入带时区偏移的修正值（见 `tacet-storage::datewin` 的统一口径）；
/// 2. v0.3 引入分时段策略时，会把时区处理彻底收口到存储层。
///
/// 之所以在核心层不引入日期库，是为了让「今天从几点开始」这个问题只有一处答案
/// （数据模型 §8 的工程约束）。这里只做粗粒度的「别在深夜打扰」判断，
/// 对精度的要求本来就低。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DayPart {
    /// 深夜（23:00~06:00）—— 除非需求拉满，否则一律不打扰。
    Night,
    /// 早晨（06:00~12:00）。
    Morning,
    /// 下午（12:00~18:00）。
    Afternoon,
    /// 傍晚到夜间（18:00~23:00）。
    Evening,
}

impl DayPart {
    /// 由小时数（0~23）判定时段。
    pub const fn from_hour(hour: u32) -> Self {
        match hour {
            6..=11 => DayPart::Morning,
            12..=17 => DayPart::Afternoon,
            18..=22 => DayPart::Evening,
            _ => DayPart::Night,
        }
    }

    /// 界面显示名。
    pub const fn display_name(self) -> &'static str {
        match self {
            DayPart::Night => "深夜",
            DayPart::Morning => "上午",
            DayPart::Afternoon => "下午",
            DayPart::Evening => "晚间",
        }
    }

    /// 这个时段是否适合主动打扰。
    ///
    /// 深夜不算「适合」，但也不是完全禁止 —— 见 [`WindowPolicy::evaluate`] 里的
    /// 紧急豁免。深夜还在工作的人，恰恰最需要被提醒去睡觉。
    pub const fn allows_interruption(self) -> bool {
        !matches!(self, DayPart::Night)
    }
}

/// 窗口判断的结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReminderWindow {
    /// 现在这个时刻是否适合打扰。
    pub is_open: bool,
    /// 现在属于哪个时段。
    pub day_part: DayPart,
    /// 不适合打扰的原因（`is_open` 为真时是 `None`）。
    pub closed_reason: Option<WindowClosedReason>,
}

impl ReminderWindow {
    /// 打开着的窗口（可以打扰）。
    pub const fn open(day_part: DayPart) -> Self {
        Self {
            is_open: true,
            day_part,
            closed_reason: None,
        }
    }

    /// 关闭着的窗口（不适合打扰）。
    pub const fn closed(day_part: DayPart, reason: WindowClosedReason) -> Self {
        Self {
            is_open: false,
            day_part,
            closed_reason: Some(reason),
        }
    }
}

/// 窗口关闭的原因。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowClosedReason {
    /// 深夜时段。
    Night,
    /// 用户不在电脑前。
    Away,
    /// 刚打扰过，需要留出安静期。
    JustInterrupted,
}

impl WindowClosedReason {
    /// 界面显示名。
    pub const fn display_name(self) -> &'static str {
        match self {
            WindowClosedReason::Night => "现在是深夜",
            WindowClosedReason::Away => "你不在电脑前",
            WindowClosedReason::JustInterrupted => "刚刚提醒过",
        }
    }
}

/// 时机窗口策略。
#[derive(Debug, Clone, Copy)]
pub struct WindowPolicy {
    /// 两次打扰之间至少间隔多久（毫秒）。
    ///
    /// 这是「不要让用户觉得被追着催」的硬约束。PRD §3.3 通用约束 4 说的是
    /// 「同一等级 10 分钟内不重复」，那是按等级限流；
    /// 这里是更上层的「整体安静期」—— 哪怕换了另一类需求，也不该立刻再开口。
    pub quiet_period_ms: i64,
    /// 深夜时段是否允许打扰。
    ///
    /// 默认允许，但仅限紧急情况（由调用方传 `urgent` 决定）。
    /// 为什么默认允许：深夜还在用电脑的人往往最需要被劝去睡觉。
    /// 但如果用户把提醒关到只剩「工作时间」，这个开关会被置为 false。
    pub allow_night_interruption: bool,
}

impl Default for WindowPolicy {
    fn default() -> Self {
        Self {
            quiet_period_ms: 5 * MINUTE,
            allow_night_interruption: true,
        }
    }
}

impl WindowPolicy {
    /// 用默认参数构造。
    pub fn new() -> Self {
        Self::default()
    }

    /// 判断当前是否适合打扰。
    ///
    /// 判断顺序：
    ///
    /// 1. **紧急豁免**：需求拉满时直接放行 ——
    ///    一个「该喝水了」可以等等，一个「你连续工作 4 小时了」不能等。
    ///    这条豁免也意味着深夜的紧急提醒仍然发得出去。
    /// 2. 人不在 → 关闭（对着空椅子说话没有意义）
    /// 3. 刚打扰过 → 关闭
    /// 4. 深夜 → 关闭
    /// 5. 其余 → 打开
    pub fn evaluate(
        &self,
        now: Timestamp,
        context: &ContextSnapshot,
        last_interruption_at: Option<Timestamp>,
        idle_threshold_seconds: u32,
        urgent: bool,
    ) -> ReminderWindow {
        let hour = hour_of_day(now);
        let day_part = DayPart::from_hour(hour);

        // ① 紧急豁免：需求拉满时，前面所有顾虑都让路。
        if urgent {
            return ReminderWindow::open(day_part);
        }

        // ② 人不在电脑前。
        if context.is_away(idle_threshold_seconds) {
            return ReminderWindow::closed(day_part, WindowClosedReason::Away);
        }

        // ③ 刚打扰过，留出安静期。
        if let Some(last) = last_interruption_at {
            let since = now.millis_since(last);
            // 只拦「刚刚」，不拦「未来时间」（时间校准导致的脏数据）
            if (0..self.quiet_period_ms).contains(&since) {
                return ReminderWindow::closed(day_part, WindowClosedReason::JustInterrupted);
            }
        }

        // ④ 深夜。
        if !day_part.allows_interruption() && !self.allow_night_interruption {
            return ReminderWindow::closed(day_part, WindowClosedReason::Night);
        }

        ReminderWindow::open(day_part)
    }
}

/// 从一个时间戳里取出「小时」。
///
/// 用的是 UTC 小时。关于它为什么可以接受，见 [`DayPart`] 的文档说明。
fn hour_of_day(at: Timestamp) -> u32 {
    let ms_in_day = at.as_millis().rem_euclid(86_400_000);
    (ms_in_day / 3_600_000) as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use tacet_core::model::{AppCategory, ForegroundApp};

    fn t0() -> Timestamp {
        Timestamp::from_millis(1_700_000_000_000)
    }

    /// 构造某个 UTC 小时整点的时间戳。
    fn at_hour(hour: u32) -> Timestamp {
        Timestamp::from_millis(hour as i64 * 3_600_000)
    }

    fn active_context() -> ContextSnapshot {
        let mut ctx = ContextSnapshot::unavailable(t0());
        ctx.foreground_app = Some(ForegroundApp::new(
            "com.apple.Safari",
            "Safari",
            AppCategory::Browser,
        ));
        ctx
    }

    #[test]
    fn 时段划分() {
        assert_eq!(DayPart::from_hour(0), DayPart::Night);
        assert_eq!(DayPart::from_hour(5), DayPart::Night);
        assert_eq!(DayPart::from_hour(6), DayPart::Morning);
        assert_eq!(DayPart::from_hour(11), DayPart::Morning);
        assert_eq!(DayPart::from_hour(12), DayPart::Afternoon);
        assert_eq!(DayPart::from_hour(17), DayPart::Afternoon);
        assert_eq!(DayPart::from_hour(18), DayPart::Evening);
        assert_eq!(DayPart::from_hour(22), DayPart::Evening);
        assert_eq!(DayPart::from_hour(23), DayPart::Night);
    }

    #[test]
    fn 正常时段且用户在场时窗口打开() {
        let policy = WindowPolicy::new();
        let window = policy.evaluate(at_hour(14), &active_context(), None, 300, false);

        assert!(window.is_open);
        assert_eq!(window.day_part, DayPart::Afternoon);
        assert_eq!(window.closed_reason, None);
    }

    #[test]
    fn 用户不在时窗口关闭() {
        let policy = WindowPolicy::new();
        let mut ctx = active_context();
        ctx.idle_seconds = 600;

        let window = policy.evaluate(at_hour(14), &ctx, None, 300, false);

        assert!(!window.is_open);
        assert_eq!(window.closed_reason, Some(WindowClosedReason::Away));
    }

    #[test]
    fn 刚打扰过会留出安静期() {
        let policy = WindowPolicy::new();
        let now = at_hour(14);
        let just_now = now.saturating_sub_millis(2 * MINUTE);

        let window = policy.evaluate(now, &active_context(), Some(just_now), 300, false);
        assert!(!window.is_open);
        assert_eq!(
            window.closed_reason,
            Some(WindowClosedReason::JustInterrupted)
        );

        // 过了安静期就恢复了
        let long_ago = now.saturating_sub_millis(6 * MINUTE);
        assert!(
            policy
                .evaluate(now, &active_context(), Some(long_ago), 300, false)
                .is_open
        );
    }

    #[test]
    fn 紧急需求可以突破所有限制() {
        // 「你连续工作 4 小时了」这种提醒，哪怕在深夜、哪怕刚提醒过，也该发出去。
        let policy = WindowPolicy::new();
        let mut ctx = active_context();
        ctx.idle_seconds = 3600; // 人还不在（比如锁屏挂着）

        let window = policy.evaluate(
            at_hour(3),
            &ctx,
            Some(at_hour(3).saturating_sub_millis(30_000)),
            300,
            true,
        );

        assert!(window.is_open, "紧急需求应当豁免一切窗口限制");
    }

    #[test]
    fn 深夜在允许时不关闭() {
        let policy = WindowPolicy::new();
        let window = policy.evaluate(at_hour(2), &active_context(), None, 300, false);

        assert!(window.is_open, "深夜还在用电脑的人往往最需要被提醒");
        assert_eq!(window.day_part, DayPart::Night);
    }

    #[test]
    fn 深夜在禁止时关闭并说明原因() {
        let policy = WindowPolicy {
            quiet_period_ms: 5 * MINUTE,
            allow_night_interruption: false,
        };

        let window = policy.evaluate(at_hour(3), &active_context(), None, 300, false);

        assert!(!window.is_open);
        assert_eq!(window.closed_reason, Some(WindowClosedReason::Night));
    }

    #[test]
    fn 时钟回拨导致的未来时间不留出安静期() {
        // 数据脏了（上次提醒的时间戳在未来），不应该把窗口永久关闭 ——
        // 那会让提醒彻底消失，是最难查的一类 bug。
        let policy = WindowPolicy::new();
        let now = at_hour(14);
        let future = now.saturating_add_millis(60 * MINUTE);

        assert!(
            policy
                .evaluate(now, &active_context(), Some(future), 300, false)
                .is_open
        );
    }

    #[test]
    fn 小时换算在一天内循环() {
        assert_eq!(hour_of_day(Timestamp::from_millis(0)), 0);
        assert_eq!(hour_of_day(at_hour(23)), 23);
        // 跨过一天后再回到 0 点
        assert_eq!(hour_of_day(Timestamp::from_millis(24 * 3_600_000)), 0);
        assert_eq!(hour_of_day(Timestamp::from_millis(25 * 3_600_000)), 1);
    }

    #[test]
    fn 关闭原因有可读文案() {
        assert_eq!(WindowClosedReason::Away.display_name(), "你不在电脑前");
        assert_eq!(DayPart::Evening.display_name(), "晚间");
    }
}
