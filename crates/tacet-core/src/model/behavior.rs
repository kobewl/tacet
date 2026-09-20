//! 行为事件类型 —— `events.kind` 的唯一取值来源。
//!
//! 数据模型 §3.1 里，所有可统计的行为都进同一张 `events` 表，用 `kind` 区分。
//! 这个设计的用意（ADR-009）是：**不为每种行为建表**。
//! 今天有「喝水」，明天加「远眺」，后者不需要动表结构，只需要多一个 kind。
//!
//! 但松散的字符串字段是 bug 的温床，所以取值在这里被收成一枚举类型，
//! 存储层与统计代码都必须经它读写。

use serde::{Deserialize, Serialize};

/// 一条行为事件的类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BehaviorKind {
    /// 开始一段连续工作。
    WorkStarted,
    /// 工作暂停（进入休息或用户离开）。
    WorkPaused,
    /// 开始一次休息。
    BreakStarted,
    /// 完整完成了休息。
    BreakCompleted,
    /// 跳过了休息。
    BreakSkipped,
    /// 把休息延后了。
    BreakSnoozed,
    /// 记录了一次喝水。
    WaterLogged,
    /// 记录了一次活动 / 站立。
    ActivityLogged,
    /// 记录了一次远眺 / 护眼。
    EyeRestLogged,
    /// 开始空闲（离开）。
    IdleStarted,
    /// 空闲结束（回来）。
    IdleEnded,
}

impl BehaviorKind {
    /// 全部类型，顺序固定（用于导出、统计遍历）。
    pub const ALL: [BehaviorKind; 11] = [
        BehaviorKind::WorkStarted,
        BehaviorKind::WorkPaused,
        BehaviorKind::BreakStarted,
        BehaviorKind::BreakCompleted,
        BehaviorKind::BreakSkipped,
        BehaviorKind::BreakSnoozed,
        BehaviorKind::WaterLogged,
        BehaviorKind::ActivityLogged,
        BehaviorKind::EyeRestLogged,
        BehaviorKind::IdleStarted,
        BehaviorKind::IdleEnded,
    ];

    /// 数据库里的取值，与数据模型 §3.1 的注释逐条对应。
    pub const fn as_str(self) -> &'static str {
        match self {
            BehaviorKind::WorkStarted => "work.started",
            BehaviorKind::WorkPaused => "work.paused",
            BehaviorKind::BreakStarted => "break.started",
            BehaviorKind::BreakCompleted => "break.completed",
            BehaviorKind::BreakSkipped => "break.skipped",
            BehaviorKind::BreakSnoozed => "break.snoozed",
            BehaviorKind::WaterLogged => "water.logged",
            BehaviorKind::ActivityLogged => "activity.logged",
            BehaviorKind::EyeRestLogged => "eye_rest.logged",
            BehaviorKind::IdleStarted => "idle.started",
            BehaviorKind::IdleEnded => "idle.ended",
        }
    }

    /// 从数据库字符串还原。
    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == s)
    }

    /// 界面显示名。
    pub const fn display_name(self) -> &'static str {
        match self {
            BehaviorKind::WorkStarted => "开始工作",
            BehaviorKind::WorkPaused => "暂停工作",
            BehaviorKind::BreakStarted => "开始休息",
            BehaviorKind::BreakCompleted => "完成休息",
            BehaviorKind::BreakSkipped => "跳过休息",
            BehaviorKind::BreakSnoozed => "延后休息",
            BehaviorKind::WaterLogged => "喝水",
            BehaviorKind::ActivityLogged => "活动",
            BehaviorKind::EyeRestLogged => "远眺",
            BehaviorKind::IdleStarted => "离开",
            BehaviorKind::IdleEnded => "回来",
        }
    }

    /// 这条事件是否代表「用户做了一件对健康有正面意义的事」。
    ///
    /// v0.1 暂时只用它做日志颜色的区分；v0.3 的统计页会用它区分
    /// 「完成率」与「打断率」这类指标。放在这里是为了避免各处重复写 match。
    pub const fn is_healthy_action(self) -> bool {
        matches!(
            self,
            BehaviorKind::BreakCompleted
                | BehaviorKind::WaterLogged
                | BehaviorKind::ActivityLogged
                | BehaviorKind::EyeRestLogged
        )
    }

    /// 这条事件是否属于「用户对提醒的回应」。
    ///
    /// 与 [`InterventionOutcome`](crate::model::InterventionOutcome) 一一对应：
    /// 干预记录里存 outcome，事件表里存对应的行为，两边口径必须一致。
    pub const fn is_response(self) -> bool {
        matches!(
            self,
            BehaviorKind::BreakCompleted | BehaviorKind::BreakSkipped | BehaviorKind::BreakSnoozed
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 字符串往返一致() {
        for kind in BehaviorKind::ALL {
            assert_eq!(BehaviorKind::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(BehaviorKind::parse("teleport.logged"), None);
    }

    #[test]
    fn 类型之间没有重复取值() {
        let mut seen: Vec<&str> = Vec::new();
        for kind in BehaviorKind::ALL {
            assert!(!seen.contains(&kind.as_str()), "{} 重复", kind.as_str());
            seen.push(kind.as_str());
        }
        assert_eq!(seen.len(), BehaviorKind::ALL.len());
    }

    #[test]
    fn 取值符合域点分格式() {
        // 数据模型里所有 kind 都长这样：域.动作。
        // 域部分可以是蛇形（eye_rest），动作部分是单个小写单词。
        // 这条测试防止有人写出 "waterLogged" 或 "water_logged" 之类的不一致形式。
        for kind in BehaviorKind::ALL {
            let name = kind.as_str();
            let (domain, action) = name
                .split_once('.')
                .unwrap_or_else(|| panic!("{name} 应当采用「域.动作」格式，例如 water.logged"));

            assert!(!domain.is_empty(), "{name} 的域部分不能为空");
            assert!(!action.is_empty(), "{name} 的动作部分不能为空");
            assert!(!action.contains('.'), "{name} 只应当有一个点分隔符");
            assert!(
                action.chars().all(|c| c.is_ascii_lowercase()),
                "{name} 的动作部分应当是全小写字母（不要下划线或驼峰）"
            );
            assert!(
                domain.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
                "{name} 的域部分只允许小写字母与下划线"
            );
        }
    }

    #[test]
    fn 健康行为与回应事件的分类() {
        assert!(BehaviorKind::WaterLogged.is_healthy_action());
        assert!(BehaviorKind::BreakCompleted.is_healthy_action());
        assert!(!BehaviorKind::BreakSkipped.is_healthy_action());
        assert!(!BehaviorKind::IdleStarted.is_healthy_action());

        assert!(BehaviorKind::BreakSkipped.is_response());
        assert!(BehaviorKind::BreakSnoozed.is_response());
        assert!(!BehaviorKind::WaterLogged.is_response());
    }
}
