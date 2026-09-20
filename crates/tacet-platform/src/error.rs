//! 平台层错误。
//!
//! 这里只定义「平台做不了这件事」的几种情况。业务错误一律不在这里 ——
//! 平台层不该知道任何产品概念。

use thiserror::Error;

use crate::capability::Capability;

/// 平台调用失败的原因。
#[derive(Debug, Error, PartialEq, Eq)]
pub enum PlatformError {
    /// 这个平台根本不提供该能力（例如 Windows 上没有 `NSWorkspace`）。
    ///
    /// 这是**正常情况**，不是故障：它意味着业务层应该降级运行。
    #[error("当前平台不支持该能力：{0:?}")]
    Unsupported(Capability),

    /// 能力存在，但暂时拿不到 —— 通常是权限没给。
    #[error("缺少授权：{0}")]
    PermissionDenied(String),

    /// 底层调用失败了（API 返回了错误码、系统调用中断等）。
    #[error("系统调用失败：{0}")]
    System(String),

    /// 调用成功，但结果不是我们期望的形态。
    #[error("返回值不符合预期：{0}")]
    Unexpected(String),
}

impl PlatformError {
    /// 这是不是「应该降级而不是报错」的情况。
    ///
    /// 业务层的典型用法：
    ///
    /// ```rust
    /// use tacet_platform::PlatformError;
    ///
    /// fn handle(err: &PlatformError) {
    ///     if err.is_degradable() {
    ///         // 静默降级：换个方式继续，或者干脆不做这件事
    ///     }
    /// }
    /// ```
    pub fn is_degradable(&self) -> bool {
        matches!(
            self,
            PlatformError::Unsupported(_) | PlatformError::PermissionDenied(_)
        )
    }

    /// 给用户看的说明（只在需要解释「为什么某个功能没生效」时使用）。
    ///
    /// 注意产品原则 7 与 ADR-011：**能力缺失不应该变成打扰**。
    /// 这段话只出现在设置页的状态说明里，绝不弹窗。
    pub fn user_facing_hint(&self) -> String {
        match self {
            PlatformError::Unsupported(capability) => {
                format!(
                    "当前系统不支持「{}」，相关功能会自动跳过。",
                    capability.display_name()
                )
            }
            PlatformError::PermissionDenied(what) => {
                format!("没有获得「{what}」的授权，相关功能会保持安静。")
            }
            PlatformError::System(detail) => format!("系统报告了一个问题：{detail}"),
            PlatformError::Unexpected(detail) => format!("收到了预期之外的返回：{detail}"),
        }
    }
}

/// 平台层的统一返回类型。
pub type Result<T> = std::result::Result<T, PlatformError>;
