//! 四类需求的评分。
//!
//! 计算规则只有一条（[`NeedInputs::needs`] 里实现的）：
//!
//! ```text
//!   需求强度 = clamp(距上次满足的时长 / 提醒间隔, 0.0, 1.0)
//! ```
//!
//! ## 一个容易忽略的细节：上限不是 1.0 而是「间隔的 N 倍」
//!
//! 分数被夹在 0~1，但**「撑了多久」这件事不应该被抹掉**。
//! 距上次休息 50 分钟和 4 小时，分数都是 1.0，可处境完全不同 ——
//! 后者需要的是「立刻停下」，前者只是「该休息了」。
//!
//! 所以除了分数，[`NeedCalculator`] 还会报告一个 [`NeedLevel`]（轻微 / 明显 / 紧迫），
//! 由「撑过间隔多少倍」决定。决策引擎用它来区分「通知」和「全屏」。

use serde::{Deserialize, Serialize};
use tacet_core::model::{HealthNeeds, NeedKind, NeedScore, ReminderSettings};
use tacet_core::time::{Timestamp, MINUTE};

/// 分数封顶后，还能撑到间隔的多少倍才彻底到顶。
///
/// 具体说：撑满 3 倍间隔时，视为「无论如何都必须说话了」。
/// 这个数字的作用是给「超时程度」一个边界，避免出现无穷大的比值。
pub const NEED_CEILING_MULTIPLIER: f64 = 3.0;

/// 封顶对应的分钟数（间隔 × 3）。
///
/// 用它可以把「实际撑了多久」换算成一个不会被时长尺度污染的强度值。
pub const NEED_CEILING_MINUTES: f64 = 180.0;

/// 需求被撑到的程度 —— 比分数多一层「撑过头了没有」的信息。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NeedLevel {
    /// 还没到提醒的时候（分数 < 触发阈值）。
    Low,
    /// 到点了（分数已过阈值）。
    Due,
    /// 已经明显超时（撑过间隔的 2 倍）。
    Overdue,
    /// 严重超时（撑过间隔的 3 倍）——这种时候不该再客气。
    Critical,
}

impl NeedLevel {
    /// 界面显示名。
    pub const fn display_name(self) -> &'static str {
        match self {
            NeedLevel::Low => "还好",
            NeedLevel::Due => "该提醒了",
            NeedLevel::Overdue => "已超时",
            NeedLevel::Critical => "严重超时",
        }
    }

    /// 是否已经到该提醒的程度。
    pub const fn is_due(self) -> bool {
        !matches!(self, NeedLevel::Low)
    }
}

/// 计算需求所需的全部输入。
///
/// 所有「上次是什么时候」都由调用方从存储层读好填进来 ——
/// 这个 crate 不碰数据库，因此可以被单元测试完整构造（架构文档 §4.1 的约定）。
#[derive(Debug, Clone, PartialEq)]
pub struct NeedInputs {
    /// 现在几点。
    pub now: Timestamp,
    /// 这一口气已经连续工作了多少分钟。
    ///
    /// 注意它和「距上次休息多久」是两回事：前者是**连续**时长（中间离开过就重置），
    /// 后者是**绝对**间隔。休息需求优先看前者 —— 用户可能一上午休息了三次，
    /// 但每次都只歇了一分钟就继续干，那他的休息需求依然是高的。
    pub continuous_work_minutes: u32,
    /// 上次**完整完成**休息是什么时候。
    pub last_break_completed_at: Option<Timestamp>,
    /// 上次记录喝水是什么时候。
    pub last_water_logged_at: Option<Timestamp>,
    /// 上次活动打卡是什么时候。
    pub last_activity_logged_at: Option<Timestamp>,
    /// 上次远眺打卡是什么时候。
    pub last_eye_rest_logged_at: Option<Timestamp>,
    /// 用户设定的四类提醒间隔。
    pub settings: ReminderSettings,
}

impl NeedInputs {
    /// 只带了时间信息的构造（连续工作时长留待调用方设置）。
    pub fn new(now: Timestamp, settings: ReminderSettings) -> Self {
        Self {
            now,
            continuous_work_minutes: 0,
            last_break_completed_at: None,
            last_water_logged_at: None,
            last_activity_logged_at: None,
            last_eye_rest_logged_at: None,
            settings,
        }
    }
}

