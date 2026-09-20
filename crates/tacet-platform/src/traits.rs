//! 七大平台能力接口（ADR-010）。
//!
//! 每个 trait 都刻意保持**窄**：只提供业务真正需要的那一两个方法。
//! 接口越窄，「换一个平台」和「写一个假实现」的成本就越低 ——
//! 这也是为什么这里没有任何「顺便也提供一下」的便利方法。
//!
//! ## 隐私边界写在接口里
//!
//! 请注意每个 trait 都带着它**不做什么**的说明。这不是注释洁癖 ——
//! 接口的形状本身就是隐私承诺的载体：
//!
//! - [`WindowManager`] 只有「前台应用的 Bundle ID」，**拿不到窗口标题**
//! - [`MeetingDetector`] 只有「麦克风是否被占用」，**拿不到音频**
//! - [`ScreenManager`] 只有屏幕尺寸，**拿不到屏幕内容**
//!
//! 想让某个 Agent 未来偷偷多采集一点，就必须先改这些 trait，
//! 而改 trait 会进代码评审 —— 这是设计上的有意为之。

use tacet_core::model::ForegroundApp;
use tacet_core::time::Timestamp;

use crate::capability::{Capability, CapabilityReport};
use crate::error::Result;

/// `ActivityMonitor` —— 用户的输入活跃程度。
///
/// 产品只用它判断「人还在吗」和「心流有多深」，**不记录按了哪些键**。
pub trait ActivityMonitor: Send + Sync {
    /// 距上一次键盘 / 鼠标输入过了多少秒。
    ///
    /// 这是空闲检测的底层数据源，也是 v0.1 计时准确性的基础。
    fn seconds_since_last_input(&self) -> Result<u32>;
}

/// `IdleMonitor` —— 空闲状态。
///
/// 与 [`ActivityMonitor`] 的区别：这个是**语义层**的（「用户离开了 / 回来了」），
/// 前者是**原始数据**。v0.1 的实现里两者共用同一个系统 API，
/// 但分开定义让将来的实现（比如加上「合盖算离开」）有地方落。
pub trait IdleMonitor: Send + Sync {
    /// 当前是否处于空闲状态。
    fn is_idle(&self, threshold_seconds: u32) -> Result<bool>;

    /// 当前空闲了多少秒。
    fn idle_seconds(&self) -> Result<u32>;
}

/// `WindowManager` —— 前台应用与全屏状态。
///
/// **只取 Bundle ID 与显示名，不取窗口标题**（v0.1 的硬约束，见平台策略 §6）。
/// 窗口标题里可能包含文档名、网页标题、聊天对象 —— 那是 P3 级隐私数据。
pub trait WindowManager: Send + Sync {
    /// 当前前台应用。
    ///
    /// 拿不到时返回 `Err`，调用方应当保留上一次的结果或者留空，
    /// 而不是让整个流程失败。
    fn foreground_app(&self) -> Result<ForegroundApp>;

    /// 前台应用是否处于全屏。
    ///
    /// 这个判断决定了会不会弹全屏提醒 —— 用户在放演示时弹全屏是很尴尬的。
    fn is_fullscreen(&self) -> Result<bool>;
}

/// `ScreenManager` —— 显示器。
///
/// **只有几何信息，没有屏幕内容**。Tacet 不做任何形式的截屏。
pub trait ScreenManager: Send + Sync {
    /// 枚举所有显示器。
    fn screens(&self) -> Result<Vec<ScreenInfo>>;
}

/// 一台显示器的信息。
///
/// 只派生 `PartialEq` 而不派生 `Eq`：`scale_factor` 是 `f64`，
/// 浮点数不满足全序关系。这是有意的 —— 显示器几何信息本来就不需要做哈希键。
#[derive(Debug, Clone, PartialEq)]
pub struct ScreenInfo {
    /// 这台显示器是不是主屏。
    pub is_primary: bool,
    /// 逻辑宽度（点）。
    pub width: u32,
    /// 逻辑高度（点）。
    pub height: u32,
    /// 缩放倍率（Retina 为 2.0）。
    pub scale_factor: f64,
}

/// `MeetingDetector` —— 会议占用检测。
///
/// ## 这个接口最重要的部分是「它不做什么」
///
/// 它**不录音、不申请麦克风权限、不读取任何音频数据**，
/// 只查询一个布尔量：「现在有没有应用在用麦克风」。
/// 平台策略 §4.4 把它写成了隐私底线：任何需要「辅助功能 / 屏幕录制 / 麦克风录音」
/// 级别权限的能力，默认答案都是「不做」。
///
/// v0.2 才会用到它（会议概率）。v0.1 的实现可以直接返回 `Unsupported`。
pub trait MeetingDetector: Send + Sync {
    /// 麦克风是否正被某个应用占用。
    fn is_microphone_in_use(&self) -> Result<bool>;
}

