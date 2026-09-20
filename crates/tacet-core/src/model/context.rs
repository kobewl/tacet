//! 上下文：用户在做什么，以及现在方便被打扰吗。
//!
//! ## 为什么这个类型定义在 core，而不是 tacet-context
//!
//! 架构文档把 `ContextSnapshot` 画在 `tacet-context` 里，但决策引擎（core）
//! 必须**读**它 —— 如果它定义在 context，core 就得依赖 context，
//! 而 context 又依赖 core，那就成了循环依赖。
//!
//! 正确的解法是把「数据长什么样」和「数据怎么算出来」分开：
//!
//! - **形状**（本文件）放在 core —— 决策引擎、存储、UI 都认得它
//! - **生产它的引擎**（`tacet-context::engine`）依赖 core，负责从平台信号里算出它
//!
//! 依赖方向始终是单向的：`platform → context → core`（架构原则 3）。

use serde::{Deserialize, Serialize};

use crate::time::Timestamp;

/// 前台应用的粗略分类。
///
/// 注意这里存的是**类别**而不是窗口标题 —— 数据模型原则 3 说得直白：
/// 「只存摘要，不存原文」。Tacet 只需要知道「他在写代码」，
/// 不需要、也不应该知道「他在写哪一行代码」。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppCategory {
    /// 编辑器 / IDE —— 大概率在心流中，别打断。
    Editor,
    /// 浏览器。
    Browser,
    /// 会议软件（v0.2 起配合会议概率使用）。
    Meeting,
    /// 即时通讯 / 邮件。
    Communication,
    /// 影音娱乐。
    Media,
    /// 终端。
    Terminal,
    /// 其它 / 没认出来。
    Other,
}

impl AppCategory {
    /// 稳定字符串（写日志、写库用）。
    pub const fn as_str(self) -> &'static str {
        match self {
            AppCategory::Editor => "editor",
            AppCategory::Browser => "browser",
            AppCategory::Meeting => "meeting",
            AppCategory::Communication => "communication",
            AppCategory::Media => "media",
            AppCategory::Terminal => "terminal",
            AppCategory::Other => "other",
        }
    }

    /// 从字符串还原（读库用）。
    pub fn parse(s: &str) -> Self {
        match s {
            "editor" => AppCategory::Editor,
            "browser" => AppCategory::Browser,
            "meeting" => AppCategory::Meeting,
            "communication" => AppCategory::Communication,
            "media" => AppCategory::Media,
            "terminal" => AppCategory::Terminal,
            _ => AppCategory::Other,
        }
    }

    /// 界面显示名。
    pub const fn display_name(self) -> &'static str {
        match self {
            AppCategory::Editor => "编辑器",
            AppCategory::Browser => "浏览器",
            AppCategory::Meeting => "会议",
            AppCategory::Communication => "通讯",
            AppCategory::Media => "影音",
            AppCategory::Terminal => "终端",
            AppCategory::Other => "其它",
        }
    }

    /// 这类应用通常意味着「用户正专注，不宜强打断」。
    ///
    /// v0.1 只用它做一件事：决定全屏提醒要不要降级成通知。
    /// v0.2 的 Interruptibility 五因子模型会把它变成一个连续权重。
    pub const fn implies_focus(self) -> bool {
        matches!(self, AppCategory::Editor | AppCategory::Terminal)
    }
}

/// 前台应用的最小描述。
///
/// **只包含三类信息**：Bundle ID（本地唯一标识）、显示名、类别。
/// 窗口标题、网页地址、文档内容一律不采集（架构文档 §9.1 P3 级数据）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForegroundApp {
    /// 例如 `com.microsoft.VSCode`。
    pub bundle_id: String,
    /// 例如 `Visual Studio Code`。
    pub name: String,
    /// 归类结果。
    pub category: AppCategory,
}

impl ForegroundApp {
    /// 构造。
    pub fn new(
        bundle_id: impl Into<String>,
        name: impl Into<String>,
        category: AppCategory,
    ) -> Self {
        Self {
            bundle_id: bundle_id.into(),
            name: name.into(),
            category,
        }
    }
}

