//! 上下文引擎 —— 把平台信号合成 [`ContextSnapshot`]。
//!
//! ## 核心设计：缺失的信号用「上一次的值」而不是「空」
//!
//! 平台调用随时可能失败（用户切换应用的瞬间、系统忙于别的事）。
//! 如果每次失败都返回一个空快照，会出现一种很难查的现象：
//! **提醒系统间歇性失灵** —— 恰好在那几次采样失败时，决策看到「什么都不知道」
//! 于是选择了沉默。
//!
//! 所以引擎会**记住上一次成功的值**，并在新采样失败时沿用旧值，
//! 同时记录「这次的数据不新鲜」。这比「空」更接近事实：
//! 用户的前台应用不会因为一次采样失败就真的变成「未知」。
//!
//! ## 采样频率
//!
//! 架构文档 §8 定的预算：空闲 10 秒一次、前台应用事件驱动。
//! v0.1 的实现简单：壳层每 10 秒调一次 [`ContextEngine::sample`]，
//! 前台应用的变化由平台层的事件驱动更新（见 [`ContextEngine::observe_app`]）。

use tacet_core::model::{ContextSnapshot, ForegroundApp};
use tacet_core::Timestamp;
use tacet_platform::{Platform, PlatformError};

use crate::category::classify;

/// 上下文引擎。
///
/// 它是有状态的，但状态只有「上一次的观测」这一个用途 ——
/// 用来在采样失败时兜底，不参与任何决策。
pub struct ContextEngine {
    /// 上一次成功读到的前台应用。
    last_app: Option<ForegroundApp>,
    /// 上一次成功读到的空闲秒数。
    last_idle_seconds: u32,
    /// 上一次成功读到的全屏状态。
    last_fullscreen: bool,
    /// 从上一次「全部字段都新鲜」到现在，有没有发生过采样失败。
    has_stale_data: bool,
}

impl Default for ContextEngine {
    fn default() -> Self {
        Self::new()
    }
}

impl ContextEngine {
    /// 新建一个引擎（还没有任何观测）。
    pub fn new() -> Self {
        Self {
            last_app: None,
            last_idle_seconds: 0,
            last_fullscreen: false,
            has_stale_data: false,
        }
    }

    /// 采集一次上下文。
    ///
    /// 即使平台的每一项调用都失败，这个方法也**不会返回错误** ——
    /// 它总会给出一个可用的快照（可能带着旧数据）。
    /// 这是产品要求：拿不到上下文不是故障，只是「知道得少一点」。
    pub fn sample(&mut self, platform: &dyn Platform, now: Timestamp) -> ContextSnapshot {
        let mut fresh = true;

        // ① 前台应用
        match platform.window().foreground_app() {
            Ok(app) => {
                // 平台层给的是「原始名字」，分类在这里做 ——
                // 让平台层只负责「读到什么」，业务含义由上下文层赋予。
                let category = classify(&app.bundle_id, &app.name);
                let app = ForegroundApp::new(app.bundle_id, app.name, category);
                self.last_app = Some(app);
            }
            Err(_) => fresh = false,
        }

        // ② 空闲时长
        match platform.idle().idle_seconds() {
            Ok(seconds) => self.last_idle_seconds = seconds,
            Err(_) => fresh = false,
        }

        // ③ 全屏状态
        match platform.window().is_fullscreen() {
            Ok(fullscreen) => self.last_fullscreen = fullscreen,
            Err(_) => fresh = false,
        }

        self.has_stale_data = !fresh;

        ContextSnapshot {
            sampled_at: now,
            foreground_app: self.last_app.clone(),
            idle_seconds: self.last_idle_seconds,
            fullscreen: self.last_fullscreen,
            // v0.2 之前这两项恒为 0：会议检测与专注度建模还没做。
            // 留着字段是为了让决策引擎的接口形状在这个版本就固定下来。
            meeting_probability: 0.0,
            focus_probability: 0.0,
        }
    }

    /// 显式记录一次前台应用变化（事件驱动路径）。
    ///
    /// 壳层收到系统的「应用切换」事件时调用它，比等下一次轮询更及时。
    pub fn observe_app(&mut self, app: ForegroundApp) {
        let category = classify(&app.bundle_id, &app.name);
        self.last_app = Some(ForegroundApp::new(app.bundle_id, app.name, category));
    }