/// 单类需求的完整评估结果。
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct NeedAssessment {
    /// 需求类型。
    pub kind: NeedKind,
    /// 需求强度 0.0~1.0。
    pub score: NeedScore,
    /// 撑到的程度。
    pub level: NeedLevel,
    /// 距上次满足过了多少分钟；从未记录过时为 `None`。
    pub minutes_since: Option<u32>,
    /// 用户设定的间隔（分钟）。
    pub interval_minutes: u32,
}

impl NeedAssessment {
    /// 是否已经到该提醒的程度。
    pub const fn is_due(&self) -> bool {
        self.level.is_due()
    }

    /// 「距上次 X 分钟 / 间隔 Y 分钟」这类可解释文案的原料。
    pub fn describe(&self) -> String {
        match self.minutes_since {
            Some(minutes) => format!(
                "{}：距上次 {} 分钟（间隔 {} 分钟，强度 {}%）",
                self.kind.display_name(),
                minutes,
                self.interval_minutes,
                self.score.percent()
            ),
            None => format!(
                "{}：尚无记录（间隔 {} 分钟）",
                self.kind.display_name(),
                self.interval_minutes
            ),
        }
    }
}

/// 需求计算器。
#[derive(Debug, Clone, Copy, Default)]
pub struct NeedCalculator;

impl NeedCalculator {
    /// 新建（无状态，可以直接用 [`Default`]）。
    pub fn new() -> Self {
        Self
    }

    /// 算出四类需求的分数。
    ///
    /// 关闭了的提醒不参与计算，分数为 0 —— 用户关掉了这类提醒，
    /// 就是明确表示「这件事我不需要你管」。让它继续累积分数再被别的路径读到，
    /// 就会出现「明明关了喝水提醒却还在说我该喝水」的鬼故事。
    pub fn needs(&self, inputs: &NeedInputs) -> HealthNeeds {
        let mut needs = HealthNeeds::none();

        for kind in NeedKind::ALL {
            let assessment = self.assess(kind, inputs);
            needs.set(kind, assessment.score);
        }

        needs
    }

    /// 详细评估某一类需求。
    pub fn assess(&self, kind: NeedKind, inputs: &NeedInputs) -> NeedAssessment {
        let interval_minutes = inputs.settings.interval_minutes(kind).unwrap_or(50);
        let enabled = inputs.settings.is_enabled(kind);

        if !enabled {
            return NeedAssessment {
                kind,
                score: NeedScore::ZERO,
                level: NeedLevel::Low,
                minutes_since: None,
                interval_minutes,
            };
        }

        // 休息与护眼看的是「连续」时长，所以有两路信号可用：
        //
        //   - 连续工作时长（状态机给的，中间离开过就重置）
        //   - 距上次完成休息 / 远眺打卡的绝对时间（存储层给的）
        //
        // 规则是**取最近的那一个**：因为「打卡」是一个明确的正面动作，
        // 用户刚做完远眺，就不该因为「连续工作 200 分钟」而立刻又被要求远眺。
        // 反过来，如果打卡记录是很久以前（甚至从没打过），连续工作时长才说话。
        let minutes_since = match kind {
            NeedKind::Rest => most_recent(
                (inputs.continuous_work_minutes > 0).then_some(inputs.continuous_work_minutes),
                inputs
                    .last_break_completed_at
                    .map(|at| elapsed_minutes(at, inputs.now)),
            ),
            NeedKind::Hydration => inputs
                .last_water_logged_at
                .map(|at| elapsed_minutes(at, inputs.now)),
            NeedKind::Movement => inputs
                .last_activity_logged_at
                .map(|at| elapsed_minutes(at, inputs.now)),
            NeedKind::EyeRest => most_recent(
                (inputs.continuous_work_minutes > 0).then_some(inputs.continuous_work_minutes),
                inputs
                    .last_eye_rest_logged_at
                    .map(|at| elapsed_minutes(at, inputs.now)),
            ),
            NeedKind::Fused => None,
        };

        let Some(minutes_since) = minutes_since else {
            // 从来没有记录过：这是一种「未知」而不是「需求为零」。
            //
            // 我们选择保守处理 —— 不因为缺少记录就报出高需求。
            // 想象一下：用户刚装好软件，什么记录都没有，此时弹一个
            // 「你已经 4 小时没喝水了」的全屏提醒，是最糟糕的第一印象。
            // 所以在拿到第一条真实记录之前，需求保持为 0。
            return NeedAssessment {
                kind,
                score: NeedScore::ZERO,
                level: NeedLevel::Low,
                minutes_since: None,
                interval_minutes,
            };
        };

        let score = NeedScore::new(minutes_since as f64 / interval_minutes.max(1) as f64);
        let level = NeedLevel::from_ratio(minutes_since as f64 / interval_minutes.max(1) as f64);
        let minutes_since = Some(minutes_since);

        NeedAssessment {
            kind,
            score,
            level,
            minutes_since,
            interval_minutes,
        }
    }