/// 某一时刻对「用户在干什么」的完整理解。
///
/// ## 渐进增强：这个结构允许「什么都不知道」
///
/// 架构原则 6 要求：拿不到上下文时系统要降级运行，而不是罢工。
/// 所以每个字段都可缺失或取保守默认值 ——
/// [`ContextSnapshot::unavailable`] 就是「我现在两眼一抹黑」的合法表达，
/// 决策引擎拿到它时的行为应该退化成 v0.1 最朴素的样子（纯计时提醒）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextSnapshot {
    /// 采样时刻。
    pub sampled_at: Timestamp,
    /// 前台应用；拿不到就是 `None`。
    pub foreground_app: Option<ForegroundApp>,
    /// 已经多久没有输入了（秒）。空闲检测的原始值。
    pub idle_seconds: u32,
    /// 前台应用是否处于全屏。
    pub fullscreen: bool,
    /// 会议概率 0.0~1.0（v0.1 恒为 0：会议检测属于 v0.2）。
    pub meeting_probability: f64,
    /// 专注概率 0.0~1.0（v0.1 恒为 0）。
    pub focus_probability: f64,
}

impl ContextSnapshot {
    /// 「什么都不知道」的快照 —— 平台能力缺失或权限未授予时的合法状态。
    pub fn unavailable(at: Timestamp) -> Self {
        Self {
            sampled_at: at,
            foreground_app: None,
            idle_seconds: 0,
            fullscreen: false,
            meeting_probability: 0.0,
            focus_probability: 0.0,
        }
    }

    /// 用户是否已经离开了（空闲秒数超过阈值）。
    pub const fn is_away(&self, idle_threshold_seconds: u32) -> bool {
        self.idle_seconds >= idle_threshold_seconds
    }

    /// 当前是否处于「不宜全屏打断」的场景。
    ///
    /// v0.1 的判断依据只有两条：应用在全屏、或者前台是编辑器/终端类应用。
    /// 这两条都属于「宁可保守」的规则 —— 它们只会让提醒**变轻**，不会变重。
    pub fn prefers_gentle_intervention(&self) -> bool {
        if self.fullscreen {
            return true;
        }

        self.foreground_app
            .as_ref()
            .is_some_and(|app| app.category.implies_focus())
    }

    /// 前台应用的显示名（没有就用「未知应用」）。
    pub fn app_name(&self) -> &str {
        self.foreground_app
            .as_ref()
            .map_or("未知应用", |app| app.name.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at() -> Timestamp {
        Timestamp::from_millis(1_700_000_000_000)
    }

    #[test]
    fn 未知快照是合法状态且行为保守() {
        let snapshot = ContextSnapshot::unavailable(at());

        assert!(snapshot.foreground_app.is_none());
        assert!(!snapshot.fullscreen);
        // 拿不到上下文时不应该因为「疑似心流」而随意降级，也不应该升级打扰。
        // 这里返回 false 表示「没有理由特殊照顾」，决策引擎会走默认路径。
        assert!(!snapshot.prefers_gentle_intervention());
        assert_eq!(snapshot.app_name(), "未知应用");
    }

    #[test]
    fn 全屏场景倾向于温和提醒() {
        let mut snapshot = ContextSnapshot::unavailable(at());
        snapshot.fullscreen = true;
        assert!(snapshot.prefers_gentle_intervention());
    }

    #[test]
    fn 编辑器与终端被视为专注场景() {
        for category in [AppCategory::Editor, AppCategory::Terminal] {
            let mut snapshot = ContextSnapshot::unavailable(at());
            snapshot.foreground_app = Some(ForegroundApp::new("com.example.app", "App", category));
            assert!(
                snapshot.prefers_gentle_intervention(),
                "{category:?} 应当被视为专注场景"
            );
        }
    }

    #[test]
    fn 浏览器与会议软件不触发专注降级() {
        for category in [
            AppCategory::Browser,
            AppCategory::Meeting,
            AppCategory::Other,
        ] {
            let mut snapshot = ContextSnapshot::unavailable(at());
            snapshot.foreground_app = Some(ForegroundApp::new("com.example.app", "App", category));
            assert!(
                !snapshot.prefers_gentle_intervention(),
                "{category:?} 不应触发专注降级"
            );
        }
    }

    #[test]
    fn 空闲判定() {
        let mut snapshot = ContextSnapshot::unavailable(at());
        snapshot.idle_seconds = 120;

        assert!(!snapshot.is_away(300), "2 分钟没到 5 分钟阈值");
        assert!(snapshot.is_away(60), "2 分钟已超过 1 分钟阈值");
    }

    #[test]
    fn 应用类别字符串往返一致() {
        let categories = [
            AppCategory::Editor,
            AppCategory::Browser,
            AppCategory::Meeting,
            AppCategory::Communication,
            AppCategory::Media,
            AppCategory::Terminal,
            AppCategory::Other,
        ];

        for category in categories {
            assert_eq!(AppCategory::parse(category.as_str()), category);
        }
        // 认不出来的字符串统一落到 Other，而不是报错 ——
        // 未来新增一个应用类别时，旧版本读到新值也不该崩。
        assert_eq!(AppCategory::parse("future_category"), AppCategory::Other);
    }
}
