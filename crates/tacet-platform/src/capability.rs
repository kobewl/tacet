//! 能力探测 —— 「这台机器现在能做什么」。
//!
//! 架构原则 6 要求每个能力接口都返回 `Result`，让上层能降级。
//! 但光靠「调用失败再降级」不够：界面需要**提前知道**哪些功能该显示成灰色、
//! 哪些开关该隐藏。所以平台实现要能主动汇报自己的能力清单。
//!
//! ## 一个真实的使用场景
//!
//! 用户在设置页看到「休息提醒：全屏」这个选项。如果这台机器
//! 根本没拿到通知权限，那么「系统通知」这一级也用不了 ——
//! 这时候设置页应当把它标注出来，而不是让用户选了以后发现没反应。

use serde::{Deserialize, Serialize};

/// 一项平台能力。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    /// 读取当前空闲时长（多久没输入）。
    IdleDetection,
    /// 读取前台应用。
    ForegroundApp,
    /// 检测全屏状态。
    FullscreenDetection,
    /// 检测麦克风占用（**不录音**）。
    MeetingDetection,
    /// 枚举显示器。
    ScreenEnumeration,
    /// 发送系统通知。
    Notification,
    /// 开机自启。
    StartupLaunch,
}

impl Capability {
    /// 全部能力（用于自检与设置页渲染）。
    pub const ALL: [Capability; 7] = [
        Capability::IdleDetection,
        Capability::ForegroundApp,
        Capability::FullscreenDetection,
        Capability::MeetingDetection,
        Capability::ScreenEnumeration,
        Capability::Notification,
        Capability::StartupLaunch,
    ];

    /// 稳定字符串。
    pub const fn as_str(self) -> &'static str {
        match self {
            Capability::IdleDetection => "idle_detection",
            Capability::ForegroundApp => "foreground_app",
            Capability::FullscreenDetection => "fullscreen_detection",
            Capability::MeetingDetection => "meeting_detection",
            Capability::ScreenEnumeration => "screen_enumeration",
            Capability::Notification => "notification",
            Capability::StartupLaunch => "startup_launch",
        }
    }

    /// 界面显示名。
    pub const fn display_name(self) -> &'static str {
        match self {
            Capability::IdleDetection => "空闲检测",
            Capability::ForegroundApp => "前台应用识别",
            Capability::FullscreenDetection => "全屏检测",
            Capability::MeetingDetection => "会议检测",
            Capability::ScreenEnumeration => "显示器识别",
            Capability::Notification => "系统通知",
            Capability::StartupLaunch => "开机自启",
        }
    }

    /// 这项能力是不是 v0.1 的必需项。
    ///
    /// 用来区分「缺了会降级但能用」和「缺了整个闭环就断了」。
    /// v0.1 的必需项只有三个：空闲检测（计时准确性）、全屏检测（不打扰判断）、
    /// 通知（Level 2 干预）。会议检测属于 v0.2，缺了完全不影响 v0.1。
    pub const fn required_in_v01(self) -> bool {
        matches!(
            self,
            Capability::IdleDetection | Capability::FullscreenDetection | Capability::Notification
        )
    }
}

/// 一台机器上各项能力的可用情况。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityReport {
    /// 可用的能力。
    pub available: Vec<Capability>,
    /// 不可用的能力，以及原因。
    pub unavailable: Vec<(Capability, String)>,
}

impl CapabilityReport {
    /// 空报告（什么都没探测）。
    pub fn empty() -> Self {
        Self {
            available: Vec::new(),
            unavailable: Vec::new(),
        }
    }

    /// 记录一项可用能力。
    pub fn mark_available(&mut self, capability: Capability) {
        if !self.available.contains(&capability) {
            self.available.push(capability);
        }
    }

    /// 记录一项不可用能力及原因。
    pub fn mark_unavailable(&mut self, capability: Capability, reason: impl Into<String>) {
        self.unavailable.push((capability, reason.into()));
    }

    /// 某项能力是否可用。
    pub fn supports(&self, capability: Capability) -> bool {
        self.available.contains(&capability)
    }

    /// v0.1 必需的能力是否齐全。
    pub fn meets_v01_requirements(&self) -> bool {
        Capability::ALL
            .into_iter()
            .filter(|c| c.required_in_v01())
            .all(|c| self.supports(c))
    }

    /// 缺失的 v0.1 必需能力（用于启动日志与自检）。
    pub fn missing_required(&self) -> Vec<Capability> {
        Capability::ALL
            .into_iter()
            .filter(|c| c.required_in_v01() && !self.supports(*c))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 能力字符串往返且不重复() {
        let mut seen = Vec::new();
        for capability in Capability::ALL {
            assert!(!seen.contains(&capability.as_str()));
            seen.push(capability.as_str());
        }
        assert_eq!(seen.len(), Capability::ALL.len());
    }

    #[test]
    fn 空报告不满足启动要求() {
        let report = CapabilityReport::empty();
        assert!(!report.meets_v01_requirements());
        assert_eq!(report.missing_required().len(), 3);
    }

    #[test]
    fn 三件必需能力齐全即可运行() {
        let mut report = CapabilityReport::empty();
        report.mark_available(Capability::IdleDetection);
        report.mark_available(Capability::ForegroundApp);
        report.mark_available(Capability::FullscreenDetection);
        report.mark_available(Capability::Notification);

        assert!(report.meets_v01_requirements());
        assert!(report.missing_required().is_empty());
        assert!(report.supports(Capability::IdleDetection));
    }

    #[test]
    fn 会议检测缺失不影响基线达标() {
        // 会议检测是 v0.2 的能力，v0.1 缺了完全不影响。
        let mut report = CapabilityReport::empty();
        for capability in Capability::ALL {
            if capability != Capability::MeetingDetection {
                report.mark_available(capability);
            }
        }
        report.mark_unavailable(Capability::MeetingDetection, "v0.2 才实现");

        assert!(report.meets_v01_requirements());
        assert!(!report.supports(Capability::MeetingDetection));
    }

    #[test]
    fn 重复标记不会重复记录() {
        let mut report = CapabilityReport::empty();
        report.mark_available(Capability::IdleDetection);
        report.mark_available(Capability::IdleDetection);

        assert_eq!(report.available.len(), 1);
    }
}