/// `NotificationService` —— 系统通知。
pub trait NotificationService: Send + Sync {
    /// 当前是否已经拿到发送通知的授权。
    ///
    /// 界面需要提前知道这件事，才能决定「Level 2」这个选项要不要显示成灰色。
    fn is_authorized(&self) -> Result<bool>;

    /// 请求通知授权（只应在用户主动开启通知时调用，不要一启动就弹）。
    fn request_authorization(&self) -> Result<bool>;

    /// 发一条通知。
    ///
    /// `actions` 是通知上的操作按钮文案，PRD §3.3 要求任何提醒都必须带「稍后」，
    /// 所以调用方传进来的 `actions` 至少要有两个。
    fn notify(&self, title: &str, body: &str, actions: &[&str]) -> Result<()>;

    /// 撤销所有当前显示的通知（用户点「现在休息」后要清场）。
    fn dismiss_all(&self) -> Result<()>;
}

/// `StartupService` —— 开机自启。
///
/// 默认关闭，只有用户显式打开才生效（平台策略 §4.4）。
pub trait StartupService: Send + Sync {
    /// 当前是否已设置为开机自启。
    fn is_enabled(&self) -> Result<bool>;

    /// 打开或关闭开机自启。
    fn set_enabled(&self, enabled: bool) -> Result<()>;
}

/// 平台实现的统一入口。
///
/// 把七个能力收在一个结构里，让壳层只需要持有**一个**对象就能访问全部平台能力。
/// 每个字段都是 `&dyn Trait` 而不是具体类型 —— 测试时整包换成 [`crate::mocks`]
/// 里的假实现即可，业务代码一行都不用改。
pub trait Platform: Send + Sync {
    /// 用户输入活跃度。
    fn activity(&self) -> &dyn ActivityMonitor;
    /// 空闲检测。
    fn idle(&self) -> &dyn IdleMonitor;
    /// 前台应用与全屏。
    fn window(&self) -> &dyn WindowManager;
    /// 显示器。
    fn screen(&self) -> &dyn ScreenManager;
    /// 会议（麦克风占用）检测。
    fn meeting(&self) -> &dyn MeetingDetector;
    /// 系统通知。
    fn notification(&self) -> &dyn NotificationService;
    /// 开机自启。
    fn startup(&self) -> &dyn StartupService;

    /// 这台机器现在能做什么。
    ///
    /// 壳层启动时调一次，结果用于设置页的灰态渲染与启动日志。
    fn capabilities(&self) -> CapabilityReport;

    /// 平台名字（日志用），如 `"macos"`。
    fn name(&self) -> &'static str;

    /// 系统是否即将休眠（v0.1 用于让状态机停下来）。
    ///
    /// 单独放在这里而不是塞进某个 trait，是因为它属于**生命周期事件**
    /// 而不是某种传感器。壳层用系统的休眠通知来驱动它。
    fn now(&self) -> Timestamp {
        Timestamp::now()
    }
}

/// 一个把所有能力都标为「不支持」的空平台。
///
/// 用于两种场景：
///
/// 1. **不支持的目标平台**做编译兜底（比如有人在 Linux 上 `cargo build`）
/// 2. 测试里当作「最简环境」，验证产品在什么都没有时也能跑起来
///
/// 第二种用途其实更有价值 —— 架构原则 6 说「能力缺失时降级运行」，
/// 那就得有一个能被自动化验证的「什么都没有」环境。
pub struct NullPlatform;

impl Platform for NullPlatform {
    fn activity(&self) -> &dyn ActivityMonitor {
        &NullActivity
    }
    fn idle(&self) -> &dyn IdleMonitor {
        &NullIdle
    }
    fn window(&self) -> &dyn WindowManager {
        &NullWindow
    }
    fn screen(&self) -> &dyn ScreenManager {
        &NullScreen
    }
    fn meeting(&self) -> &dyn MeetingDetector {
        &NullMeeting
    }
    fn notification(&self) -> &dyn NotificationService {
        &NullNotification
    }
    fn startup(&self) -> &dyn StartupService {
        &NullStartup
    }

    fn capabilities(&self) -> CapabilityReport {
        let mut report = CapabilityReport::empty();
        for capability in Capability::ALL {
            report.mark_unavailable(capability, "当前平台没有提供实现");
        }
        report
    }

    fn name(&self) -> &'static str {
        "null"
    }
}

/// 所有调用都返回「不支持」。
struct NullActivity;
struct NullIdle;
struct NullWindow;
struct NullScreen;
struct NullMeeting;
struct NullNotification;
struct NullStartup;

