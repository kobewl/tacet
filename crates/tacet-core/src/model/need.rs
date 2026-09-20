//! 健康需求：Tacet 关心的四件事，以及它们「有多急」。
//!
//! 产品把健康需求抽象成四个互相独立的维度（功能清单 F2）：
//!
//! - **休息**（Rest）—— 连续工作太久了
//! - **喝水**（Hydration）—— 该补水了
//! - **活动**（Movement）—— 久坐，该站起来动一动
//! - **护眼**（EyeRest）—— 盯着屏幕太久了
//!
//! 每一维的强度是 0.0~1.0 的连续值，不是「到点 / 没到点」的布尔量。
//! 这是整个产品「不机械」的基础：`0.58` 和 `0.95` 是完全不同的处境，
//! 前者可以再等等，后者该说话了。

use serde::{Deserialize, Serialize};

/// 四类健康需求，外加一个「融合」形态。
///
/// `Fused` 不是一个真实需求，而是**多个需求合并成一次干预**时的归因标签
/// （功能清单 F6.4，v0.2 交付）。它出现在这里是因为干预记录要能表达
/// 「这次提醒是融合的结果」，但 [`NeedKind::ALL`] 里不含它 ——
/// 融合项本身不参与「谁最急」的比较。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NeedKind {
    /// 休息：连续工作达到阈值。
    Rest,
    /// 喝水。
    Hydration,
    /// 站立 / 活动。
    Movement,
    /// 护眼 / 远眺。
    EyeRest,
    /// 融合干预的归因标签（v0.2 起使用）。
    Fused,
}

impl NeedKind {
    /// 四个真实需求，顺序固定。
    ///
    /// 顺序固定是有意的：当两类需求分数完全相同时，
    /// 决策引擎按这个顺序取第一个，从而保证**同样的输入永远得到同样的决策**。
    /// 不确定性是测试的敌人，也是可解释性的敌人。
    pub const ALL: [NeedKind; 4] = [
        NeedKind::Rest,
        NeedKind::Hydration,
        NeedKind::Movement,
        NeedKind::EyeRest,
    ];

    /// 写入数据库 / 序列化成 JSON 时用的稳定字符串。
    ///
    /// 与数据模型 §3.1 中 `events.kind`、`interventions.kind` 的取值一一对应，
    /// **改动这里等于改数据库口径**，必须同步迁移方案。
    pub const fn as_str(self) -> &'static str {
        match self {
            NeedKind::Rest => "rest",
            NeedKind::Hydration => "hydration",
            NeedKind::Movement => "movement",
            NeedKind::EyeRest => "eye_rest",
            NeedKind::Fused => "fused",
        }
    }

    /// 从数据库字符串还原。
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "rest" => Some(NeedKind::Rest),
            "hydration" => Some(NeedKind::Hydration),
            "movement" => Some(NeedKind::Movement),
            "eye_rest" => Some(NeedKind::EyeRest),
            "fused" => Some(NeedKind::Fused),
            _ => None,
        }
    }

    /// 界面上显示的名字。
    pub const fn display_name(self) -> &'static str {
        match self {
            NeedKind::Rest => "休息",
            NeedKind::Hydration => "喝水",
            NeedKind::Movement => "活动",
            NeedKind::EyeRest => "护眼",
            NeedKind::Fused => "综合提醒",
        }
    }

    /// 菜单栏 / 通知里用的图标字符。
    pub const fn icon(self) -> &'static str {
        match self {
            NeedKind::Rest => "☕",
            NeedKind::Hydration => "💧",
            NeedKind::Movement => "🧍",
            NeedKind::EyeRest => "👁",
            NeedKind::Fused => "🎵",
        }
    }
}

/// 0.0~1.0 的需求强度。
///
/// 用一个 newtype 而不是裸 `f64`，是为了让「一定是合法值」成为**类型保证**：
/// 构造函数会夹取范围，所以下游代码永远不用再写 `if score < 0.0`。
/// 这类「把校验做在构造处」的写法，Rust 社区叫 *parse, don't validate*。
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct NeedScore(f64);

impl NeedScore {
    /// 毫无需求。
    pub const ZERO: Self = Self(0.0);
    /// 需求拉满（比如连续工作远超上限）。
    pub const ONE: Self = Self(1.0);

    /// 构造一个需求分数，超出 [0,1] 的部分被夹取。
    ///
    /// `NaN` / `Infinity` 一律当作 0：一个算坏了的分数不应该变成「最紧急的需求」，
    /// 那会导致莫名其妙的全屏弹窗 —— 健康工具宁可少说话，也不能乱说话。
    pub fn new(value: f64) -> Self {
        if value.is_finite() {
            Self(value.clamp(0.0, 1.0))
        } else {
            Self::ZERO
        }
    }

    /// 取回原始数值（用于展示、比较、写库）。
    pub const fn get(self) -> f64 {
        self.0
    }

    /// 是否达到（≥）某个阈值。
    pub fn at_least(self, threshold: f64) -> bool {
        self.0 >= threshold
    }

    /// 换算成百分数（0~100），给界面显示用。
    pub fn percent(self) -> u32 {
        (self.0 * 100.0).round() as u32
    }
}

impl From<f64> for NeedScore {
    fn from(v: f64) -> Self {
        Self::new(v)
    }
}

/// 四维需求的完整快照。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct HealthNeeds {
    /// 休息需求。
    pub rest: NeedScore,
    /// 喝水需求。
    pub hydration: NeedScore,
    /// 活动需求。
    pub movement: NeedScore,
    /// 护眼需求。
    pub eye_rest: NeedScore,
}

