//! 用户偏好：四类提醒的开关与间隔，以及几条全局设置。
//!
//! 默认值全部来自 PRD §3.1 / §3.2，代码里的常量与文档是同一套数字，
//! 改任何一条都要同时改文档（研发规范 §6 的同步义务）。
//!
//! ## 设置用「键值对」存，但代码里不能是字符串裸奔
//!
//! 数据库里 `settings` 是 KV 表（ADR-009），好处是加一项设置不用改表结构。
//! 但代价是键名变成散落各处的字符串字面量，拼错一个字母就是「设置了但没生效」
//! 这种最难查的 bug。所以这里用 [`SettingsKey`] 枚举把键名收口，
//! 所有读写都必须经过它。

use serde::{Deserialize, Serialize};

use crate::model::NeedKind;

/// 一类提醒的配置。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReminderRule {
    /// 是否开启这类提醒。
    pub enabled: bool,
    /// 提醒间隔（分钟）。
    pub interval_minutes: u32,
}

impl ReminderRule {
    /// 间隔的下限与上限。
    ///
    /// 为什么要有上下限：下限保护用户不被自己设置的「每 2 分钟提醒一次」折磨，
    /// 上限则是因为超过两小时基本等于关掉了提醒。UI 上用滑杆给出这个区间，
    /// 而不是让用户输入一个任意数字。
    pub const MIN_INTERVAL_MINUTES: u32 = 5;
    /// 单类提醒间隔的最大值。
    pub const MAX_INTERVAL_MINUTES: u32 = 240;

    /// 新建一条规则，间隔自动夹取到合法区间。
    pub fn new(enabled: bool, interval_minutes: u32) -> Self {
        Self {
            enabled,
            interval_minutes: interval_minutes
                .clamp(Self::MIN_INTERVAL_MINUTES, Self::MAX_INTERVAL_MINUTES),
        }
    }

    /// 间隔换算成毫秒。
    pub const fn interval_ms(&self) -> i64 {
        self.interval_minutes as i64 * crate::time::MINUTE
    }
}

/// 四类提醒的配置集合。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReminderSettings {
    /// 休息提醒。
    pub rest: ReminderRule,
    /// 喝水提醒。
    pub hydration: ReminderRule,
    /// 活动提醒。
    pub movement: ReminderRule,
    /// 护眼提醒。
    pub eye_rest: ReminderRule,
}

impl Default for ReminderSettings {
    /// PRD §3.2 的默认间隔：休息 50 / 喝水 45 / 活动 60 / 护眼 40 分钟。
    fn default() -> Self {
        Self {
            rest: ReminderRule::new(true, 50),
            hydration: ReminderRule::new(true, 45),
            movement: ReminderRule::new(true, 60),
            eye_rest: ReminderRule::new(true, 40),
        }
    }
}

impl ReminderSettings {
    /// 按类型取规则。
    pub const fn get(&self, kind: NeedKind) -> Option<ReminderRule> {
        match kind {
            NeedKind::Rest => Some(self.rest),
            NeedKind::Hydration => Some(self.hydration),
            NeedKind::Movement => Some(self.movement),
            NeedKind::EyeRest => Some(self.eye_rest),
            NeedKind::Fused => None,
        }
    }

    /// 按类型取提醒间隔（分钟）。融合类型没有独立间隔，返回 `None`。
    pub const fn interval_minutes(&self, kind: NeedKind) -> Option<u32> {
        match self.get(kind) {
            Some(rule) => Some(rule.interval_minutes),
            None => None,
        }
    }

    /// 这类提醒是否开着。
    pub const fn is_enabled(&self, kind: NeedKind) -> bool {
        match self.get(kind) {
            Some(rule) => rule.enabled,
            None => false,
        }
    }

    /// 有没有任何一类提醒是开着的。
    pub const fn any_enabled(&self) -> bool {
        self.rest.enabled
            || self.hydration.enabled
            || self.movement.enabled
            || self.eye_rest.enabled
    }
}

