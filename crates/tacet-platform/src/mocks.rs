//! 测试替身 —— 没有 macOS 也能把整个产品跑起来。
//!
//! 架构原则 5 说「Mock 平台层即可跑通全部核心测试」，这个文件就是那句话的落地。
//! CI 上跑 Linux runner 时，所有平台相关的东西都换成这里的东西。
//!
//! ## 设计要点：可操控，而不是「永远返回固定值」
//!
//! 一个只会返回固定值的假实现，只能测「主流程能跑通」。
//! 真正的 bug 都藏在边界里 —— 权限被拒绝、全屏突然打开、系统不支持某项能力。
//! 所以这里的每一个假实现都可以被**中途改变行为**：
//!
//! ```rust
//! use tacet_platform::mocks::MockPlatform;
//! use tacet_platform::{Capability, Platform};
//!
//! let platform = MockPlatform::new();
//! assert_eq!(platform.idle().idle_seconds().expect("默认可用"), 0);
//!
//! // 模拟用户离开了电脑
//! platform.control.set_idle_seconds(600);
//! assert_eq!(platform.idle().idle_seconds().expect("仍然可用"), 600);
//!
//! // 模拟系统不支持空闲检测（旧系统 / 虚拟机）
//! platform.control.disable(Capability::IdleDetection);
//! assert!(platform.idle().idle_seconds().is_err());
//! ```

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

use tacet_core::model::{AppCategory, ForegroundApp};

use crate::capability::{Capability, CapabilityReport};
use crate::error::{PlatformError, Result};
use crate::traits::{
    ActivityMonitor, IdleMonitor, MeetingDetector, NotificationService, Platform, ScreenInfo,
    ScreenManager, StartupService, WindowManager,
};

/// 已发出的通知（测试断言用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SentNotification {
    /// 标题。
    pub title: String,
    /// 正文。
    pub body: String,
    /// 操作按钮文案。
    pub actions: Vec<String>,
}

/// 控制假平台行为的把手。
///
/// 所有字段都是「可写」的，测试想模拟什么场景就改什么。
#[derive(Debug)]
pub struct MockControl {
    idle_seconds: AtomicU32,
    app: Mutex<ForegroundApp>,
    fullscreen: AtomicBool,
    microphone_in_use: AtomicBool,
    notification_authorized: AtomicBool,
    app_launch_enabled: AtomicBool,
    /// 这些能力被标记为「不支持」，调用会返回 `PlatformError::Unsupported`。
    disabled: Mutex<Vec<Capability>>,
    /// 已发出的通知记录。
    pub sent: Mutex<Vec<SentNotification>>,
}

impl MockControl {
    /// 默认：一切可用、有前台应用（浏览器）、不在全屏、空闲 0 秒。
    fn new() -> Self {
        Self {
            idle_seconds: AtomicU32::new(0),
            app: Mutex::new(ForegroundApp::new(
                "com.apple.Safari",
                "Safari",
                AppCategory::Browser,
            )),
            fullscreen: AtomicBool::new(false),
            microphone_in_use: AtomicBool::new(false),
            notification_authorized: AtomicBool::new(true),
            app_launch_enabled: AtomicBool::new(false),
            disabled: Mutex::new(Vec::new()),
            sent: Mutex::new(Vec::new()),
        }
    }

    /// 设定空闲秒数。
    pub fn set_idle_seconds(&self, seconds: u32) {
        self.idle_seconds.store(seconds, Ordering::SeqCst);
    }

    /// 设定前台应用。
    pub fn set_foreground_app(&self, bundle_id: &str, name: &str, category: AppCategory) {
        if let Ok(mut guard) = self.app.lock() {
            *guard = ForegroundApp::new(bundle_id, name, category);
        }
    }

    /// 设定是否全屏。
    pub fn set_fullscreen(&self, fullscreen: bool) {
        self.fullscreen.store(fullscreen, Ordering::SeqCst);
    }

    /// 设定麦克风是否被占用。
    pub fn set_microphone_in_use(&self, in_use: bool) {
        self.microphone_in_use.store(in_use, Ordering::SeqCst);
    }

