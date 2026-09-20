//! # tacet-core —— Tacet 的领域内核
//!
//! 这个 crate 是整个项目的「地基」，它只做三件事：
//!
//! 1. **领域模型**（[`model`]）—— 用类型把产品概念写死：什么是「需求」「干预等级」「Intent」。
//! 2. **工作状态机**（[`state`]）—— 决定「现在算不算在工作」「连续工作多久了」。
//! 3. **决策引擎**（[`policy`]）—— 回答那个核心问题：**此刻该不该打扰用户，用哪一级方式**。
//!
//! ## 为什么它一行平台代码都没有
//!
//! 架构原则 1 要求「核心与平台解耦」。这不是洁癖：只有把业务逻辑和操作系统隔开，
//! 我们才能在一台没有 macOS 的机器上、在几毫秒内跑完几百个决策用例 ——
//! 而平台相关的东西全部躲在 `tacet-platform` 的 trait 后面（[`tacet_platform`] 里定义，
//! `platform-macos` 提供实现，测试时换成 Mock）。
//!
//! 具体到代码上有一条硬规则：**这个 crate 里不允许出现 `#[cfg(target_os = ...)]`**，
//! CI 会静态检查（研发规范 §3.3 红线 1）。
//!
//! ## 一张图看懂数据怎么流动
//!
//! ```text
//!   平台传感器 ──► 事件总线(Event) ──► 状态机(WorkClock) ──┐
//!                                                       ├──► 决策(PolicyEngine) ──► 干预
//!                       需求评分(HealthNeeds) ◄───────────┘
//! ```
//!
//! 单向、可回放、可测试（架构原则 3、4、5）。

pub mod clock;
pub mod error;
pub mod event;
pub mod model;
pub mod policy;
pub mod state;
pub mod time;

pub use clock::{Clock, SystemClock};
pub use error::CoreError;
pub use time::Timestamp;
