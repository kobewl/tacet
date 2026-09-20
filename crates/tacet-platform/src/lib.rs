//! # tacet-platform —— 平台抽象层
//!
//! 这个 crate 定义 **7 个 trait**，把「操作系统能提供什么」抽象成一组纯 Rust 接口。
//! 核心逻辑（core / health / context / storage）只认这些接口，不认识 macOS，
//! 也不认识任何将来会出现的平台。
//!
//! ```text
//!   tacet-core / tacet-health / tacet-context / tacet-storage
//!          │  只 import trait
//!          ▼
//!      tacet-platform  ←── 本 crate：只定义接口，没有任何实现
//!          ▲
//!          ├── platform-macos    （v0.1 实现）
//!          └── platform-windows  （v0.5 实现）
//! ```
//!
//! ## 为什么接口要返回 `Result` 而不是裸值
//!
//! 架构原则 6「渐进增强」：任何一项平台能力都可能不可用 ——
//! 用户没给通知权限、系统版本太老、虚拟机里没有对应硬件。
//! 这时候产品必须**降级运行**，而不是崩掉或者卡住主流程。
//!
//! 所以每个 trait 方法都返回 [`Result`]，并且：
//!
//! - 不可用是**正常返回值**（[`PlatformError::Unsupported`]），不是 panic
//! - 调用方拿到 `Err` 后该怎么降级，由业务层决定（见 [`capability`] 的说明）
//!
//! ## 测试替身
//!
//! 每个 trait 都在 [`mocks`] 里配了可直接用的假实现（[`mocks::MockPlatform`]）。
//! 这让所有核心逻辑都能在没有 macOS 的机器上跑测试 ——
//! CI 的 Linux runner 也能验证决策逻辑，这正是架构原则 5 想要的。

pub mod capability;
pub mod error;
pub mod mocks;
pub mod traits;

pub use capability::{Capability, CapabilityReport};
pub use error::{PlatformError, Result};
pub use mocks::{MockControl, MockPlatform, SentNotification};
pub use traits::{
    ActivityMonitor, IdleMonitor, MeetingDetector, NotificationService, Platform, ScreenInfo,
    ScreenManager, StartupService, WindowManager,
};
