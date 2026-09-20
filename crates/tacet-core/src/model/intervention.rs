//! 干预记录：每一次「Tacet 开口说话」的完整档案。
//!
//! 这张记录是 v0.1 里最值钱的数据，因为它同时是三个东西的来源：
//!
//! 1. **接受率的分母** —— 发出去的提醒里，有多少被接受（数据模型 §8）
//! 2. **可解释性的存档** —— 当时到底基于什么理由开的口
//! 3. **v0.3 学习的原料** —— 用户在什么情况下更愿意听劝
//!
//! ## 一条重要的口径
//!
//! “发出去了” 不等于 “打扰到人了”。Level 0（静默）和 Level 1（菜单栏计数）
//! 用户根本不会注意到，把它们算进接受率的分母会让数字变得毫无意义。
//! 所以统计口径里只统计 [`InterventionLevel::disturbs_user`] 为真的记录 ——
//! 这条规则写在这里，是为了让所有调用方用的是同一套判断。

use serde::{Deserialize, Serialize};

use crate::model::{InterventionLevel, NeedKind, Reason};
use crate::Timestamp;

/// 用户对一次提醒的回应。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InterventionOutcome {
    /// 接受了（去休息了 / 喝了水 / 起身活动了）。
    Completed,
    /// 延后了，稍后再提醒。
    Snoozed,
    /// 跳过了。
    Skipped,
    /// 超时未操作 —— 用户可能根本不在，也可能就是不想理。
    ///
    /// 它和 `Skipped` 分开记：跳过是「我看到了但不需要」，
    /// 忽略更可能是「我不在」。两者对学习模块的含义完全不同。
    Ignored,
}

impl InterventionOutcome {
    /// 数据库里的取值（数据模型 §3.1 `interventions.outcome`）。
    pub const fn as_str(self) -> &'static str {
        match self {
            InterventionOutcome::Completed => "completed",
            InterventionOutcome::Snoozed => "snoozed",
            InterventionOutcome::Skipped => "skipped",
            InterventionOutcome::Ignored => "ignored",
        }
    }

    /// 从数据库还原。
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "completed" => Some(InterventionOutcome::Completed),
            "snoozed" => Some(InterventionOutcome::Snoozed),
            "skipped" => Some(InterventionOutcome::Skipped),
            "ignored" => Some(InterventionOutcome::Ignored),
            _ => None,
        }
    }

    /// 界面文案。
    pub const fn display_name(self) -> &'static str {
        match self {
            InterventionOutcome::Completed => "已完成",
            InterventionOutcome::Snoozed => "已延后",
            InterventionOutcome::Skipped => "已跳过",
            InterventionOutcome::Ignored => "未响应",
        }
    }

    /// 这次回应是否算「接受」（接受率的分子）。
    pub const fn is_accepted(self) -> bool {
        matches!(self, InterventionOutcome::Completed)
    }
}

/// 一次干预的完整记录。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Intervention {
    /// 数据库主键；尚未落库时为 `None`。
    pub id: Option<i64>,
    /// 这次干预是为了哪类需求。
    pub kind: NeedKind,
    /// 用了哪一级强度。
    pub level: InterventionLevel,
    /// 当时的决策依据（可解释因子）。
    pub reasons: Vec<Reason>,
    /// 发出时间。
    pub fired_at: Timestamp,
    /// 用户响应时间；未响应时为 `None`。
    pub resolved_at: Option<Timestamp>,
    /// 用户的回应；未响应时为 `None`。
    pub outcome: Option<InterventionOutcome>,
    /// 如果选了延后，延后多久（分钟）。
    pub snooze_minutes: Option<u32>,
}

impl Intervention {
    /// 新建一条「刚发出」的记录。
    pub fn fired(
        kind: NeedKind,
        level: InterventionLevel,
        reasons: Vec<Reason>,
        fired_at: Timestamp,
    ) -> Self {
        Self {
            id: None,
            kind,
            level,
            reasons,
            fired_at,
            resolved_at: None,
            outcome: None,
            snooze_minutes: None,
        }
    }

    /// 从数据库读回来的记录。
    #[allow(clippy::too_many_arguments)]
    pub fn from_stored(
        id: i64,
        kind: NeedKind,
        level: InterventionLevel,
        reasons: Vec<Reason>,
        fired_at: Timestamp,
        resolved_at: Option<Timestamp>,
        outcome: Option<InterventionOutcome>,
        snooze_minutes: Option<u32>,
    ) -> Self {
        Self {
            id: Some(id),
            kind,
            level,
            reasons,
            fired_at,
            resolved_at,
            outcome,
            snooze_minutes,
        }
    }

    /// 记录用户的回应（完成 / 跳过 / 忽略）。
    ///
    /// 延后请用 [`Intervention::snooze`] —— 它还要额外记住延后了多久。
    pub fn resolve(&mut self, outcome: InterventionOutcome, at: Timestamp) {
        self.resolved_at = Some(at);
        self.outcome = Some(outcome);
        self.snooze_minutes = None;
    }

    /// 记录用户选择了「延后 N 分钟」。
    pub fn snooze(&mut self, minutes: u32, at: Timestamp) {
        self.resolved_at = Some(at);
        self.outcome = Some(InterventionOutcome::Snoozed);
        self.snooze_minutes = Some(minutes);
    }

    /// 用户是否已经回应过。
    pub const fn is_resolved(&self) -> bool {
        self.outcome.is_some()
    }

