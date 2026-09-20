//! 开机自启 —— 基于 `SMAppService`（实现位于应用壳层）。
//!
//! ## 为什么用 `SMAppService` 而不是老办法
//!
//! 历史上 macOS 上做开机自启有两条路，都有问题：
//!
//! 1. **`LSSharedFileList`**（登录项）：需要授权，且在 macOS 13 起被废弃
//! 2. **往 `~/Library/LaunchAgents` 写 plist**：能用，但这是「绕过系统」的
//!    做法 —— 应用卸载后 plist 会残留，用户只能手动清理
//!
//! `SMAppService`（macOS 13+）是 Apple 现在的正统做法：系统代管、
//! 卸载时自动清理、状态随时可查。这与平台策略选定的最低版本
//! （macOS 13 Ventura）正好对齐。
//!
//! ## 为什么这个文件里没有真正的注册代码
//!
//! `SMAppService` 有一个硬性前提：**调用方必须是一个有 bundle 的应用**。
//! 它要求能读到一个 `Info.plist` 里的 `SMAppServiceAgent` 声明 ——
//! `platform-macos` 是一个纯库 crate，它没有、也不该有 bundle。
//!
//! 所以职责这样划分：
//!
//! | 谁 | 负责什么 |
//! | --- | --- |
//! | 本模块 | 版本判断（能否使用这项能力）+ 如实报告「库层不提供」 |
//! | `apps/desktop` | 真正的注册 / 注销 / 查询状态 |
//!
//! 这个划分与通知能力是一致的（见 `lib.rs` 里的 `MacNotificationService`）：
//! **凡是需要 bundle 身份的能力，实现都在壳层。**
//! 库层能做的最有价值的事，是别假装自己能干 —— 假装成功会让用户在设置页
//! 打开开关却什么也没发生，那是最糟糕的体验。

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

/// `SMAppService` 在当前系统上是否可用。
///
/// 平台最低支持版本就是 macOS 13，所以运行时实际上总是为真。
/// 保留这个判断有两个用途：
///
/// 1. **显式表达版本依赖** —— 让「为什么最低要 macOS 13」在代码里可查
/// 2. 能力探测用得上：万一将来支持更低版本，这里就是分支点
pub fn sm_app_service_available() -> bool {
    macos_major_version() >= 13
}

/// 读取 macOS 主版本号。
///
/// 用 `NSProcessInfo.operatingSystemVersion` 而不是解析 `sw_vers` 的输出：
/// 后者要起一个子进程，而这是个会被频繁调用的判断。
fn macos_major_version() -> i64 {
    use objc2_foundation::NSProcessInfo;

    NSProcessInfo::processInfo()
        .operatingSystemVersion()
        .majorVersion as i64
}

#[cfg(test)]
mod tests {
    use super::*;
    use tacet_platform::StartupService;

    #[test]
    fn 当前系统版本满足要求() {
        // 平台策略定的最低版本是 macOS 13，开发机与 CI 都应当满足。
        let version = macos_major_version();
        assert!(version >= 13, "当前系统版本 {version} 低于最低要求 13");
        assert!(sm_app_service_available());
    }

    #[test]
    fn 库层如实报告自启不可用() {
        // 纯库 crate 没有 bundle 身份，SMAppService 无法工作。
        // 重要的是**如实报告**而不是假装成功。
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
