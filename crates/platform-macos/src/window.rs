//! 前台应用与全屏检测 —— 基于 AppKit。
//!
//! ## 隐私边界：这里永远拿不到窗口标题
//!
//! 用 `NSWorkspace.frontmostApplication` 拿到的是
//! `NSRunningApplication`，它能提供的**只有**：
//!
//! - `bundleIdentifier`（如 `com.microsoft.VSCode`）
//! - `localizedName`（如 `Visual Studio Code`）
//!
//! 窗口标题属于另一个 API（`CGWindowListCopyWindowInfo` 的
//! `kCGWindowName`），我们**刻意不去碰它** —— 那里可能有文档名、
//! 网页标题、聊天对象，属于 P3 级隐私数据（架构文档 §9.1）。
//!
//! 换句话说：这个模块的能力上限，就是「知道你在用什么软件」，
//! 而这正是产品需要的全部。

use objc2_app_kit::NSWorkspace;
use objc2_foundation::NSString;
use tacet_core::model::{AppCategory, ForegroundApp};
use tacet_platform::{Capability, PlatformError, Result};

/// 前台应用与全屏状态。
#[derive(Debug, Clone, Copy, Default)]
pub struct NSWindowManager {
    _private: (),
}

impl NSWindowManager {
    /// 新建（无状态）。
    pub fn new() -> Self {
        Self { _private: () }
    }
}

impl tacet_platform::WindowManager for NSWindowManager {
    fn foreground_app(&self) -> Result<ForegroundApp> {
        let workspace = NSWorkspace::sharedWorkspace();
        let app = workspace.frontmostApplication();

        let Some(app) = app else {
            // 理论上总有前台应用（Finder 兜底），拿不到说明处于
            // 登录窗口 / 屏幕锁定之类的特殊状态。这不是故障，
            // 而是「此刻没有前台应用」这个事实。
            return Err(PlatformError::Unsupported(Capability::ForegroundApp));
        };

        let bundle_id = app
            .bundleIdentifier()
            .map(|value: objc2::rc::Retained<NSString>| value.to_string())
            .unwrap_or_else(|| "unknown".to_string());

        let name = app
            .localizedName()
            .map(|value: objc2::rc::Retained<NSString>| value.to_string())
            .unwrap_or_else(|| "未知应用".to_string());

        // 分类留空给上下文层去做（见 tacet-context::category）。
        //
        // 为什么不在平台层分类：分类是**业务规则**（「编辑器算专注场景」
        // 这种判断会随产品演进变化），而平台层的职责是如实回报系统状态。
        // 混在一起会导致「改一条分类规则要动平台代码」，也会让
        // Windows 实现不得不抄一遍同样的规则。
        Ok(ForegroundApp::new(bundle_id, name, AppCategory::Other))
    }

    fn is_fullscreen(&self) -> Result<bool> {
        // ## 这里做的判断
        //
        // 判断「当前是否全屏」在 macOS 上没有完美答案，
        // 因为全屏有两种完全不同的机制：
        //
        // 1. **原生全屏**（绿色按钮 / `Cmd+Ctrl+F`）：应用占据一个独立 Space，
        //    菜单栏和 Dock 都隐藏。这是用户认知里的「全屏」。
        // 2. **最大化窗口**：窗口铺满屏幕可见区域，但菜单栏还在。
        //
        // 对 Tacet 来说，第 1 种是**必须**识别的 —— 用户在全屏看演示、
        // 开会共享屏幕、看电影时，一个全屏提醒会非常尴尬。
        // 第 2 种相对安全（菜单栏还在，说明用户没有进入沉浸状态），
        // 而且从外部可靠地区分两者需要辅助功能权限，代价太高。
        //
        // 所以我们用 `NSScreen.visibleFrame` 与主屏尺寸的关系做一个
        // **保守近似**：可见区域等于整个屏幕高度时，认为处于全屏。
        // 这个判断只会「多报一点全屏」，而多报的后果是提醒变温和 ——
        // 这正是我们希望的错误方向。
        let screens =
            unsafe { objc2_app_kit::NSScreen::screens(objc2::MainThreadMarker::new_unchecked()) };

        let Some(main) = screens.firstObject() else {
            // 一块屏幕都报不出来：当作「无法判断」，让上层走保守路径。
            return Err(PlatformError::Unsupported(Capability::FullscreenDetection));
        };

        let frame = main.frame();
        let visible = main.visibleFrame();

        // 可见区域与整屏高度一致 → 菜单栏和 Dock 都被隐藏了 → 全屏。
        //
        // 用一个小容差：macOS 的坐标计算会留下不到 1 点的舍入误差。
        let tolerance = 1.0;
        let fullscreen = (frame.size.height - visible.size.height).abs() <= tolerance;

        Ok(fullscreen)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tacet_platform::WindowManager;

    #[test]
    fn 能读到前台应用() {
        let manager = NSWindowManager::new();
        let app = manager.foreground_app();

        // CI 上可能没有前台应用（无头环境），所以两种情况都算通过 ——
        // 关键是不能 panic。
        if let Ok(app) = app {
            assert!(!app.name.is_empty(), "应用名不该为空");
            assert!(
                !app.bundle_id.is_empty(),
                "Bundle ID 不该为空（拿不到时应当是 unknown）"
            );
        }
    }

    #[test]
    fn 能判断全屏状态而不崩溃() {
        let manager = NSWindowManager::new();
        // 同样：结果取决于当前环境，这里只验证「能给出答案」
        let _ = manager.is_fullscreen();
    }

    #[test]
    fn 前台应用的分类留给上下文层() {
        let manager = NSWindowManager::new();
        if let Ok(app) = manager.foreground_app() {
            assert_eq!(
                app.category,
                AppCategory::Other,
                "平台层不做业务分类，应当统一返回 Other"
            );
        }
    }

    #[test]
    fn 管理器可以跨线程共享() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<NSWindowManager>();
    }
}