    /// 设定通知授权状态。
    pub fn set_notification_authorized(&self, authorized: bool) {
        self.notification_authorized
            .store(authorized, Ordering::SeqCst);
    }

    /// 设定开机自启状态。
    pub fn set_startup_enabled(&self, enabled: bool) {
        self.app_launch_enabled.store(enabled, Ordering::SeqCst);
    }

    /// 让某项能力不可用（模拟旧系统 / 缺少权限）。
    pub fn disable(&self, capability: Capability) {
        if let Ok(mut guard) = self.disabled.lock() {
            if !guard.contains(&capability) {
                guard.push(capability);
            }
        }
    }

    /// 让某项能力恢复可用。
    pub fn enable(&self, capability: Capability) {
        if let Ok(mut guard) = self.disabled.lock() {
            guard.retain(|c| *c != capability);
        }
    }

    /// 读取已发出的通知。
    pub fn sent_notifications(&self) -> Vec<SentNotification> {
        self.sent
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default()
    }

    /// 是否有某项能力被禁用。
    fn is_disabled(&self, capability: Capability) -> bool {
        self.disabled
            .lock()
            .map(|guard| guard.contains(&capability))
            .unwrap_or(false)
    }
}

/// 一个完全可控的假平台。
#[derive(Clone)]
pub struct MockPlatform {
    /// 行为控制把手。
    pub control: Arc<MockControl>,
}

impl Default for MockPlatform {
    fn default() -> Self {
        Self::new()
    }
}

impl MockPlatform {
    /// 新建一个「一切正常」的假平台。
    pub fn new() -> Self {
        Self {
            control: Arc::new(MockControl::new()),
        }
    }

    /// 新建一个「什么都没有」的假平台 —— 用来验证降级路径。
    pub fn without_capabilities() -> Self {
        let platform = Self::new();
        for capability in Capability::ALL {
            platform.control.disable(capability);
        }
        platform
    }
}

impl Platform for MockPlatform {
    fn activity(&self) -> &dyn ActivityMonitor {
        self
    }
    fn idle(&self) -> &dyn IdleMonitor {
        self
    }
    fn window(&self) -> &dyn WindowManager {
        self
    }
    fn screen(&self) -> &dyn ScreenManager {
        self
    }
    fn meeting(&self) -> &dyn MeetingDetector {
        self
    }
    fn notification(&self) -> &dyn NotificationService {
        self
    }
    fn startup(&self) -> &dyn StartupService {
        self
    }

    fn capabilities(&self) -> CapabilityReport {
        let mut report = CapabilityReport::empty();
        for capability in Capability::ALL {
            if self.control.is_disabled(capability) {
                report.mark_unavailable(capability, "测试中标记为不可用");
            } else {
                report.mark_available(capability);
            }
        }
        report
    }

    fn name(&self) -> &'static str {
        "mock"
    }
}

impl ActivityMonitor for MockPlatform {
    fn seconds_since_last_input(&self) -> Result<u32> {
        if self.control.is_disabled(Capability::IdleDetection) {
            return Err(PlatformError::Unsupported(Capability::IdleDetection));
        }
        Ok(self.control.idle_seconds.load(Ordering::SeqCst))
    }
}

impl IdleMonitor for MockPlatform {
    fn is_idle(&self, threshold_seconds: u32) -> Result<bool> {
        Ok(self.idle_seconds()? >= threshold_seconds)
    }

    fn idle_seconds(&self) -> Result<u32> {
        self.seconds_since_last_input()
    }
}

impl WindowManager for MockPlatform {
    fn foreground_app(&self) -> Result<ForegroundApp> {
        if self.control.is_disabled(Capability::ForegroundApp) {
            return Err(PlatformError::Unsupported(Capability::ForegroundApp));
        }
        self.control
            .app
            .lock()
            .map(|guard| guard.clone())
            .map_err(|_| PlatformError::System("假平台的应用信息锁中毒".to_string()))
    }