    /// 是否应当计入接受率的统计（即：是否真的打扰到了用户）。
    ///
    /// 口径来源：数据模型 §8「接受率 = outcome = completed 的 interventions / 全部**已发出**的 interventions」，
    /// 结合 PRD §3.3 中 Level 0/1「不发出声音、不弹窗」的定义，
    /// 这里把分母收窄为「真正会打扰人的等级」。
    pub const fn counts_toward_acceptance_rate(&self) -> bool {
        self.level.disturbs_user()
    }

    /// 从发出到响应过了多久（毫秒）；未响应时为 `None`。
    pub fn response_ms(&self) -> Option<i64> {
        self.resolved_at.map(|at| at.millis_since(self.fired_at))
    }

    /// 如果用户选了延后，下一次该在什么时候提醒。
    ///
    /// 返回 `None` 表示这条记录不需要重新排期。
    pub fn snooze_until(&self) -> Option<Timestamp> {
        match (self.outcome, self.snooze_minutes) {
            (Some(InterventionOutcome::Snoozed), Some(minutes)) => Some(
                self.resolved_at
                    .unwrap_or(self.fired_at)
                    .saturating_add_millis(minutes as i64 * crate::time::MINUTE),
            ),
            _ => None,
        }
    }

    /// 把当时的决策依据渲染成给用户看的多行文案。
    ///
    /// 这是界面上「为什么现在提醒我」那个折叠区的数据来源。
    pub fn why_lines(&self) -> Vec<String> {
        Reason::render_lines(&self.reasons)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::MINUTE;

    fn at() -> Timestamp {
        Timestamp::from_millis(1_700_000_000_000)
    }

    fn sample() -> Intervention {
        Intervention::fired(
            NeedKind::Hydration,
            InterventionLevel::Notification,
            vec![Reason::SinceLastHydration { minutes: 93 }],
            at(),
        )
    }

    #[test]
    fn 新记录处于待响应状态() {
        let record = sample();

        assert!(!record.is_resolved());
        assert!(record.resolved_at.is_none());
        assert!(record.snooze_minutes.is_none());
        assert_eq!(record.response_ms(), None);
        assert_eq!(record.snooze_until(), None);
    }

    #[test]
    fn 完成操作记录响应时间() {
        let mut record = sample();
        let responded_at = at().saturating_add_millis(12 * 1000);

        record.resolve(InterventionOutcome::Completed, responded_at);

        assert!(record.is_resolved());
        assert_eq!(record.outcome, Some(InterventionOutcome::Completed));
        assert_eq!(record.response_ms(), Some(12_000));
        assert!(record.outcome.is_some_and(InterventionOutcome::is_accepted));
    }

    #[test]
    fn 延后操作记住时长并算出下次时间() {
        let mut record = sample();
        let responded_at = at().saturating_add_millis(5 * 1000);

        record.snooze(3, responded_at);

        assert_eq!(record.outcome, Some(InterventionOutcome::Snoozed));
        assert_eq!(record.snooze_minutes, Some(3));
        assert_eq!(
            record.snooze_until(),
            Some(responded_at.saturating_add_millis(3 * MINUTE))
        );
    }

    #[test]
    fn 从延后改为完成会清掉延后时长() {
        // 这是个真实场景：用户点了「3 分钟后」，转身又直接去喝水了。
        let mut record = sample();
        record.snooze(3, at());
        assert_eq!(record.snooze_minutes, Some(3));

        record.resolve(
            InterventionOutcome::Completed,
            at().saturating_add_millis(MINUTE),
        );

        assert_eq!(record.outcome, Some(InterventionOutcome::Completed));
        assert_eq!(record.snooze_minutes, None, "已完成的记录不应残留延后时长");
        assert_eq!(record.snooze_until(), None);
    }

    #[test]
    fn 静默与环境级不计入接受率分母() {
        let silent =
            Intervention::fired(NeedKind::Rest, InterventionLevel::Silent, Vec::new(), at());
        let ambient =
            Intervention::fired(NeedKind::Rest, InterventionLevel::Ambient, Vec::new(), at());
        let notification = sample();

        assert!(!silent.counts_toward_acceptance_rate());
        assert!(!ambient.counts_toward_acceptance_rate());
        assert!(notification.counts_toward_acceptance_rate());
    }

    #[test]
    fn 尚无响应时的下次提醒时间() {
        // 用户还没回应，但记录里若意外带着延后时长，也不应该算出排期。
        let mut record = sample();
        record.snooze_minutes = Some(5);
        assert_eq!(record.snooze_until(), None);
    }

    #[test]
    fn 回应类型字符串往返一致() {
        let outcomes = [
            InterventionOutcome::Completed,
            InterventionOutcome::Snoozed,
            InterventionOutcome::Skipped,
            InterventionOutcome::Ignored,
        ];

        for outcome in outcomes {
            assert_eq!(InterventionOutcome::parse(outcome.as_str()), Some(outcome));
        }
        assert_eq!(InterventionOutcome::parse("unknown"), None);
    }

    #[test]
    fn 从数据库还原保留全部字段() {
        let restored = Intervention::from_stored(
            42,
            NeedKind::Rest,
            InterventionLevel::FullScreen,
            vec![Reason::ContinuousWork { minutes: 78 }],
            at(),
            Some(at().saturating_add_millis(30_000)),
            Some(InterventionOutcome::Skipped),
            None,
        );

        assert_eq!(restored.id, Some(42));
        assert_eq!(restored.level, InterventionLevel::FullScreen);
        assert_eq!(restored.reasons.len(), 1);
        assert_eq!(restored.response_ms(), Some(30_000));
    }
}
