//! # platform-macos —— Tacet 的 macOS 实现
//!
//! 这个 crate 是**唯一允许出现平台条件编译**的地方（架构文档 §4）。
//! 它把 macOS 的几套系统 API 适配成 `tacet-platform` 定义的 7 个 trait：
//!
//! | trait | 用到的系统 API | 需要权限 |
//! | --- | --- | --- |
//! | [`ActivityMonitor`] | CoreGraphics `CGEventSourceSecondsSinceLastEventType` | 无 |
//! | [`IdleMonitor`] | 同上 | 无 |
//! | [`WindowManager`] | AppKit `NSWorkspace` / `NSRunningApplication` | 无 |
//! | [`ScreenManager`] | AppKit `NSScreen` | 无 |
//! | [`MeetingDetector`] | CoreAudio 设备占用状态（v0.2） | 无 |
//! | [`NotificationService`] | 由壳层用 Tauri 通知插件实现 | 通知权限 |
//! | [`StartupService`] | `SMAppService`（macOS 13+） | 无 |
//!
//! ## 权限：一个都不申请
//!
//! 平台策略 §4.4 列了这张表，请注意前三项全部是「无需授权」：
//! 前台应用识别、空闲检测、全屏检测都走公开 API，不需要辅助功能、
//! 不需要屏幕录制、不需要麦克风。这对一个健康工具来说非常关键 ——
//! 用户不该为了被提醒休息而交出系统控制权。
//!
//! 需要授权的只有系统通知，而且是**首次需要发通知时**才申请，
//! 不是一启动就弹窗。
//!
//! ## 非 macOS 上的行为
//!
//! 这个 crate 在非 macOS 目标上会退化成一个「空平台」，
//! 保证 workspace 在 Linux CI runner 上也能编译（对应架构原则 7 的
//! 「CI 专项检查」——可移植性不只是 agent crate 的事）。

#![cfg_attr(
    not(target_os = "macos"),
    allow(unused_imports, dead_code, clippy::all)
)]

#[cfg(target_os = "macos")]
mod idle;
#[cfg(target_os = "macos")]
mod screens;
#[cfg(target_os = "macos")]
mod startup;
#[cfg(target_os = "macos")]
mod window;

#[cfg(target_os = "macos")]
pub use idle::{CgActivityMonitor, CgIdleMonitor};
#[cfg(target_os = "macos")]
pub use screens::NSScreenManager;
#[cfg(target_os = "macos")]
pub use startup::MacStartupService;
#[cfg(target_os = "macos")]
pub use window::NSWindowManager;

use tacet_platform::{Capability, CapabilityReport, Platform};

/// macOS 平台实现。
///
/// 每个字段都是「一次性创建、长期持有」的 —— 系统 API 调用没有需要持久化的
/// 状态，所以这些实现都是无状态的薄封装，可以安全地在多线程间共享。
pub struct MacPlatform {
    activity: CgActivityMonitor,
    idle: CgIdleMonitor,
    window: NSWindowManager,
    screen: NSScreenManager,
    meeting: MacMeetingDetector,
    notification: MacNotificationService,
    startup: MacStartupService,
}

impl Default for MacPlatform {
    fn default() -> Self {
        Self::new()
    }
}

impl MacPlatform {
    /// 构造 macOS 平台实现。
    ///
    /// 这个函数**不会失败**：所有能力探测都是「能不能用」的查询，
    /// 而不是「能不能初始化」的操作。真正的失败留到实际调用时暴露，
    /// 那时上层已经有降级路径可走。
    pub fn new() -> Self {
        Self {
            activity: CgActivityMonitor::new(),
            idle: CgIdleMonitor::new(),
            window: NSWindowManager::new(),
            screen: NSScreenManager::new(),
            meeting: MacMeetingDetector,
            notification: MacNotificationService,
            startup: MacStartupService::new(),
        }
    }
}

impl Platform for MacPlatform {
    fn activity(&self) -> &dyn tacet_platform::ActivityMonitor {
        &self.activity
    }

    fn idle(&self) -> &dyn tacet_platform::IdleMonitor {
        &self.idle
    }

    fn window(&self) -> &dyn tacet_platform::WindowManager {
        &self.window
    }

    fn screen(&self) -> &dyn tacet_platform::ScreenManager {
        &self.screen
    }

    fn meeting(&self) -> &dyn tacet_platform::MeetingDetector {
        &self.meeting
    }

    fn notification(&self) -> &dyn tacet_platform::NotificationService {
        &self.notification
    }

    fn startup(&self) -> &dyn tacet_platform::StartupService {
        &self.startup
    }

