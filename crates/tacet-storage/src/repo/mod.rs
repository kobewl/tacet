//! 仓储层 —— 领域对象与 SQL 之间唯一的翻译层。
//!
//! ## 为什么要有这一层
//!
//! 如果把 SQL 散落在业务代码里，会立刻遇到三个问题：
//!
//! 1. **口径分裂**：同一个「今日喝水次数」，在统计页写一个 SQL、
//!    在通知文案里又写一个，两处迟早对不上（数据模型 §8 点名的历史教训）
//! 2. **隐私泄漏**：某处为了图方便 `SELECT *`，把不该用的字段带进了业务层
//! 3. **测试困难**：业务测试要为每一条 SQL 准备数据
//!
//! 所以约定：**所有 SQL 只出现在 `repo/` 下面**。
//! 业务代码通过领域类型（`Intent` / `Intervention` / `BehaviorKind`）与它交互，
//! 完全不知道表长什么样。

pub mod event;
pub mod intent;
pub mod intervention;
pub mod settings;

pub use event::{EventRepo, EventRow};
pub use intent::IntentRepo;
pub use intervention::{InterventionRepo, InterventionStats};
pub use settings::SettingsRepo;