impl Default for HealthNeeds {
    fn default() -> Self {
        Self::none()
    }
}

impl HealthNeeds {
    /// 全零：刚休息完、刚喝过水时的状态。
    pub const fn none() -> Self {
        Self {
            rest: NeedScore::ZERO,
            hydration: NeedScore::ZERO,
            movement: NeedScore::ZERO,
            eye_rest: NeedScore::ZERO,
        }
    }

    /// 按类型取值。
    pub const fn get(&self, kind: NeedKind) -> NeedScore {
        match kind {
            NeedKind::Rest => self.rest,
            NeedKind::Hydration => self.hydration,
            NeedKind::Movement => self.movement,
            NeedKind::EyeRest => self.eye_rest,
            // 融合不是独立需求，问它自己的分数没有意义；返回 0 而不是 panic，
            // 让调用方在忘记分支时得到一个安全的默认值。
            NeedKind::Fused => NeedScore::ZERO,
        }
    }

    /// 按类型写入。
    pub fn set(&mut self, kind: NeedKind, score: NeedScore) {
        match kind {
            NeedKind::Rest => self.rest = score,
            NeedKind::Hydration => self.hydration = score,
            NeedKind::Movement => self.movement = score,
            NeedKind::EyeRest => self.eye_rest = score,
            NeedKind::Fused => {}
        }
    }

    /// 找出**最紧急**的需求。
    ///
    /// 分数相同时按 [`NeedKind::ALL`] 的固定顺序取第一个，保证决策可复现。
    /// 返回的分数至少是 0，所以调用方永远不需要处理 `None` ——
    /// 「没有任何需求」用分数 0 表达，而不是用「没有值」表达。
    pub fn highest(&self) -> (NeedKind, NeedScore) {
        let mut best_kind = NeedKind::Rest;
        let mut best_score = self.rest;

        for kind in NeedKind::ALL.into_iter().skip(1) {
            let score = self.get(kind);
            if score > best_score {
                best_kind = kind;
                best_score = score;
            }
        }

        (best_kind, best_score)
    }

    /// 是否至少有一类需求达到阈值。
    pub fn any_at_least(&self, threshold: f64) -> bool {
        NeedKind::ALL
            .into_iter()
            .any(|k| self.get(k).at_least(threshold))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 需求分数被夹取在零到一之间() {
        assert_eq!(NeedScore::new(-3.0).get(), 0.0);
        assert_eq!(NeedScore::new(0.42).get(), 0.42);
        assert_eq!(NeedScore::new(9.9).get(), 1.0);
    }

    #[test]
    fn 非法浮点数退化为零而不是最紧急() {
        // 这是有意的保守设计：算坏一个分数，结果应该是「不打扰」，
        // 而不是「立刻全屏弹窗」。健康工具宁可漏报，不可乱报。
        assert_eq!(NeedScore::new(f64::NAN).get(), 0.0);
        assert_eq!(NeedScore::new(f64::INFINITY).get(), 0.0);
        assert_eq!(NeedScore::new(f64::NEG_INFINITY).get(), 0.0);
    }

    #[test]
    fn 百分数换算() {
        assert_eq!(NeedScore::new(0.0).percent(), 0);
        assert_eq!(NeedScore::new(0.756).percent(), 76);
        assert_eq!(NeedScore::new(1.0).percent(), 100);
    }

    #[test]
    fn 取最紧急的需求() {
        let needs = HealthNeeds {
            rest: NeedScore::new(0.3),
            hydration: NeedScore::new(0.91),
            movement: NeedScore::new(0.5),
            eye_rest: NeedScore::new(0.2),
        };

        assert_eq!(needs.highest(), (NeedKind::Hydration, NeedScore::new(0.91)));
    }

    #[test]
    fn 平手时按固定顺序取避免结果漂移() {
        // 三类的分数完全一样，应该稳定地返回 ALL 里排在前的 Rest。
        let needs = HealthNeeds {
            rest: NeedScore::new(0.8),
            hydration: NeedScore::new(0.8),
            movement: NeedScore::new(0.8),
            eye_rest: NeedScore::ZERO,
        };

        assert_eq!(needs.highest().0, NeedKind::Rest);
        // 再多跑几次，结果必须一致（防止有人把实现改成 HashMap 之类的无序容器）
        for _ in 0..10 {
            assert_eq!(needs.highest().0, NeedKind::Rest);
        }
    }

    #[test]
    fn 全零需求找到的还是休息且分数为零() {
        let needs = HealthNeeds::none();
        assert_eq!(needs.highest(), (NeedKind::Rest, NeedScore::ZERO));
        assert!(!needs.any_at_least(0.6));
    }

    #[test]
    fn 读写单类需求() {
        let mut needs = HealthNeeds::none();
        needs.set(NeedKind::EyeRest, NeedScore::new(0.7));
        assert_eq!(needs.get(NeedKind::EyeRest).get(), 0.7);
        assert_eq!(needs.eye_rest.get(), 0.7);

        // 写入融合类型不产生任何效果，也不 panic
        needs.set(NeedKind::Fused, NeedScore::ONE);
        assert_eq!(needs.highest().1.get(), 0.7);
    }

    #[test]
    fn 数据库字符串往返一致() {
        for kind in NeedKind::ALL.into_iter().chain([NeedKind::Fused]) {
            assert_eq!(NeedKind::parse(kind.as_str()), Some(kind));
        }
        assert_eq!(NeedKind::parse("unknown"), None);
    }

    #[test]
    fn 序列化为蛇形字符串() {
        let json = serde_json::to_string(&NeedKind::EyeRest).expect("序列化不应失败");
        assert_eq!(json, "\"eye_rest\"");
    }
}