/// 全部用户偏好。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UserPreferences {
    /// 四类提醒的开关与间隔。
    pub reminders: ReminderSettings,
    /// 勿扰模式：开着的时候一律 Level 0。
    pub do_not_disturb: bool,
    /// 多久没有输入就算「离开」（分钟），PRD §3.1 默认 5 分钟。
    pub idle_threshold_minutes: u32,
    /// 一次休息持续多久（分钟），PRD §3.2 默认 5 分钟。
    pub break_duration_minutes: u32,
    /// 休息提醒上提供哪些延后选项（分钟），PRD §3.3 默认 1 / 3 / 5。
    pub snooze_options_minutes: Vec<u32>,
}

impl Default for UserPreferences {
    fn default() -> Self {
        Self {
            reminders: ReminderSettings::default(),
            do_not_disturb: false,
            idle_threshold_minutes: 5,
            break_duration_minutes: 5,
            snooze_options_minutes: vec![1, 3, 5],
        }
    }
}

impl UserPreferences {
    /// 空闲阈值换算成秒（平台层的空闲检测以秒为单位）。
    pub const fn idle_threshold_seconds(&self) -> u32 {
        self.idle_threshold_minutes * 60
    }

    /// 休息时长换算成毫秒。
    pub const fn break_duration_ms(&self) -> i64 {
        self.break_duration_minutes as i64 * crate::time::MINUTE
    }
}

/// 设置项在 KV 表里的键名。
///
/// ```rust
/// use tacet_core::model::SettingsKey;
///
/// assert_eq!(SettingsKey::ReminderRestInterval.as_str(), "reminder.rest.interval_minutes");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SettingsKey {
    /// 休息提醒开关。
    ReminderRestEnabled,
    /// 休息提醒间隔（分钟）。
    ReminderRestInterval,
    /// 喝水提醒开关。
    ReminderHydrationEnabled,
    /// 喝水提醒间隔（分钟）。
    ReminderHydrationInterval,
    /// 活动提醒开关。
    ReminderMovementEnabled,
    /// 活动提醒间隔（分钟）。
    ReminderMovementInterval,
    /// 护眼提醒开关。
    ReminderEyeRestEnabled,
    /// 护眼提醒间隔（分钟）。
    ReminderEyeRestInterval,
    /// 勿扰模式。
    DoNotDisturb,
    /// 空闲判定阈值（分钟）。
    IdleThresholdMinutes,
    /// 休息时长（分钟）。
    BreakDurationMinutes,
    /// 延后选项（分钟数组）。
    SnoozeOptionsMinutes,
}

impl SettingsKey {
    /// 全部键，用于导出与自检。
    pub const ALL: [SettingsKey; 12] = [
        SettingsKey::ReminderRestEnabled,
        SettingsKey::ReminderRestInterval,
        SettingsKey::ReminderHydrationEnabled,
        SettingsKey::ReminderHydrationInterval,
        SettingsKey::ReminderMovementEnabled,
        SettingsKey::ReminderMovementInterval,
        SettingsKey::ReminderEyeRestEnabled,
        SettingsKey::ReminderEyeRestInterval,
        SettingsKey::DoNotDisturb,
        SettingsKey::IdleThresholdMinutes,
        SettingsKey::BreakDurationMinutes,
        SettingsKey::SnoozeOptionsMinutes,
    ];

