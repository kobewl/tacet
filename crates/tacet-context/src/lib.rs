//! # tacet-context —— 上下文引擎
//!
//! 它把平台层的三样原始信号 —— **前台应用、空闲时长、全屏状态** ——
//! 变成一句人话：**「用户现在在干什么，方便被打扰吗」**。
//!
//! ## 这个 crate 在整条链路上的位置
//!
//! ```text
//!   tacet-platform（平台信号）
//!          │  foreground_app() / idle_seconds() / is_fullscreen()
//!          ▼
//!   tacet-context（本 crate）  ← 把信号合成 ContextSnapshot
//!          │
//!          ▼
//!   tacet-core::policy（决策）  ← 读快照，决定要不要说话、说多大声
//! ```
//!
//! 注意依赖方向：`context → platform`，而 `core` 不认识 `context`。
//! 之所以能这样，是因为**数据形状**（`ContextSnapshot`）定义在 core 里，
//! 本 crate 只负责生产它。这样决策引擎不需要依赖任何平台相关的东西 ——
//! 测试时构造一个快照就能验证全部决策逻辑。
//!
//! ## v0.1 的诚实边界
//!
//! v0.1 只做三件事：前台应用、空闲、全屏。**会议概率恒为 0**，
//! 专注概率也恒为 0 —— 它们属于 v0.2 的 Interruptibility 五因子模型。
//!
//! 但结构已经预留好了：快照里那两个字段一直都在，将来填上真实值即可，
//! 决策引擎不用改一行。

pub mod category;
pub mod engine;

pub use category::classify;
pub use engine::ContextEngine;
