//! 决策因子：**为什么现在提醒我**。
//!
//! 交互原则 5 说「提醒卡片上永远能找到『为什么』」，功能清单 F6.6 把它列为 P0。
//! 实现上它很朴素：决策引擎每做一次判断，就把用到的依据原样记下来，
//! 交给界面渲染成一句话。
//!
//! ## 这不是装饰，是产品的地基
//!
//! 一个会打断你的程序，如果还说不出理由，用户很快就会把它关掉 —— 而且是永久关掉。
//! 反过来，只要每次打扰都能给出一句站得住脚的解释，用户就愿意给它更多信任额度。
//! 所以 [`Reason`] 既是 UI 文案的来源，也是决策日志（v0.2 `decisions` 表）的核心字段。
//!
//! ## 文案纪律
//!
//! 每条 `to_text()` 都必须遵守 PRD §5：**客观陈述事实，不做道德评判**。
//! 对：「已连续工作 78 分钟」。错：「你已经工作太久了」。

use serde::{Deserialize, Serialize};

use crate::model::NeedKind;

/// 一条决策依据。
///
/// 用结构化的 enum 而不是直接存字符串，是为了让 v0.3 的学习模块能统计
/// 「用户最常在哪种理由下跳过提醒」—— 字符串是没法可靠统计的。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "reason", rename_all = "snake_case")]
pub enum Reason {
    /// 连续工作了多少分钟。
    ContinuousWork { minutes: u32 },
    /// 距离上次真正完成的休息有多久。
    SinceLastBreak { minutes: u32 },
    /// 距离上次记录喝水有多久。
    SinceLastHydration { minutes: u32 },
    /// 距离上次活动打卡有多久。
    SinceLastMovement { minutes: u32 },
    /// 连续看屏幕的时长。
    ScreenTime { minutes: u32 },
    /// 前台应用正处于全屏。
    AppFullscreen { app: String },
    /// 用户当前不在电脑前。
    UserAway,
    /// 勿扰模式开着。
    DoNotDisturb,
    /// 需求还没到提醒阈值。
    NeedBelowThreshold { kind: NeedKind, percent: u32 },
    /// 同类提醒刚刚发过，为了不烦人而主动限流。
    RateLimited { kind: NeedKind, minutes_ago: u32 },
    /// 平台能力不可用，本次判断缺少依据（渐进增强原则 6）。
    ContextUnavailable,
}

impl Reason {
    /// 渲染成给用户看的一句话。
    pub fn to_text(&self) -> String {
        match self {
            Reason::ContinuousWork { minutes } => format!("已连续工作 {minutes} 分钟"),
            Reason::SinceLastBreak { minutes } => format!("距离上次休息 {minutes} 分钟"),
            Reason::SinceLastHydration { minutes } => format!("距离上次喝水 {minutes} 分钟"),
            Reason::SinceLastMovement { minutes } => format!("已经坐着 {minutes} 分钟没起身"),
            Reason::ScreenTime { minutes } => format!("连续看屏幕 {minutes} 分钟"),
            Reason::AppFullscreen { app } => format!("{app} 正在全屏使用"),
            Reason::UserAway => "你刚刚不在电脑前".to_string(),
            Reason::DoNotDisturb => "勿扰模式已开启".to_string(),
            Reason::NeedBelowThreshold { kind, percent } => {
                format!("{}需求 {percent}%，暂时不需要提醒", kind.display_name())
            }
            Reason::RateLimited { kind, minutes_ago } => {
                format!("{}分钟前刚提醒过{}", minutes_ago, kind.display_name())
            }
            Reason::ContextUnavailable => "暂时读不到上下文，按基础规则处理".to_string(),
        }
    }