    /// 评估全部四类，返回明细（界面上的「四类状态卡」直接用这个渲染）。
    pub fn assess_all(&self, inputs: &NeedInputs) -> Vec<NeedAssessment> {
        NeedKind::ALL
            .into_iter()
            .map(|kind| self.assess(kind, inputs))
            .collect()
    }

    /// 最紧急的那一类需求。
    pub fn most_urgent(&self, inputs: &NeedInputs) -> NeedAssessment {
        let all = self.assess_all(inputs);

        // 用 `max_by` 会取到最后一个平手项，与 core 里「平手取 ALL 顺序靠前」的
        // 约定不一致，所以这里手写循环，保证与 HealthNeeds::highest 的行为一致。
        let mut best = all[0];
        for assessment in all.into_iter().skip(1) {
            if assessment.score > best.score {
                best = assessment;
            }
        }
        best
    }
}

impl NeedLevel {
    /// 由「撑了间隔的多少倍」判定程度。
    fn from_ratio(ratio: f64) -> Self {
        // 触发线 0.75 与 core 的 DEFAULT_TRIGGER_THRESHOLD 保持一致。
        if ratio >= NEED_CEILING_MULTIPLIER {
            NeedLevel::Critical
        } else if ratio >= 2.0 {
            NeedLevel::Overdue
        } else if ratio >= 0.75 {
            NeedLevel::Due
        } else {
            NeedLevel::Low
        }
    }
}

/// 距某个时刻过了多少分钟（负数归零）。
fn elapsed_minutes(since: Timestamp, now: Timestamp) -> u32 {
    (now.millis_since(since) / MINUTE).max(0) as u32
}

