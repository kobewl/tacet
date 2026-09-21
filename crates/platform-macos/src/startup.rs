//! 开机自启 —— 库层入口（真正的实现在应用壳层）。
//!
//! ## 这个文件为什么是空的实现
//!
//! 注册登录项需要一个**有 bundle 身份的进程**：系统要知道该登记谁。
//! `platform-macos` 是一个纯库 crate，它没有、也不该有 bundle。
//!
//! 所以职责这样划分：
//!
//! | 谁 | 负责什么 |
//! | --- | --- |
//! | 本模块 | 如实报告「库层不提供这项能力」 |
//! | `apps/desktop` | 真正的注册 / 注销 / 查询状态 |
//!
//! 这与通知能力是同一类问题（见 `lib.rs` 里的 `MacNotificationService`）：
//! **凡是需要应用身份的能力，实现都在壳层。**
//! 库层能做的最有价值的事是别假装自己能干 —— 假装成功会让用户在设置页
//! 打开开关却什么也没发生，那是最糟糕的体验。
//!
//! ## 壳层用的是什么机制
//!
//! 往 `~/Library/LaunchAgents/` 写一个 plist（`tauri-plugin-autostart`
//! 的 `MacosLauncher::LaunchAgent`）。取舍写在 `apps/desktop/src/autostart.rs`
//! 的模块头部，这里只记结论：因为 Tacet 是**未签名**的个人测试包，
//! 而 `SMAppService` 对未签名应用有兼容风险。
//!
//! ## 将来若完成 Apple 签名，这里应该升级
//!
//! `SMAppService`（macOS 13+）是 Apple 现在的正统做法：系统代管登录项、
//! 应用卸载时自动清理、状态随时可查。它比写 plist 更干净，代价是
//! **要求应用有稳定签名**。
//!
//! 等 Tacet 拿到 Apple 开发者账号并完成签名后，壳层那一处可以换成
//! `SMAppService`。本文件与它的公开 API 不需要变 ——
//! 这正是「实现放壳层」这个划分的好处。

use tacet_platform::{Capability, PlatformError, Result};

/// 开机自启服务（库层入口）。
#[derive(Debug, Clone, Copy, Default)]
pub struct MacStartupService {
    _private: (),
}

impl MacStartupService {
    /// 新建。
    pub fn new() -> Self {
        Self { _private: () }
    }
}

impl tacet_platform::StartupService for MacStartupService {
    fn is_enabled(&self) -> Result<bool> {
        Err(PlatformError::Unsupported(Capability::StartupLaunch))
    }

    fn set_enabled(&self, _enabled: bool) -> Result<()> {
        Err(PlatformError::Unsupported(Capability::StartupLaunch))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tacet_platform::StartupService;

    #[test]
    fn 库层如实报告自启不可用() {
        // 纯库 crate 没有 bundle 身份，注册登录项无法工作。
        // 重要的是**如实报告**而不是假装成功 —— 假装会让用户在设置页
        // 打开开关却什么也没发生。
        let service = MacStartupService::new();
        let err = service
            .is_enabled()
            .expect_err("库层不应当假装能查询自启状态");

        assert!(err.is_degradable(), "能力缺失必须走降级路径");
        assert_eq!(err, PlatformError::Unsupported(Capability::StartupLaunch));
    }

    #[test]
    fn 切换自启也如实报告不可用() {
        let service = MacStartupService::new();
        assert!(service.set_enabled(true).is_err());
        assert!(service.set_enabled(false).is_err());
    }

    #[test]
    fn 服务可以跨线程共享() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<MacStartupService>();
    }
}
