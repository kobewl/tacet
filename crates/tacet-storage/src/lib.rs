//! # tacet-storage —— 本地持久化
//!
//! 这个 crate 管三件事：
//!
//! 1. **数据库本身**（[`db`]）—— 打开、配置、事务
//! 2. **结构演进**（[`migration`]）—— 版本化迁移、备份、降级保护
//! 3. **读写数据**（[`repo`]）—— 四张表的仓储
//!
//! 另外还有一个不那么显眼但同样重要的模块：[`datewin`] 负责**统一日期口径**。
//!
//! ## 两条与产品直接相关的设计
//!
//! ### 一、为什么「今天」只能在这里算
//!
//! 数据模型 §8 把这条写成了工程约束，还标了「历史教训」四个字：
//!
//! > 所有"今天/本周/几点"的计算必须在 storage 层用统一的日期工具函数完成，
//! > UI 层禁止自行计算日期边界。
//!
//! 原因是数据库里存 UTC，而用户说「今天」时想的是本地自然日。
//! 一旦两处各算各的，晚上八点以后的行为就会被算进「明天」，
//! 统计数字看起来凭空消失。所以边界计算只允许出现在 [`datewin`] 里。
//!
//! ### 二、数据属于用户
//!
//! 存储原则 5 说「数据属于用户」，具体到代码上是：
//!
//! - 所有数据都在用户目录下的一个文件里，没有云端、没有账号
//! - 迁移前自动备份，且失败不阻断（[`migration`]）
//! - 遇到不认识的设置项**原样保留**，绝不删除（[`repo::SettingsRepo::unknown_keys`]）
//! - 数据库版本比自己新时，拒绝写入并提示用户（而不是冒险改坏数据）
//!
//! ## 四张表的关系
//!
//! ```text
//!   events          一切行为的原始记录（喝水、活动、休息完成…）
//!      │
//!      ├──► 统计：今天喝了几次水
//!      └──► 需求评分：距上次喝水多久
//!
//!   interventions   每次提醒的发出与响应
//!      │
//!      └──► 接受率 = completed / (level >= 2 的总数)
//!
//!   intents         用户手写的「下一步做什么」（P1 隐私，默认不进 AI）
//!
//!   settings        键值对配置（加设置项不用改表结构）
//! ```

pub mod datewin;
pub mod db;
pub mod error;
pub mod migration;
pub mod path;
pub mod repo;

pub use datewin::{DateWindow, LocalOffset, WeekStart};
pub use db::Database;
pub use error::{Result, StorageError};
pub use migration::CURRENT_SCHEMA_VERSION;
pub use repo::{
    EventRepo, EventRow, IntentRepo, InterventionRepo, InterventionStats, SettingsRepo,
};