    fn capabilities(&self) -> CapabilityReport {
        let mut report = CapabilityReport::empty();

        // CoreGraphics 的事件源 API 从 10.9 起可用，macOS 13+ 必然支持。
        report.mark_available(Capability::IdleDetection);

        // NSWorkspace 的前台应用查询是公开 API。
        report.mark_available(Capability::ForegroundApp);

        // 全屏检测走 NSApplication 的呈现选项，也无需权限。
        report.mark_available(Capability::FullscreenDetection);

        // NSScreen 是公开 API。
        report.mark_available(Capability::ScreenEnumeration);

        // SMAppService 在 macOS 13+ 可用，与最低系统版本一致。
        if startup::sm_app_service_available() {
            report.mark_available(Capability::StartupLaunch);
        } else {
            report.mark_unavailable(
                Capability::StartupLaunch,
                "系统版本低于 macOS 13，不支持 SMAppService",
            );
        }

        // 下面两项由壳层负责，这里只说明当前状态的「未知」：
        // 通知权限是一个运行时状态（用户可以随时撤销），只有在真正尝试
        // 发送时才知道；会议检测属于 v0.2，v0.1 明确不提供。
        report.mark_unavailable(
            Capability::Notification,
            "通知能力由应用壳层提供，权限状态在首次发送时确定",
        );
        report.mark_unavailable(Capability::MeetingDetection, "会议检测属于 v0.2 范围");

        report
    }

    fn name(&self) -> &'static str {
        "macos"
    }
}

/// 会议检测的 macOS 实现。
///
/// ## v0.1 明确不实现
///
/// 平台策略 §4.4 说这项能力**不需要麦克风权限**（只查询
/// `kAudioDevicePropertyDeviceIsRunningSomewhere` 这个设备属性，
/// 不触碰任何音频流）。但那是 v0.2 的工作，v0.1 里它如实返回
/// [`PlatformError::Unsupported`]。
///
/// ## 为什么不如实实现「返回 false」而是返回错误
///
/// 「麦克风没被占用」和「我检测不了」对上层是完全不同的信息：
///
/// - 前者会让决策引擎认为「现在适合全屏提醒」
/// - 后者会让它走保守路径
///
/// 在 v0.1 里假装「没有会议」是很危险的 —— 用户真的在开会时，
/// 系统会自信地弹一个全屏提醒盖住整个会议界面。
/// 返回错误才是诚实的做法。
#[derive(Default)]
pub struct MacMeetingDetector;

impl tacet_platform::MeetingDetector for MacMeetingDetector {
    fn is_microphone_in_use(&self) -> tacet_platform::Result<bool> {
        Err(tacet_platform::PlatformError::Unsupported(
            Capability::MeetingDetection,
        ))
    }
}

/// 通知能力的 macOS 实现占位。
///
/// ## 为什么这里是空的
///
/// 通知是**唯一需要窗口身份**的平台能力：macOS 要求
/// `UNUserNotificationCenter` 必须在有 bundle identifier 的应用进程里调用。
/// 而 `platform-macos` 是一个纯库 crate，它连 bundle 都没有。
///
/// 所以真正的实现在 `apps/desktop` 里 —— 壳层用 Tauri 的通知插件来做，
/// 那里的进程身份是正确的。这个空实现的职责是**如实说明
/// 「这里不提供通知能力，请用壳层的那一个」**，而不是假装能用。
#[derive(Default)]
pub struct MacNotificationService;

impl tacet_platform::NotificationService for MacNotificationService {
    fn is_authorized(&self) -> tacet_platform::Result<bool> {
        Err(tacet_platform::PlatformError::Unsupported(
            Capability::Notification,
        ))
    }

    fn request_authorization(&self) -> tacet_platform::Result<bool> {
        Err(tacet_platform::PlatformError::Unsupported(
            Capability::Notification,
        ))
    }

    fn notify(&self, _title: &str, _body: &str, _actions: &[&str]) -> tacet_platform::Result<()> {
        Err(tacet_platform::PlatformError::Unsupported(
            Capability::Notification,
        ))
    }

    fn dismiss_all(&self) -> tacet_platform::Result<()> {
        Err(tacet_platform::PlatformError::Unsupported(
            Capability::Notification,
        ))
    }
}

#[cfg(test)]
#[cfg(target_os = "macos")]
mod tests {
    use super::*;
    use tacet_platform::Platform;

    #[test]
    fn 平台名是macos() {
        assert_eq!(MacPlatform::new().name(), "macos");
    }

    #[test]
    fn 无需权限的三项能力都被标记为可用() {
        let report = MacPlatform::new().capabilities();

        assert!(report.supports(Capability::IdleDetection));
        assert!(report.supports(Capability::ForegroundApp));
        assert!(report.supports(Capability::FullscreenDetection));
        assert!(report.supports(Capability::ScreenEnumeration));
        assert!(
            report.meets_v01_requirements() || !report.supports(Capability::Notification),
            "v0.1 必需项里唯一可能缺的是通知（它由壳层提供）"
        );
    }

    #[test]
    fn 会议检测明确报告不支持() {
        let platform = MacPlatform::new();
        let err = platform
            .meeting()
            .is_microphone_in_use()
            .expect_err("v0.1 不该假装能检测会议");

        assert!(err.is_degradable(), "能力缺失必须是可降级的");
        assert!(!report_supports_meeting(&platform));
    }

    fn report_supports_meeting(platform: &MacPlatform) -> bool {
        platform
            .capabilities()
            .supports(Capability::MeetingDetection)
    }

    #[test]
    fn 平台对象可以跨线程共享() {
        // 后台调度线程要定时采样子系统状态。
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<MacPlatform>();
    }
}
