//! 五层干预体系（Level 0~5）。
//!
//! 这是 Tacet 最重要的一条产品红线（ADR-006）。它把「打扰用户」变成一个有刻度的
//! 连续变量，从而可以回答那个核心问题：**这次要用哪一级？**
//!
//! | 等级 | 名字 | 表现 | 用户感知 |
//! | --- | --- | --- | --- |
//! | 0 | Silent | 什么都不做 | 无 |
//! | 1 | Ambient | 菜单栏上的计数 | 余光可及 |
//! | 2 | Notification | 系统通知 | 轻微 |
//! | 3 | Floating Card | 不阻塞操作的浮卡 | 明显，但不挡路 |
//! | 4 | Full Screen | 全屏毛玻璃 | 强，但一键可退 |
//! | 5 | Escalated | 升级文案与频率 | 强（**永不锁屏**） |
//!
//! 请注意 Level 5 的定义：它升级的只是**文案与频率**，不是权限。
//! 「用更强的打扰去惩罚不听话的用户」是这类产品最常见的堕落路径，
//! 而 Tacet 的原则 2 明说：跳过是数据，不是失败。

use serde::{Deserialize, Serialize};

use crate::CoreError;

/// 干预等级。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InterventionLevel {
    /// 0 —— 完全不打扰。刚休息过、用户开了勿扰、需求不高时都是这一级。
    Silent,
    /// 1 —— 环境级提示：只改变菜单栏上的一行小字，不出声、不弹窗。
    Ambient,
    /// 2 —— 系统通知：轻微打断，带操作按钮，文案里必有「稍后」。
    Notification,
    /// 3 —— 悬浮卡片：醒目但不阻塞，可拖动、可关闭、不抢焦点（v0.2）。
    FloatingCard,
    /// 4 —— 全屏毛玻璃提醒：打断最强，但必须提供一键跳过。
    FullScreen,
    /// 5 —— 升级提醒：反复被跳过之后升级文案与频率（v0.2）。
    Escalated,
}

impl InterventionLevel {
    /// 从 0 到 5 的顺序数组，方便遍历。
    pub const ALL: [InterventionLevel; 6] = [
        InterventionLevel::Silent,
        InterventionLevel::Ambient,
        InterventionLevel::Notification,
        InterventionLevel::FloatingCard,
        InterventionLevel::FullScreen,
        InterventionLevel::Escalated,
    ];

    /// 数值形式（写数据库 `interventions.level` 用）。
    pub const fn as_i64(self) -> i64 {
        match self {
            InterventionLevel::Silent => 0,
            InterventionLevel::Ambient => 1,
            InterventionLevel::Notification => 2,
            InterventionLevel::FloatingCard => 3,
            InterventionLevel::FullScreen => 4,
            InterventionLevel::Escalated => 5,
        }
    }

    /// 从数据库读回。
    pub fn from_i64(value: i64) -> Result<Self, CoreError> {
        match value {
            0 => Ok(InterventionLevel::Silent),
            1 => Ok(InterventionLevel::Ambient),
            2 => Ok(InterventionLevel::Notification),
            3 => Ok(InterventionLevel::FloatingCard),
            4 => Ok(InterventionLevel::FullScreen),
            5 => Ok(InterventionLevel::Escalated),
            other => Err(CoreError::InvalidInterventionLevel(other)),
        }
    }