    /// 这条理由属于哪一类需求（用于分组显示与统计）。
    ///
    /// 返回 `None` 表示它是「通用理由」（如勿扰、限流），不归任何一类需求。
    pub const fn need_kind(&self) -> Option<NeedKind> {
        match self {
            Reason::ContinuousWork { .. } | Reason::SinceLastBreak { .. } => Some(NeedKind::Rest),
            Reason::SinceLastHydration { .. } => Some(NeedKind::Hydration),
            Reason::SinceLastMovement { .. } => Some(NeedKind::Movement),
            Reason::ScreenTime { .. } => Some(NeedKind::EyeRest),
            Reason::NeedBelowThreshold { kind, .. } | Reason::RateLimited { kind, .. } => {
                Some(*kind)
            }
            Reason::AppFullscreen { .. }
            | Reason::UserAway
            | Reason::DoNotDisturb
            | Reason::ContextUnavailable => None,
        }
    }

    /// 把一串理由渲染成多行文案（界面上的「为什么」列表）。
    pub fn render_lines(reasons: &[Reason]) -> Vec<String> {
        reasons.iter().map(Reason::to_text).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 文案是客观陈述而不是指责() {
        // 这条测试是产品原则的守卫：一旦有人写出「你已经工作太久了」这种句子，
        // 它就会红。测试在这里扮演的是「文案评审员」。
        let forbidden = ["又", "应该", "必须", "太久", "还没", "竟然", "居然"];

        let samples = [
            Reason::ContinuousWork { minutes: 78 },
            Reason::SinceLastBreak { minutes: 95 },
            Reason::SinceLastHydration { minutes: 93 },
            Reason::SinceLastMovement { minutes: 62 },
            Reason::ScreenTime { minutes: 40 },
            Reason::AppFullscreen {
                app: "Visual Studio Code".to_string(),
            },
            Reason::UserAway,
            Reason::DoNotDisturb,
            Reason::NeedBelowThreshold {
                kind: NeedKind::Rest,
                percent: 32,
            },
            Reason::RateLimited {
                kind: NeedKind::Hydration,
                minutes_ago: 38,
            },
            Reason::ContextUnavailable,
        ];

        for reason in samples {
            let text = reason.to_text();
            assert!(!text.is_empty(), "文案不能为空");
            for word in forbidden {
                assert!(
                    !text.contains(word),
                    "文案「{text}」含有指责性词汇「{word}」，违反 PRD §5"
                );
            }
        }
    }

    #[test]
    fn 具体文案格式() {
        assert_eq!(
            Reason::ContinuousWork { minutes: 78 }.to_text(),
            "已连续工作 78 分钟"
        );
        assert_eq!(
            Reason::SinceLastHydration { minutes: 93 }.to_text(),
            "距离上次喝水 93 分钟"
        );
        assert_eq!(
            Reason::NeedBelowThreshold {
                kind: NeedKind::Movement,
                percent: 41
            }
            .to_text(),
            "活动需求 41%，暂时不需要提醒"
        );
    }

    #[test]
    fn 理由能追溯到需求类别() {
        assert_eq!(
            Reason::ContinuousWork { minutes: 1 }.need_kind(),
            Some(NeedKind::Rest)
        );
        assert_eq!(
            Reason::ScreenTime { minutes: 1 }.need_kind(),
            Some(NeedKind::EyeRest)
        );
        assert_eq!(Reason::DoNotDisturb.need_kind(), None);
        assert_eq!(Reason::AppFullscreen { app: "X".into() }.need_kind(), None);
    }

    #[test]
    fn 序列化后可以带标签解析回来() {
        let reason = Reason::RateLimited {
            kind: NeedKind::Hydration,
            minutes_ago: 38,
        };

        let json = serde_json::to_string(&reason).expect("序列化不应失败");
        assert!(json.contains("\"reason\":\"rate_limited\""));
        assert!(json.contains("\"kind\":\"hydration\""));
        assert!(json.contains("\"minutes_ago\":38"));

        let back: Reason = serde_json::from_str(&json).expect("反序列化不应失败");
        assert_eq!(back, reason);
    }

    #[test]
    fn 多行渲染保持顺序() {
        let lines = Reason::render_lines(&[
            Reason::ContinuousWork { minutes: 78 },
            Reason::ContextUnavailable,
        ]);

        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0], "已连续工作 78 分钟");
    }
}