    /// 数据库里的键名。
    ///
    /// 命名沿用数据模型文档里的示例格式 `reminder.rest.interval_minutes`：
    /// 「域.对象.属性」，全小写加点分隔，一眼能看出层级。
    pub const fn as_str(self) -> &'static str {
        match self {
            SettingsKey::ReminderRestEnabled => "reminder.rest.enabled",
            SettingsKey::ReminderRestInterval => "reminder.rest.interval_minutes",
            SettingsKey::ReminderHydrationEnabled => "reminder.hydration.enabled",
            SettingsKey::ReminderHydrationInterval => "reminder.hydration.interval_minutes",
            SettingsKey::ReminderMovementEnabled => "reminder.movement.enabled",
            SettingsKey::ReminderMovementInterval => "reminder.movement.interval_minutes",
            SettingsKey::ReminderEyeRestEnabled => "reminder.eye_rest.enabled",
            SettingsKey::ReminderEyeRestInterval => "reminder.eye_rest.interval_minutes",
            SettingsKey::DoNotDisturb => "general.do_not_disturb",
            SettingsKey::IdleThresholdMinutes => "general.idle_threshold_minutes",
            SettingsKey::BreakDurationMinutes => "break.duration_minutes",
            SettingsKey::SnoozeOptionsMinutes => "break.snooze_options_minutes",
        }
    }

    /// 从字符串还原（读库时用；未知键返回 `None`）。
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|key| key.as_str() == s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 默认值来自产品文档() {
        let prefs = UserPreferences::default();

        assert_eq!(prefs.reminders.rest.interval_minutes, 50);
        assert_eq!(prefs.reminders.hydration.interval_minutes, 45);
        assert_eq!(prefs.reminders.movement.interval_minutes, 60);
        assert_eq!(prefs.reminders.eye_rest.interval_minutes, 40);
        assert_eq!(prefs.idle_threshold_minutes, 5);
        assert_eq!(prefs.break_duration_minutes, 5);
        assert_eq!(prefs.snooze_options_minutes, vec![1, 3, 5]);
        assert!(!prefs.do_not_disturb);
    }

    #[test]
    fn 间隔被夹取到合法区间() {
        assert_eq!(
            ReminderRule::new(true, 1).interval_minutes,
            ReminderRule::MIN_INTERVAL_MINUTES
        );
        assert_eq!(
            ReminderRule::new(true, 9999).interval_minutes,
            ReminderRule::MAX_INTERVAL_MINUTES
        );
        // 区间内的值原样保留
        assert_eq!(ReminderRule::new(true, 50).interval_minutes, 50);
    }

    #[test]
    fn 按类型查询提醒配置() {
        let settings = ReminderSettings::default();

        assert!(settings.is_enabled(NeedKind::Rest));
        assert_eq!(settings.interval_minutes(NeedKind::EyeRest), Some(40));
        // 融合类型没有独立配置
        assert_eq!(settings.interval_minutes(NeedKind::Fused), None);
        assert!(!settings.is_enabled(NeedKind::Fused));
    }

    #[test]
    fn 关掉全部提醒后可知晓() {
        let mut settings = ReminderSettings::default();
        assert!(settings.any_enabled());

        settings.rest.enabled = false;
        settings.hydration.enabled = false;
        settings.movement.enabled = false;
        settings.eye_rest.enabled = false;
        assert!(!settings.any_enabled());
    }

    #[test]
    fn 单位换算正确() {
        let prefs = UserPreferences::default();
        assert_eq!(prefs.idle_threshold_seconds(), 300);
        assert_eq!(prefs.break_duration_ms(), 5 * crate::time::MINUTE);
        assert_eq!(prefs.reminders.rest.interval_ms(), 50 * crate::time::MINUTE);
    }

    #[test]
    fn 设置键字符串往返一致且不重复() {
        let mut seen = Vec::new();

        for key in SettingsKey::ALL {
            assert_eq!(SettingsKey::parse(key.as_str()), Some(key));
            assert!(
                !seen.contains(&key.as_str()),
                "设置键 {} 重复了，会导致两项设置互相覆盖",
                key.as_str()
            );
            seen.push(key.as_str());
        }

        assert_eq!(SettingsKey::ALL.len(), seen.len());
        assert_eq!(SettingsKey::parse("no.such.key"), None);
    }

    #[test]
    fn 设置键命名符合约定() {
        for key in SettingsKey::ALL {
            let name = key.as_str();
            assert!(
                name.contains('.'),
                "设置键 {name} 应当使用「域.对象.属性」的层级命名"
            );
            assert!(
                name.chars()
                    .all(|c| c.is_ascii_lowercase() || c == '.' || c == '_'),
                "设置键 {name} 只允许小写字母、下划线与点"
            );
        }
    }
}