    /// 稳定字符串（日志、JSON 用）。
    pub const fn as_str(self) -> &'static str {
        match self {
            InterventionLevel::Silent => "silent",
            InterventionLevel::Ambient => "ambient",
            InterventionLevel::Notification => "notification",
            InterventionLevel::FloatingCard => "floating_card",
            InterventionLevel::FullScreen => "full_screen",
            InterventionLevel::Escalated => "escalated",
        }
    }

    /// 界面显示名。
    pub const fn display_name(self) -> &'static str {
        match self {
            InterventionLevel::Silent => "静默",
            InterventionLevel::Ambient => "环境提示",
            InterventionLevel::Notification => "系统通知",
            InterventionLevel::FloatingCard => "悬浮卡片",
            InterventionLevel::FullScreen => "全屏提醒",
            InterventionLevel::Escalated => "升级提醒",
        }
    }

    /// 这一级是否真的会「打扰到人」。
    ///
    /// 用于统计口径：接受率的分母应该是**真正打扰过的干预**，
    /// 把 Level 0/1 也算进去会让数字失真（它们被用户完全无感地略过了）。
    pub const fn disturbs_user(self) -> bool {
        matches!(
            self,
            InterventionLevel::Notification
                | InterventionLevel::FloatingCard
                | InterventionLevel::FullScreen
                | InterventionLevel::Escalated
        )
    }

    /// 取两者中更克制（数值更小）的那个。
    ///
    /// 决策引擎里到处在用：「本想全屏提醒，但用户在开会」→ 降级取通知。
    pub fn min(self, other: Self) -> Self {
        if self.as_i64() <= other.as_i64() {
            self
        } else {
            other
        }
    }

    /// 取两者中更强（数值更大）的那个。
    pub fn max(self, other: Self) -> Self {
        if self.as_i64() >= other.as_i64() {
            self
        } else {
            other
        }
    }

    /// 降一级（0 再降还是 0）。
    pub fn degrade(self) -> Self {
        Self::from_i64((self.as_i64() - 1).max(0)).unwrap_or(InterventionLevel::Silent)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 数值往返一致() {
        for level in InterventionLevel::ALL {
            assert_eq!(
                InterventionLevel::from_i64(level.as_i64()).expect("合法等级"),
                level
            );
        }
    }

    #[test]
    fn 越界数值返回错误而不是静默降级() {
        // 注意这里刻意返回 Err 而不是 Silent：数据库里读到 7 是个 bug，
        // 悄悄当成静默会把 bug 藏起来，最终表现成「提醒莫名其妙不见了」。
        assert_eq!(
            InterventionLevel::from_i64(7),
            Err(CoreError::InvalidInterventionLevel(7))
        );
        assert_eq!(
            InterventionLevel::from_i64(-1),
            Err(CoreError::InvalidInterventionLevel(-1))
        );
    }

    #[test]
    fn 顺序与数值一致() {
        assert!(InterventionLevel::Silent < InterventionLevel::Ambient);
        assert!(InterventionLevel::Notification < InterventionLevel::FullScreen);
        assert!(InterventionLevel::FullScreen < InterventionLevel::Escalated);
    }

    #[test]
    fn 只有二到五级算真正打扰() {
        assert!(!InterventionLevel::Silent.disturbs_user());
        assert!(!InterventionLevel::Ambient.disturbs_user());
        assert!(InterventionLevel::Notification.disturbs_user());
        assert!(InterventionLevel::FloatingCard.disturbs_user());
        assert!(InterventionLevel::FullScreen.disturbs_user());
        assert!(InterventionLevel::Escalated.disturbs_user());
    }

    #[test]
    fn 取克制与取更强() {
        assert_eq!(
            InterventionLevel::FullScreen.min(InterventionLevel::Notification),
            InterventionLevel::Notification
        );
        assert_eq!(
            InterventionLevel::Ambient.max(InterventionLevel::FullScreen),
            InterventionLevel::FullScreen
        );
    }

    #[test]
    fn 降级在最低处站住() {
        assert_eq!(
            InterventionLevel::Notification.degrade(),
            InterventionLevel::Ambient
        );
        assert_eq!(
            InterventionLevel::Silent.degrade(),
            InterventionLevel::Silent
        );
    }

    #[test]
    fn 序列化为蛇形字符串() {
        assert_eq!(
            serde_json::to_string(&InterventionLevel::FullScreen).expect("序列化不应失败"),
            "\"full_screen\""
        );
    }
}