    fn is_fullscreen(&self) -> Result<bool> {
        if self.control.is_disabled(Capability::FullscreenDetection) {
            return Err(PlatformError::Unsupported(Capability::FullscreenDetection));
        }
        Ok(self.control.fullscreen.load(Ordering::SeqCst))
    }
}

impl ScreenManager for MockPlatform {
    fn screens(&self) -> Result<Vec<ScreenInfo>> {
        if self.control.is_disabled(Capability::ScreenEnumeration) {
            return Err(PlatformError::Unsupported(Capability::ScreenEnumeration));
        }
        Ok(vec![ScreenInfo {
            is_primary: true,
            width: 1728,
            height: 1117,
            scale_factor: 2.0,
        }])
    }
}

impl MeetingDetector for MockPlatform {
    fn is_microphone_in_use(&self) -> Result<bool> {
        if self.control.is_disabled(Capability::MeetingDetection) {
            return Err(PlatformError::Unsupported(Capability::MeetingDetection));
        }
        Ok(self.control.microphone_in_use.load(Ordering::SeqCst))
    }
}

impl NotificationService for MockPlatform {
    fn is_authorized(&self) -> Result<bool> {
        if self.control.is_disabled(Capability::Notification) {
            return Err(PlatformError::Unsupported(Capability::Notification));
        }
        Ok(self.control.notification_authorized.load(Ordering::SeqCst))
    }

    fn request_authorization(&self) -> Result<bool> {
        if self.control.is_disabled(Capability::Notification) {
            return Err(PlatformError::Unsupported(Capability::Notification));
        }
        // 假实现里「请求」就等于「同意」，方便测试主流程。
        self.control.set_notification_authorized(true);
        Ok(true)
    }

    fn notify(&self, title: &str, body: &str, actions: &[&str]) -> Result<()> {
        if self.control.is_disabled(Capability::Notification) {
            return Err(PlatformError::Unsupported(Capability::Notification));
        }
        if !self.control.notification_authorized.load(Ordering::SeqCst) {
            return Err(PlatformError::PermissionDenied("通知".to_string()));
        }

        if let Ok(mut guard) = self.control.sent.lock() {
            guard.push(SentNotification {
                title: title.to_string(),
                body: body.to_string(),
                actions: actions.iter().map(|a| (*a).to_string()).collect(),
            });
        }
        Ok(())
    }

    fn dismiss_all(&self) -> Result<()> {
        if self.control.is_disabled(Capability::Notification) {
            return Err(PlatformError::Unsupported(Capability::Notification));
        }
        if let Ok(mut guard) = self.control.sent.lock() {
            guard.clear();
        }
        Ok(())
    }
}

impl StartupService for MockPlatform {
    fn is_enabled(&self) -> Result<bool> {
        if self.control.is_disabled(Capability::StartupLaunch) {
            return Err(PlatformError::Unsupported(Capability::StartupLaunch));
        }
        Ok(self.control.app_launch_enabled.load(Ordering::SeqCst))
    }