fn unsupported(capability: Capability) -> crate::PlatformError {
    crate::PlatformError::Unsupported(capability)
}

impl ActivityMonitor for NullActivity {
    fn seconds_since_last_input(&self) -> Result<u32> {
        Err(unsupported(Capability::IdleDetection))
    }
}

impl IdleMonitor for NullIdle {
    fn is_idle(&self, _threshold_seconds: u32) -> Result<bool> {
        Err(unsupported(Capability::IdleDetection))
    }
    fn idle_seconds(&self) -> Result<u32> {
        Err(unsupported(Capability::IdleDetection))
    }
}

impl WindowManager for NullWindow {
    fn foreground_app(&self) -> Result<ForegroundApp> {
        Err(unsupported(Capability::ForegroundApp))
    }
    fn is_fullscreen(&self) -> Result<bool> {
        Err(unsupported(Capability::FullscreenDetection))
    }
}

impl ScreenManager for NullScreen {
    fn screens(&self) -> Result<Vec<ScreenInfo>> {
        Err(unsupported(Capability::ScreenEnumeration))
    }
}

impl MeetingDetector for NullMeeting {
    fn is_microphone_in_use(&self) -> Result<bool> {
        Err(unsupported(Capability::MeetingDetection))
    }
}

impl NotificationService for NullNotification {
    fn is_authorized(&self) -> Result<bool> {
        Err(unsupported(Capability::Notification))
    }
    fn request_authorization(&self) -> Result<bool> {
        Err(unsupported(Capability::Notification))
    }
    fn notify(&self, _title: &str, _body: &str, _actions: &[&str]) -> Result<()> {
        Err(unsupported(Capability::Notification))
    }
    fn dismiss_all(&self) -> Result<()> {
        Err(unsupported(Capability::Notification))
    }
}

impl StartupService for NullStartup {
    fn is_enabled(&self) -> Result<bool> {
        Err(unsupported(Capability::StartupLaunch))
    }
    fn set_enabled(&self, _enabled: bool) -> Result<()> {
        Err(unsupported(Capability::StartupLaunch))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 空平台的所有能力都不可用() {
        let platform = NullPlatform;
        let report = platform.capabilities();

        for capability in Capability::ALL {
            assert!(!report.supports(capability), "{capability:?} 不该可用");
        }
        assert!(!report.meets_v01_requirements());
        assert_eq!(platform.name(), "null");
    }

    #[test]
    fn 空平台的调用返回不支持而不是崩溃() {
        // 这是「渐进增强」最低限度的保证：什么都没有的机器上，
        // 程序照样能跑起来，只是功能静默缺失。
        let platform = NullPlatform;

        let err = platform.idle().idle_seconds().expect_err("应当返回错误");
        assert!(err.is_degradable());
        assert_eq!(
            err,
            crate::PlatformError::Unsupported(Capability::IdleDetection)
        );

        let err = platform
            .window()
            .foreground_app()
            .expect_err("应当返回错误");
        assert_eq!(
            err,
            crate::PlatformError::Unsupported(Capability::ForegroundApp)
        );

        let err = platform
            .notification()
            .notify("标题", "内容", &["知道了", "稍后"])
            .expect_err("应当返回错误");
        assert!(err.is_degradable());
    }

    #[test]
    fn 错误提示面向用户时不含技术术语() {
        let err = crate::PlatformError::Unsupported(Capability::IdleDetection);
        let hint = err.user_facing_hint();

        assert!(hint.contains("空闲检测"));
        assert!(!hint.contains("Err"), "不该把技术细节暴露给用户：{hint}");
        // 产品原则：能力缺失不能变成打扰
        assert!(
            !hint.contains("请"),
            "提示语气应当是告知，不是要求用户做什么"
        );
    }

    #[test]
    fn 权限问题的提示说明会保持安静() {
        let err = crate::PlatformError::PermissionDenied("通知".to_string());
        let hint = err.user_facing_hint();

        assert!(hint.contains("授权"));
        assert!(hint.contains("安静"), "应当明确告诉用户不会被打扰：{hint}");
        assert!(err.is_degradable());
    }

    #[test]
    fn 系统错误不算可降级而是需要关注() {
        let err = crate::PlatformError::System("IO 错误".to_string());
        assert!(!err.is_degradable(), "系统错误应当被上报而不是静默吞掉");
    }

    #[test]
    fn 屏幕信息包含几何数据() {
        let screen = ScreenInfo {
            is_primary: true,
            width: 1728,
            height: 1117,
            scale_factor: 2.0,
        };

        assert!(screen.is_primary);
        assert_eq!(screen.width, 1728);
    }
}