    /// 显式记录一次空闲秒数（用于让引擎立刻反映最新的输入状态）。
    pub fn observe_idle(&mut self, idle_seconds: u32) {
        self.last_idle_seconds = idle_seconds;
    }

    /// 显式记录一次全屏变化。
    pub fn observe_fullscreen(&mut self, fullscreen: bool) {
        self.last_fullscreen = fullscreen;
    }

    /// 上一次采样的数据里有没有「沿用旧值」的字段。
    ///
    /// 界面可以在诊断页显示它，用来回答「为什么有时候判断不准」。
    /// **不要用它来提示普通用户** —— 那属于实现细节，不是用户的问题。
    pub const fn has_stale_data(&self) -> bool {
        self.has_stale_data
    }

    /// 清空记忆（很少用：用户手动重置数据时）。
    pub fn reset(&mut self) {
        *self = Self::new();
    }

    /// 当前记住的前台应用。
    pub fn last_app(&self) -> Option<&ForegroundApp> {
        self.last_app.as_ref()
    }
}

/// 判断一个平台错误是否表示「这项能力这台机器上没有」。
///
/// 提供给壳层做启动自检：如果 v0.1 必需的能力缺失，就写一条启动日志
/// （**不是弹窗** —— 产品原则 7 说能力缺失不该变成打扰）。
pub fn capability_missing(err: &PlatformError) -> bool {
    matches!(err, PlatformError::Unsupported(_))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tacet_core::model::AppCategory;
    use tacet_platform::{Capability, MockPlatform};

    fn t0() -> Timestamp {
        Timestamp::from_millis(1_700_000_000_000)
    }

    #[test]
    fn 从平台信号合成快照() {
        let platform = MockPlatform::new();
        platform.control.set_foreground_app(
            "com.microsoft.VSCode",
            "Visual Studio Code",
            AppCategory::Editor,
        );
        platform.control.set_idle_seconds(120);

        let mut engine = ContextEngine::new();
        let snapshot = engine.sample(&platform, t0());

        assert_eq!(snapshot.sampled_at, t0());
        assert_eq!(snapshot.idle_seconds, 120);
        let app = snapshot.foreground_app.as_ref().expect("应当有前台应用");
        assert_eq!(app.bundle_id, "com.microsoft.VSCode");
        assert!(!snapshot.fullscreen);
    }

    #[test]
    fn 应用分类由上下文层完成() {
        // 平台层只回报原始信息，分类是上下文层的职责 ——
        // 这样换一个平台实现时，分类规则不用重写。
        let platform = MockPlatform::new();
        platform
            .control
            .set_foreground_app("us.zoom.xos", "zoom.us", AppCategory::Other);

        let mut engine = ContextEngine::new();
        let snapshot = engine.sample(&platform, t0());

        let app = snapshot.foreground_app.expect("应当有应用");
        assert_eq!(
            app.category,
            AppCategory::Meeting,
            "应当根据 Bundle ID 重新分类，而不是照抄平台层的分类"
        );
    }

    #[test]
    fn 全屏状态被反映() {
        let platform = MockPlatform::new();
        platform.control.set_fullscreen(true);

        let mut engine = ContextEngine::new();
        let snapshot = engine.sample(&platform, t0());

        assert!(snapshot.fullscreen);
        assert!(
            snapshot.prefers_gentle_intervention(),
            "全屏时应当倾向于温和的干预方式"
        );
    }

    #[test]
    fn 采样失败时沿用上一次的值() {
        // 这是本模块最重要的行为：一次采样失败不该让系统「失忆」，
        // 否则会出现「提醒偶尔莫名其妙不触发」这种极难排查的问题。
        let platform = MockPlatform::new();
        platform
            .control
            .set_foreground_app("com.apple.Safari", "Safari", AppCategory::Browser);
        platform.control.set_idle_seconds(30);

        let mut engine = ContextEngine::new();
        let first = engine.sample(&platform, t0());
        assert!(!engine.has_stale_data());

        // 平台能力整体失效
        for capability in [
            Capability::ForegroundApp,
            Capability::IdleDetection,
            Capability::FullscreenDetection,
        ] {
            platform.control.disable(capability);
        }

        let second = engine.sample(&platform, t0().saturating_add_millis(10_000));

        assert!(engine.has_stale_data(), "应当标记数据不新鲜");
        assert_eq!(
            second.foreground_app, first.foreground_app,
            "应当沿用上一次的应用信息"
        );
        assert_eq!(second.idle_seconds, 30, "应当沿用上一次的空闲时长");
    }

    #[test]
    fn 恢复采样后不再标记为陈旧() {
        let platform = MockPlatform::new();
        platform.control.disable(Capability::IdleDetection);

        let mut engine = ContextEngine::new();
        engine.sample(&platform, t0());
        assert!(engine.has_stale_data());

        platform.control.enable(Capability::IdleDetection);
        platform.control.set_idle_seconds(5);
        let snapshot = engine.sample(&platform, t0());

        assert!(!engine.has_stale_data());
        assert_eq!(snapshot.idle_seconds, 5);
    }

    #[test]
    fn 平台全不可用时也能给出快照而不崩溃() {
        let platform = MockPlatform::without_capabilities();
        let mut engine = ContextEngine::new();

        let snapshot = engine.sample(&platform, t0());

        assert!(snapshot.foreground_app.is_none());
        assert_eq!(snapshot.idle_seconds, 0);
        assert!(!snapshot.fullscreen);
        // 关键：不 panic、不返回 Err，决策引擎照常能跑（只是知道得少）
        assert_eq!(snapshot.app_name(), "未知应用");
    }

    #[test]
    fn 事件驱动的观测立即生效() {
        let mut engine = ContextEngine::new();

        engine.observe_app(ForegroundApp::new(
            "com.google.Chrome",
            "Google Chrome",
            AppCategory::Other,
        ));
        engine.observe_idle(300);
        engine.observe_fullscreen(true);

        // 即使平台什么都没提供，引擎也已经知道了这些
        let snapshot = engine.sample(&MockPlatform::without_capabilities(), t0());

        assert_eq!(snapshot.idle_seconds, 300, "事件观测优先于平台失败");
        assert!(snapshot.fullscreen);
        assert_eq!(snapshot.app_name(), "Google Chrome");
    }

    #[test]
    fn 事件观测的应用也会被重新分类() {
        let mut engine = ContextEngine::new();
        engine.observe_app(ForegroundApp::new(
            "com.microsoft.VSCode",
            "Visual Studio Code",
            AppCategory::Other,
        ));

        let app = engine.last_app().expect("应当记住应用");
        assert_eq!(app.category, AppCategory::Editor);
    }

    #[test]
    fn 重置会清空记忆() {
        let platform = MockPlatform::new();
        let mut engine = ContextEngine::new();

        engine.sample(&platform, t0());
        assert!(engine.last_app().is_some());

        engine.reset();

        assert!(engine.last_app().is_none());
        assert!(!engine.has_stale_data());
    }

    #[test]
    fn 空闲时长用于判定离开() {
        let platform = MockPlatform::new();
        platform.control.set_idle_seconds(600);

        let mut engine = ContextEngine::new();
        let snapshot = engine.sample(&platform, t0());

        assert!(snapshot.is_away(300), "10 分钟空闲应当判定为离开");
        assert!(!snapshot.is_away(900));
    }

    #[test]
    fn 会议与专注概率在基线版本恒为零() {
        // 这两个字段是给 v0.2 预留的，v0.1 必须保持为 0 ——
        // 如果哪天有人填了假数据进来，决策引擎会基于虚构的信息做判断。
        let platform = MockPlatform::new();
        platform.control.set_microphone_in_use(true);

        let mut engine = ContextEngine::new();
        let snapshot = engine.sample(&platform, t0());

        assert_eq!(
            snapshot.meeting_probability, 0.0,
            "v0.1 不做会议判断，绝不能凭麦克风占用就填一个概率"
        );
        assert_eq!(snapshot.focus_probability, 0.0);
    }

    #[test]
    fn 能力缺失可以被识别出来供启动自检() {
        let err = PlatformError::Unsupported(Capability::IdleDetection);
        assert!(capability_missing(&err));

        let err = PlatformError::System("IO 错误".to_string());
        assert!(!capability_missing(&err), "系统故障不是「能力缺失」");
    }
}