/// 两个「距上次满足过了多久」的信号里，取**最近**的那个。
///
/// 两路都缺 → `None`（没有任何依据）；只有一路 → 用那一路。
fn most_recent(a: Option<u32>, b: Option<u32>) -> Option<u32> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (Some(only), None) | (None, Some(only)) => Some(only),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tacet_core::model::NeedKind;

    fn t0() -> Timestamp {
        Timestamp::from_millis(1_700_000_000_000)
    }

    fn minutes_ago(minutes: i64) -> Timestamp {
        t0().saturating_sub_millis(minutes * MINUTE)
    }

    fn inputs() -> NeedInputs {
        NeedInputs::new(t0(), ReminderSettings::default())
    }

    #[test]
    fn 刚喝过水时需求为零() {
        let calc = NeedCalculator::new();
        let mut i = inputs();
        i.last_water_logged_at = Some(t0());

        let assessment = calc.assess(NeedKind::Hydration, &i);
        assert_eq!(assessment.score.get(), 0.0);
        assert_eq!(assessment.level, NeedLevel::Low);
        assert!(!assessment.is_due());
    }

    #[test]
    fn 撑满一个间隔时需求封顶() {
        let calc = NeedCalculator::new();
        let mut i = inputs();
        // 喝水间隔默认 45 分钟
        i.last_water_logged_at = Some(minutes_ago(45));

        let assessment = calc.assess(NeedKind::Hydration, &i);
        assert_eq!(assessment.score.get(), 1.0);
        assert_eq!(assessment.minutes_since, Some(45));
        assert!(assessment.is_due());
    }

    #[test]
    fn 分数与间隔成正比() {
        let calc = NeedCalculator::new();
        let mut i = inputs();
        i.last_water_logged_at = Some(minutes_ago(22)); // 45 的一半

        let assessment = calc.assess(NeedKind::Hydration, &i);
        assert!(
            (assessment.score.get() - 0.488).abs() < 0.01,
            "22/45 ≈ 0.489，实际 {}",
            assessment.score.get()
        );
    }

    #[test]
    fn 超时程度会被识别出来() {
        let calc = NeedCalculator::new();

        // 2 倍间隔
        let mut i = inputs();
        i.last_water_logged_at = Some(minutes_ago(90));
        assert_eq!(
            calc.assess(NeedKind::Hydration, &i).level,
            NeedLevel::Overdue
        );

        // 3 倍间隔以上
        i.last_water_logged_at = Some(minutes_ago(300));
        assert_eq!(
            calc.assess(NeedKind::Hydration, &i).level,
            NeedLevel::Critical
        );
    }

    #[test]
    fn 没有任何记录时不报高需求() {
        // 刚装好应用就弹「你已经 4 小时没喝水了」是最糟的第一印象。
        let calc = NeedCalculator::new();
        let i = inputs();

        for kind in NeedKind::ALL {
            let assessment = calc.assess(kind, &i);
            assert_eq!(
                assessment.score.get(),
                0.0,
                "{kind:?} 在没有历史记录时不应有需求"
            );
            assert_eq!(assessment.minutes_since, None);
        }
    }

    #[test]
    fn 关掉的提醒不参与计算() {
        let calc = NeedCalculator::new();
        let mut settings = ReminderSettings::default();
        settings.hydration.enabled = false;

        let mut i = NeedInputs::new(t0(), settings);
        i.last_water_logged_at = Some(minutes_ago(600));

        let assessment = calc.assess(NeedKind::Hydration, &i);
        assert_eq!(
            assessment.score.get(),
            0.0,
            "关掉的提醒不该累积需求，否则别的路径读到会出鬼故事"
        );
    }

    #[test]
    fn 刚做过远眺打卡需求应当重置() {
        // 用户连续工作很久（150 分钟），但 5 分钟前刚远眺过 ——
        // 这时候再要他远眺，只会显得这个工具不懂事。
        let calc = NeedCalculator::new();
        let mut i = inputs();
        i.continuous_work_minutes = 150;
        i.last_eye_rest_logged_at = Some(minutes_ago(5));

        let assessment = calc.assess(NeedKind::EyeRest, &i);
        assert_eq!(assessment.minutes_since, Some(5), "应当以最近一次打卡为准");
        assert!((assessment.score.get() - 0.125).abs() < 0.01);
        assert!(!assessment.is_due());
    }

    #[test]
    fn 从未远眺过时按连续工作时长评估() {
        let calc = NeedCalculator::new();
        let mut i = inputs();
        i.continuous_work_minutes = 100;

        let assessment = calc.assess(NeedKind::EyeRest, &i);
        assert_eq!(assessment.minutes_since, Some(100));
        assert_eq!(assessment.score.get(), 1.0);
        assert!(assessment.is_due());
    }

    #[test]
    fn 休息需求优先看连续工作时长() {
        let calc = NeedCalculator::new();
        let mut i = inputs();
        // 用户一上午休息过（1 小时前完成过休息），但这一口气已经干了 55 分钟
        i.last_break_completed_at = Some(minutes_ago(60));
        i.continuous_work_minutes = 55;

        let assessment = calc.assess(NeedKind::Rest, &i);
        assert_eq!(assessment.minutes_since, Some(55), "应当采用连续工作时长");
        assert!(
            assessment.score.get() > 1.0 - f64::EPSILON,
            "55 分钟 ≥ 50 分钟间隔，分数应当封顶"
        );
    }

    #[test]
    fn 没有连续工作时用距上次休息的绝对时间() {
        let calc = NeedCalculator::new();
        let mut i = inputs();
        i.last_break_completed_at = Some(minutes_ago(25));
        i.continuous_work_minutes = 0;

        let assessment = calc.assess(NeedKind::Rest, &i);
        assert_eq!(assessment.minutes_since, Some(25));
        assert!((assessment.score.get() - 0.5).abs() < 0.01);
    }

    #[test]
    fn 四类需求一起算() {
        let calc = NeedCalculator::new();
        let mut i = inputs();
        i.last_water_logged_at = Some(minutes_ago(90)); // 间隔 45 → 封顶
        i.last_activity_logged_at = Some(minutes_ago(30)); // 间隔 60 → 0.5
        i.last_eye_rest_logged_at = Some(minutes_ago(10)); // 间隔 40 → 0.25
        i.continuous_work_minutes = 20; // 间隔 50 → 0.4

        let needs = calc.needs(&i);

        assert_eq!(needs.hydration.get(), 1.0);
        assert!((needs.movement.get() - 0.5).abs() < 0.01);
        assert!((needs.eye_rest.get() - 0.25).abs() < 0.01);
        assert!((needs.rest.get() - 0.4).abs() < 0.01);
    }

    #[test]
    fn 找最紧急的那一类() {
        let calc = NeedCalculator::new();
        let mut i = inputs();
        i.last_water_logged_at = Some(minutes_ago(20)); // 0.44
        i.last_activity_logged_at = Some(minutes_ago(58)); // 0.97 → 最急
        i.last_eye_rest_logged_at = Some(minutes_ago(5)); // 0.125
        i.continuous_work_minutes = 10; // 0.2

        let urgent = calc.most_urgent(&i);
        assert_eq!(urgent.kind, NeedKind::Movement);
    }

    #[test]
    fn 平手时取顺序靠前的那一类() {
        // 与 core 的 HealthNeeds::highest 行为保持一致，避免两处实现漂移。
        let calc = NeedCalculator::new();
        let mut i = inputs();
        i.last_water_logged_at = Some(minutes_ago(45)); // 1.0
        i.last_activity_logged_at = Some(minutes_ago(60)); // 1.0

        assert_eq!(calc.most_urgent(&i).kind, NeedKind::Hydration);
    }

    #[test]
    fn 用户改间隔会立刻改变分数() {
        let calc = NeedCalculator::new();
        let mut i = inputs();
        i.last_water_logged_at = Some(minutes_ago(45));
        assert_eq!(calc.assess(NeedKind::Hydration, &i).score.get(), 1.0);

        // 用户把喝水间隔从 45 分钟调到 90 分钟
        i.settings.hydration.interval_minutes = 90;
        let assessment = calc.assess(NeedKind::Hydration, &i);
        assert!(
            (assessment.score.get() - 0.5).abs() < 0.01,
            "间隔调大后，同样的时长只该有一半强度，实际 {}",
            assessment.score.get()
        );
        assert!(!assessment.is_due(), "间隔 90 分钟时，45 分钟还没到点");
    }

    #[test]
    fn 关闭全部提醒后所有需求为零() {
        let calc = NeedCalculator::new();
        let mut settings = ReminderSettings::default();
        settings.rest.enabled = false;
        settings.hydration.enabled = false;
        settings.movement.enabled = false;
        settings.eye_rest.enabled = false;

        let mut i = NeedInputs::new(t0(), settings);
        i.last_water_logged_at = Some(minutes_ago(600));
        i.continuous_work_minutes = 300;

        let needs = calc.needs(&i);
        assert_eq!(needs, HealthNeeds::none());
    }

    #[test]
    fn 时间倒挂不会产生负需求() {
        let calc = NeedCalculator::new();
        let mut i = inputs();
        // 记录时间在未来（时钟被校准过 / 数据脏了）
        i.last_water_logged_at = Some(t0().saturating_add_millis(60 * MINUTE));

        let assessment = calc.assess(NeedKind::Hydration, &i);
        assert_eq!(assessment.score.get(), 0.0);
        assert_eq!(assessment.minutes_since, Some(0));
    }

    #[test]
    fn 可解释文案包含关键数字() {
        let calc = NeedCalculator::new();
        let mut i = inputs();
        i.last_water_logged_at = Some(minutes_ago(93));

        let text = calc.assess(NeedKind::Hydration, &i).describe();
        assert!(text.contains("93"), "文案应当给出实际间隔：{text}");
        assert!(text.contains("45"), "文案应当给出用户设定的间隔：{text}");
        assert!(text.contains("喝水"), "文案应当说明是哪类需求：{text}");
    }

    #[test]
    fn 无记录时的文案说明情况而不是编造数字() {
        let calc = NeedCalculator::new();
        let text = calc.assess(NeedKind::Movement, &inputs()).describe();
        assert!(text.contains("尚无记录"), "实际文案：{text}");
    }

    #[test]
    fn 评估明细覆盖四类需求() {
        let calc = NeedCalculator::new();
        let all = calc.assess_all(&inputs());

        assert_eq!(all.len(), 4);
        let kinds: Vec<NeedKind> = all.iter().map(|a| a.kind).collect();
        assert_eq!(kinds, NeedKind::ALL.to_vec());
    }

    #[test]
    fn 分数永远不会超过一也不会低于零() {
        let calc = NeedCalculator::new();
        let mut i = inputs();
        i.last_water_logged_at = Some(minutes_ago(100_000));

        let score = calc.assess(NeedKind::Hydration, &i).score;
        assert_eq!(score.get(), 1.0);
        assert!(score.get() >= 0.0);
    }
}