    fn set_enabled(&self, enabled: bool) -> Result<()> {
        if self.control.is_disabled(Capability::StartupLaunch) {
            return Err(PlatformError::Unsupported(Capability::StartupLaunch));
        }
        self.control.set_startup_enabled(enabled);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 默认假平台一切正常() {
        let platform = MockPlatform::new();
        let report = platform.capabilities();

        assert!(
            report.meets_v01_requirements(),
            "默认假平台应当满足 v0.1 要求"
        );
        assert_eq!(platform.name(), "mock");
        assert_eq!(platform.idle().idle_seconds().expect("可用"), 0);
        assert!(!platform.window().is_fullscreen().expect("可用"));
        assert_eq!(
            platform.window().foreground_app().expect("可用").name,
            "Safari"
        );
    }

    #[test]
    fn 可以模拟用户离开() {
        let platform = MockPlatform::new();
        platform.control.set_idle_seconds(600);

        assert_eq!(platform.idle().idle_seconds().expect("可用"), 600);
        assert!(platform.idle().is_idle(300).expect("可用"));
        assert!(!platform.idle().is_idle(900).expect("可用"));
    }

    #[test]
    fn 可以模拟前台应用切换() {
        let platform = MockPlatform::new();
        platform.control.set_foreground_app(
            "com.microsoft.VSCode",
            "Visual Studio Code",
            AppCategory::Editor,
        );

        let app = platform.window().foreground_app().expect("可用");
        assert_eq!(app.bundle_id, "com.microsoft.VSCode");
        assert_eq!(app.category, AppCategory::Editor);
        assert!(app.category.implies_focus());
    }

    #[test]
    fn 可以模拟全屏场景() {
        let platform = MockPlatform::new();
        platform.control.set_fullscreen(true);

        assert!(platform.window().is_fullscreen().expect("可用"));
    }

    #[test]
    fn 可以模拟通知授权被拒绝() {
        let platform = MockPlatform::new();
        platform.control.set_notification_authorized(false);

        assert!(!platform.notification().is_authorized().expect("能力可用"));
        let err = platform
            .notification()
            .notify("标题", "内容", &["知道了"])
            .expect_err("未授权时应当失败");
        assert!(err.is_degradable(), "权限问题应当是可降级的");
    }

    #[test]
    fn 记录发出的通知供断言() {
        let platform = MockPlatform::new();

        platform
            .notification()
            .notify(
                "建议休息一下",
                "你已经连续工作 78 分钟了。",
                &["现在休息", "3 分钟后"],
            )
            .expect("应当发送成功");

        let sent = platform.control.sent_notifications();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].title, "建议休息一下");
        assert_eq!(sent[0].actions, vec!["现在休息", "3 分钟后"]);
    }

    #[test]
    fn 清场会清掉已发通知() {
        let platform = MockPlatform::new();
        platform
            .notification()
            .notify("a", "b", &["c"])
            .expect("应当发送成功");
        assert_eq!(platform.control.sent_notifications().len(), 1);

        platform.notification().dismiss_all().expect("应当成功");
        assert!(platform.control.sent_notifications().is_empty());
    }

    #[test]
    fn 可以模拟某项能力缺失() {
        let platform = MockPlatform::new();
        platform.control.disable(Capability::IdleDetection);

        assert_eq!(
            platform.idle().idle_seconds(),
            Err(PlatformError::Unsupported(Capability::IdleDetection))
        );
        // 其它能力不受影响
        assert!(platform.window().foreground_app().is_ok());
    }

    #[test]
    fn 可以模拟能力恢复() {
        let platform = MockPlatform::new();
        platform.control.disable(Capability::IdleDetection);
        assert!(platform.idle().idle_seconds().is_err());

        platform.control.enable(Capability::IdleDetection);
        assert!(platform.idle().idle_seconds().is_ok());
    }

    #[test]
    fn 全无能力的假平台触发降级路径() {
        let platform = MockPlatform::without_capabilities();
        let report = platform.capabilities();

        assert!(!report.meets_v01_requirements());
        assert_eq!(report.missing_required().len(), 3);
        assert!(platform.idle().idle_seconds().is_err());
        assert!(platform.notification().notify("a", "b", &[]).is_err());
        // 但程序本身不崩，所有调用都只是返回错误
    }

    #[test]
    fn 开机自启默认关闭且可切换() {
        let platform = MockPlatform::new();

        assert!(!platform.startup().is_enabled().expect("可用"));
        platform.startup().set_enabled(true).expect("应当成功");
        assert!(platform.startup().is_enabled().expect("可用"));
    }

    #[test]
    fn 可以模拟麦克风占用() {
        let platform = MockPlatform::new();
        assert!(!platform.meeting().is_microphone_in_use().expect("可用"));

        platform.control.set_microphone_in_use(true);
        assert!(platform.meeting().is_microphone_in_use().expect("可用"));
    }

    #[test]
    fn 假平台可跨线程共享() {
        // 后台调度线程与 UI 线程都要访问平台层，这个保证不能少。
        let platform = MockPlatform::new();
        let clone = platform.clone();

        let handle = std::thread::spawn(move || clone.idle().idle_seconds());

        platform.control.set_idle_seconds(42);
        assert!(handle.join().expect("线程不应 panic").is_ok());
    }
}
