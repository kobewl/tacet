//! 领域模型 —— 用类型描述 Tacet 眼里的世界。
//!
//! 这里的每个类型都对应产品文档里的一个概念，命名刻意与文档保持一致，
//! 这样评审时「文档里说的 X」和「代码里的 X」是同一样东西：
//!
//! | 文档概念 | 代码类型 | 出处 |
//! | --- | --- | --- |
//! | 四类健康需求 | [`need::NeedKind`] | 功能清单 F2 |
//! | 干预等级 0~5 | [`level::InterventionLevel`] | ADR-006 |
//! | 上下文快照 | [`context::ContextSnapshot`] | 功能清单 F5.6 |
//! | 决策因子 | [`reason::Reason`] | 功能清单 F6.6 |
//! | Intent | [`intent::Intent`] | 功能清单 F4 |
//! | 用户偏好 | [`prefs::UserPreferences`] | 功能清单 F10.1 |
//! | 干预记录 | [`intervention::Intervention`] | 数据模型 S1 |
//! | 行为事件类型 | [`behavior::BehaviorKind`] | 数据模型 S1 `events.kind` |

pub mod behavior;
pub mod context;
pub mod intent;
pub mod intervention;
pub mod level;
pub mod need;
pub mod prefs;
pub mod reason;

pub use behavior::BehaviorKind;
pub use context::{AppCategory, ContextSnapshot, ForegroundApp};
pub use intent::{Intent, MAX_INTENT_CHARS};
pub use intervention::{Intervention, InterventionOutcome};
pub use level::InterventionLevel;
pub use need::{HealthNeeds, NeedKind, NeedScore};
pub use prefs::{ReminderRule, ReminderSettings, SettingsKey, UserPreferences};
pub use reason::Reason;
