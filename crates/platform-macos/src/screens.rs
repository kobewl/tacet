//! 显示器枚举 —— 基于 `NSScreen`。
//!
//! ## v0.1 只需要知道「有几块屏、多大」
//!
//! 多显示器 Overlay 是 v0.2 的工作（功能清单 F3.6）。v0.1 用它做两件事：
//!
//! 1. **能力探测**：确认系统确实能报出屏幕（否则 Overlay 逻辑有问题）
//! 2. **给全屏检测提供几何信息**（见 `window.rs` 里的保守近似）
//!
//! ## 只读几何，不读内容
//!
//! `NSScreen` 提供的是分辨率、缩放倍率、可用区域这些**几何属性**。
//! 要拿到屏幕内容需要 `CGWindowListCreateImage` 或 ScreenCaptureKit，
//! 那需要屏幕录制权限 —— 平台策略 §4.4 明确说「不需要，也不使用」。
//! 这个模块的存在方式本身就是那条红线的体现。

use objc2_app_kit::NSScreen;
use tacet_platform::{Result, ScreenInfo};

/// 显示器管理器。
#[derive(Debug, Clone, Copy, Default)]
pub struct NSScreenManager {
    _private: (),
}

impl NSScreenManager {
    /// 新建（无状态）。
    pub fn new() -> Self {
        Self { _private: () }
    }
}

impl tacet_platform::ScreenManager for NSScreenManager {
    fn screens(&self) -> Result<Vec<ScreenInfo>> {
        // `NSScreen::screens` 需要主线程标记。
        //
        // ## 关于这里的 `new_unchecked`
        //
        // 严格说，访问 `NSScreen` 应该在主线程上做。但屏幕列表是一个
        // **几乎不变的只读属性**（用户插拔显示器时才会变），从后台线程读它
        // 在实践中是安全的，也是很多 macOS Rust 应用的做法。
        //
        // 之所以接受这个取舍：替代方案是让壳层在主线程上预先采集屏幕信息
        // 再传进来，那会让平台接口多出一层「必须由特定线程初始化」的约束，
        // 而这个约束会一路传染到业务代码里。为了一次配置读取付出这个代价
        // 不划算。
        //
        // 如果将来发现这里真的会出问题（比如某种显示器热插拔场景），
        // 正确的修法是把屏幕信息改成壳层注入的缓存，而不是在这里加锁。
        let mtm = unsafe { objc2::MainThreadMarker::new_unchecked() };
        let screens = NSScreen::screens(mtm);

        let count = screens.count();
        if count == 0 {
            // 一块屏都没有：不应当发生，但真发生时如实报告 ——
            // 假装有一块屏会让 Overlay 逻辑去计算一个不存在的坐标。
            return Err(tacet_platform::PlatformError::Unsupported(
                tacet_platform::Capability::ScreenEnumeration,
            ));
        }

        let main_screen = NSScreen::mainScreen(mtm);

        let mut result = Vec::with_capacity(count);

        for index in 0..count {
            let screen = screens.objectAtIndex(index);

            let frame = screen.frame();

            // 判断是不是主屏：比较指针身份。
            let is_primary = match &main_screen {
                Some(main) => std::ptr::eq(&*screen as *const NSScreen, &**main as *const NSScreen),
                None => index == 0,
            };

            // `backingScaleFactor` 在 Retina 上是 2.0，外接普通屏上是 1.0。
            // 它决定 Overlay 的渲染分辨率 —— 这个值取错会让毛玻璃糊掉。
            let scale_factor = screen.backingScaleFactor();

            result.push(ScreenInfo {
                is_primary,
                width: frame.size.width.max(0.0) as u32,
                height: frame.size.height.max(0.0) as u32,
                scale_factor,
            });
        }

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tacet_platform::ScreenManager;

    #[test]
    fn 至少能报出一块屏幕() {
        let manager = NSScreenManager::new();
        let screens = manager.screens().expect("应当能枚举显示器");

        assert!(!screens.is_empty(), "至少应当有一块屏幕");
    }

    #[test]
    fn 恰好有一块屏时它就是主屏() {
        let manager = NSScreenManager::new();
        let screens = manager.screens().expect("枚举");

        if screens.len() == 1 {
            assert!(screens[0].is_primary, "唯一的屏幕必须是主屏");
        }
    }

    #[test]
    fn 恰好一块主屏() {
        let manager = NSScreenManager::new();
        let screens = manager.screens().expect("枚举");

        let primary_count = screens.iter().filter(|s| s.is_primary).count();
        assert_eq!(primary_count, 1, "应当有且只有一块主屏");
    }

    #[test]
    fn 屏幕尺寸是合理的() {
        let manager = NSScreenManager::new();
        let screens = manager.screens().expect("枚举");

        for screen in screens {
            assert!(screen.width > 0, "屏幕宽度应当是正数");
            assert!(screen.height > 0, "屏幕高度应当是正数");
            // 现实中不会有小于 100 点或大于 10000 点的屏幕；
            // 这个断言防的是「取错了单位」（比如误取了毫米）。
            assert!(
                (100..=10_000).contains(&screen.width),
                "屏幕宽度 {} 不像一个真实值",
                screen.width
            );
            assert!(screen.scale_factor >= 1.0, "缩放倍率至少是 1.0");
            assert!(screen.scale_factor <= 4.0, "缩放倍率不该超过 4.0");
        }
    }

    #[test]
    fn 管理器可以跨线程共享() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<NSScreenManager>();
    }
}
