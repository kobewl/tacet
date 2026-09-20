//! Policy Engine —— 回答 Tacet 最核心的那个问题：
//!
//! > **现在该不该说话？如果该说，用多大声？**
//!
//! ## 输入与输出
//!
//! ```text
//!   ① 我现在在干什么        ContextSnapshot    （谁在前台、全屏吗、人还在吗）
//!   ② 我有多需要休息        HealthNeeds        （四类需求各多急）
//!   ③ 我希望被怎么对待      UserPreferences    （开关、间隔、勿扰）
//!   ④ 刚才发生过什么        RecentHistory      （上次提醒是什么时候）
//!             │
//!             ▼
//!      InterventionDecision { 类型, 等级, 理由[], 动作[] }
//! ```
//!
//! ## v0.1 是「简单规则」，不是「简陋规则」
//!
//! 完整形态的决策引擎在 v0.2（需求评分 × 可打扰度五因子）。v0.1 只有阈值判断，
//! 但它已经具备了完整形态的骨架：
//!
//! - **决策完全由输入决定**：同样的输入永远得到同样的输出，可穷举测试
//! - **每条决策都带理由**：界面直接渲染成「为什么现在提醒我」
//! - **有降级、有限流、有出口**：这三件事从第一天就在，不是后补的
//!
//! ## 一条不可动摇的规则
//!
//! 这个模块**不做任何网络调用**，也不引入任何 LLM（ADR-005、架构红线 5）。
//! 决策必须是离线的、确定性的、可解释的 —— 断网、没配 AI、模型抽风，
//! 都不能影响「该不该提醒你休息」这个判断。

use serde::{Deserialize, Serialize};

use crate::model::{
    ContextSnapshot, HealthNeeds, Intervention, InterventionLevel, NeedKind, NeedScore, Reason,
    UserPreferences,
};
use crate::time::{Timestamp, MINUTE};

/// 需求分数达到多少就该开口。
///
/// 为什么是 0.75 而不是 1.0：需求分数是「紧迫程度」而不是「倒计时归零」。
/// 等它涨到 1.0 意味着已经严重超时，那时候再提醒就太晚了。
/// 0.75 大致对应「刚过提醒间隔」的位置，是产品和医学上都比较舒服的时机。
pub const DEFAULT_TRIGGER_THRESHOLD: f64 = 0.75;

/// 同类提醒之间的**最小**冷却时间（PRD §3.3 通用约束 4）。
///
/// ## 为什么是「最小」而不是固定值
///
/// 早期版本把冷却写死成 10 分钟。看起来合理，但和用户设置打架：
/// 用户把休息间隔调到 60 分钟，表达的是「我希望大约每小时提醒一次」；
/// 而 10 分钟的冷却意味着只要需求分数还在阈值之上，
/// **每隔 10 分钟就会再提醒一次** —— 用户会觉得「我设的 60 分钟根本没用」。
///
/// 所以冷却时间现在按用户间隔算（见 [`cooldown_for`]），
/// 这个常量只作为**下限**：无论用户怎么设，都不允许短时间内反复轰炸。
pub const MIN_COOLDOWN_MS: i64 = 10 * MINUTE;

/// 冷却时间占用户设定间隔的比例。
///
/// 为什么是 60%：用户在 60 分钟这个刻度上，通常想要的是
/// 「大概每小时被提一次」。如果把冷却设成等于间隔，那么一次提醒之后
/// 必须整整等满一小时才可能再次提醒 —— 而需求分数是连续上升的，
/// 这会导致提醒实际间隔被拉长到远超设定值。
///
/// 取 60% 的效果：设 60 分钟 → 冷却 36 分钟，实际的提醒节奏
/// 落在「比设定略紧一点」的位置，符合直觉。
const COOLDOWN_RATIO: f64 = 0.6;

/// 按用户设定的提醒间隔算出冷却时长。
///
/// 见 [`MIN_COOLDOWN_MS`] 里对「为什么不是固定值」的说明。
pub fn cooldown_for(interval_minutes: u32) -> i64 {
    let scaled = (interval_minutes as f64 * COOLDOWN_RATIO) as i64 * MINUTE;
    // 下限兜底：用户设 5 分钟间隔时，60% 只有 3 分钟 —— 太密了。
    scaled.max(MIN_COOLDOWN_MS)
}

/// 同一类提醒在这个时间内不重复发出（保留给不关心用户设置的调用方）。
///
/// 新代码请用 [`cooldown_for`]，它才是尊重用户设置的版本。
pub const SAME_KIND_COOLDOWN_MS: i64 = MIN_COOLDOWN_MS;

/// 最近发生过什么 —— 决策的「记忆」部分。
///
/// 全部由调用方从存储层读出来填好，决策引擎自己不碰数据库。
/// 这样做的好处是决策逻辑可以在测试里被完全构造（架构文档 §4.1 的工程约定）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RecentHistory {
    /// 最近一次真正打扰到用户的干预。
    pub last_intervention: Option<InterventionRecap>,
    /// 最近一次完成的休息。
    pub last_break_completed_at: Option<Timestamp>,
    /// 最近一次记录喝水。
    pub last_water_logged_at: Option<Timestamp>,
    /// 最近一次活动打卡。
    pub last_activity_logged_at: Option<Timestamp>,
    /// 最近一次远眺打卡。
    pub last_eye_rest_logged_at: Option<Timestamp>,
    /// 最近一次用户主动延后的时间点（延后期间不重复提醒）。
    pub snoozed_until: Option<Timestamp>,
}

/// 最近一次干预的摘要。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct InterventionRecap {
    /// 为了哪类需求。
    pub kind: NeedKind,
    /// 用什么等级发的。
    pub level: InterventionLevel,
    /// 什么时候发的。
    pub fired_at: Timestamp,
}

impl RecentHistory {
    /// 距上次同类提醒过了多久（分钟）；没有记录时为 `None`。
    pub fn minutes_since_last_of(&self, kind: NeedKind, now: Timestamp) -> Option<u32> {
        let last = self.last_intervention?;
        if last.kind != kind {
            return None;
        }
        Some((now.millis_since(last.fired_at) / MINUTE).max(0) as u32)
    }

    /// 上次提醒是不是还在冷却期内。
    ///
    /// 冷却时长由调用方传入（见 [`cooldown_for`]）——
    /// 它必须跟着用户的提醒间隔走，不能是固定值。
    fn is_in_cooldown(&self, kind: NeedKind, now: Timestamp, cooldown_ms: i64) -> Option<u32> {
        let last = self.last_intervention?;
        if last.kind != kind {
            return None;
        }

        let elapsed_ms = now.millis_since(last.fired_at).max(0);
        if elapsed_ms < cooldown_ms {
            Some((elapsed_ms / MINUTE) as u32)
        } else {
            None
        }
    }
}

/// 决策的输入（架构文档 §4.1 的既约版）。
#[derive(Debug, Clone, PartialEq)]
pub struct DecisionInput {
    /// 现在几点。
    pub now: Timestamp,
    /// 用户在干什么。
    pub context: ContextSnapshot,
    /// 四类需求各多急。
    pub needs: HealthNeeds,
    /// 用户偏好。
    pub preferences: UserPreferences,
    /// 最近发生过什么。
    pub recent: RecentHistory,
    /// 是否正在休息中（休息时不重复提醒）。
    pub is_breaking: bool,
    /// 这一次已经连续工作了多少分钟。
    pub continuous_work_minutes: u32,
}

/// 决策的输出。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InterventionDecision {
    /// 为了哪类需求。
    pub kind: NeedKind,
    /// 用哪一级方式。
    pub level: InterventionLevel,
    /// 为什么（可解释因子，直接给界面渲染）。
    pub reasons: Vec<Reason>,
    /// 建议用户做的具体动作（如「起来活动 2~3 分钟」「远眺 20 秒」）。
    pub actions: Vec<String>,
    /// 是否被融合了多个需求（v0.2 使用；v0.1 恒为空）。
    pub fused: Vec<NeedKind>,
}

impl InterventionDecision {
    /// 什么都不做的决策。
    pub fn silent(kind: NeedKind, reasons: Vec<Reason>) -> Self {
        Self {
            kind,
            level: InterventionLevel::Silent,
            reasons,
            actions: Vec::new(),
            fused: Vec::new(),
        }
    }

    /// 这次决策是否会真的打扰到用户。
    pub const fn will_disturb(&self) -> bool {
        self.level.disturbs_user()
    }

    /// 把决策转成一条待落库的干预记录。
    pub fn to_intervention(&self, now: Timestamp) -> Intervention {
        Intervention::fired(self.kind, self.level, self.reasons.clone(), now)
    }

    /// 渲染「为什么现在提醒我」的多行文案。
    pub fn why_lines(&self) -> Vec<String> {
        Reason::render_lines(&self.reasons)
    }
}

/// 决策引擎。
///
/// 它是一个**无状态**的纯函数持有者：所有需要记住的东西都由调用方通过
/// [`DecisionInput`] 传进来。这样设计的好处有三点：
///
/// 1. 同样的输入永远得到同样的输出（可测试、可回放）
/// 2. 引擎可以被任意拷贝到多个线程里用，不存在共享可变状态
/// 3. 状态该存在哪儿是一个存储问题，不是决策问题 —— 两者不该混在一起
#[derive(Debug, Clone, Copy)]
pub struct PolicyEngine {
    /// 触发提醒的需求阈值。
    trigger_threshold: f64,
    /// 需求极高时，即使场景不适合也至少给一个环境提示的阈值。
    urgent_threshold: f64,
}

impl Default for PolicyEngine {
    fn default() -> Self {
        Self {
            trigger_threshold: DEFAULT_TRIGGER_THRESHOLD,
            urgent_threshold: 0.95,
        }
    }
}

impl PolicyEngine {
    /// 用默认阈值构造。
    pub fn new() -> Self {
        Self::default()
    }

    /// 自定义触发阈值（v0.2 起会由用户模型动态调整）。
    pub fn with_threshold(trigger_threshold: f64, urgent_threshold: f64) -> Self {
        Self {
            trigger_threshold: trigger_threshold.clamp(0.0, 1.0),
            urgent_threshold: urgent_threshold.clamp(0.0, 1.0),
        }
    }

    /// 做一次决策。
    ///
    /// 判断顺序很关键，而且**顺序本身就有产品含义**：
    ///
    /// 1. 正在休息 → 闭嘴（已经在休息了，不需要再提醒）
    /// 2. 勿扰模式 → 闭嘴（用户明确说了别烦我，这个权利高于任何健康建议）
    /// 3. 整体需求都低 → 闭嘴
    /// 4. 同类刚提醒过 → 闭嘴（限流）
    /// 5. 用户刚延后过 → 闭嘴（延后是承诺，不能言而无信）
    /// 6. 需求高 + 场景适合 → 开口，并决定用哪一级
    pub fn decide(&self, input: &DecisionInput) -> InterventionDecision {
        let (kind, score) = input.needs.highest();

        // ① 休息中：不重复提醒。
        if input.is_breaking {
            return InterventionDecision::silent(kind, vec![Reason::ContextUnavailable]);
        }

        // ② 勿扰模式：优先级最高的一道闸门。
        //    理由写进决策里，用户以后能查到「那段时间它为什么一声不吭」。
        if input.preferences.do_not_disturb {
            return InterventionDecision::silent(kind, vec![Reason::DoNotDisturb]);
        }

        // ③ 需求没到阈值。
        if !score.at_least(self.trigger_threshold) {
            return InterventionDecision::silent(
                kind,
                vec![Reason::NeedBelowThreshold {
                    kind,
                    percent: score.percent(),
                }],
            );
        }

        // ④ 同类提醒的冷却期。
        //
        // 冷却时长跟着用户设的间隔走（见 `cooldown_for`）。
        // 用户把间隔调大，冷却也跟着变长 —— 否则「设了 60 分钟却
        // 每 10 分钟被提醒一次」这种事就会发生。
        let cooldown = cooldown_for(
            input
                .preferences
                .reminders
                .interval_minutes(kind)
                .unwrap_or(50),
        );
        if let Some(minutes_ago) = input.recent.is_in_cooldown(kind, input.now, cooldown) {
            return InterventionDecision::silent(
                kind,
                vec![Reason::RateLimited { kind, minutes_ago }],
            );
        }

        // ⑤ 用户刚说过「稍后」。
        if let Some(until) = input.recent.snoozed_until {
            if input.now < until {
                let minutes = ((until.millis_since(input.now)) / MINUTE).max(0) as u32;
                return InterventionDecision::silent(
                    kind,
                    vec![Reason::RateLimited {
                        kind,
                        minutes_ago: minutes,
                    }],
                );
            }
        }

        // ⑥ 开口。先把「为什么」收集齐，再决定用多大声。
        let mut reasons = self.build_reasons(kind, score, input);

        let (level, level_reasons) = self.choose_level(kind, score, input);
        // 降级的理由也要写进决策：用户看到「因为你在全屏，所以只发了通知」，
        // 才会相信这套系统是真的在替他考虑，而不是随机选了个弱一点的方式。
        reasons.extend(level_reasons);

        let actions = suggested_actions(kind);

        InterventionDecision {
            kind,
            level,
            reasons,
            actions,
            fused: Vec::new(),
        }
    }

    /// 组装「为什么现在提醒我」。
    fn build_reasons(
        &self,
        kind: NeedKind,
        score: NeedScore,
        input: &DecisionInput,
    ) -> Vec<Reason> {
        let mut reasons = Vec::new();

        // 只在这条理由确实成立时才写进去。空话会稀释可信度：
        // 如果每次都说「已连续工作 0 分钟」，用户很快就不再读这些文字了。
        match kind {
            NeedKind::Rest => {
                if input.continuous_work_minutes > 0 {
                    reasons.push(Reason::ContinuousWork {
                        minutes: input.continuous_work_minutes,
                    });
                }
                if let Some(at) = input.recent.last_break_completed_at {
                    let minutes = (input.now.millis_since(at) / MINUTE).max(0) as u32;
                    reasons.push(Reason::SinceLastBreak { minutes });
                }
            }
            NeedKind::Hydration => {
                if let Some(at) = input.recent.last_water_logged_at {
                    let minutes = (input.now.millis_since(at) / MINUTE).max(0) as u32;
                    reasons.push(Reason::SinceLastHydration { minutes });
                }
            }
            NeedKind::Movement => {
                if let Some(at) = input.recent.last_activity_logged_at {
                    let minutes = (input.now.millis_since(at) / MINUTE).max(0) as u32;
                    reasons.push(Reason::SinceLastMovement { minutes });
                }
            }
            NeedKind::EyeRest => {
                if input.continuous_work_minutes > 0 {
                    reasons.push(Reason::ScreenTime {
                        minutes: input.continuous_work_minutes,
                    });
                }
            }
            NeedKind::Fused => {}
        }

        // 阈值的具体数值也告诉用户 —— 「需求 78%，超过了 75% 的提醒线」，
        // 比一句「该休息了」诚实得多。
        reasons.push(Reason::NeedBelowThreshold {
            kind,
            percent: score.percent(),
        });

        reasons
    }

    /// 决定用哪一级干预，以及这么决定的理由。
    ///
    /// 这是「健康需求 × 可打扰度」第一次真正落地的地方。
    /// v0.1 只有两个可打扰度因子（全屏、专注型应用），但决策的**形状**
    /// 已经是完整的：需求越高越想用强手段，场景越敏感越要压低。
    ///
    /// 返回值里的第二个元素是**降级理由**：它是给用户看的解释，
    /// 说明「为什么这次只用了通知而不是全屏」。返回 `None` 级别的场景
    /// （即没有发生降级）返回空数组。
    fn choose_level(
        &self,
        kind: NeedKind,
        score: NeedScore,
        input: &DecisionInput,
    ) -> (InterventionLevel, Vec<Reason>) {
        let gentle_scene = input.context.prefers_gentle_intervention();
        let urgent = score.at_least(self.urgent_threshold);

        // 场景不适合强打断时，最高只能到系统通知。
        if gentle_scene {
            let mut reasons = Vec::new();

            if input.context.fullscreen {
                reasons.push(Reason::AppFullscreen {
                    app: input.context.app_name().to_string(),
                });
            } else if let Some(app) = input.context.foreground_app.as_ref() {
                if app.category.implies_focus() {
                    reasons.push(Reason::AppFullscreen {
                        app: app.name.clone(),
                    });
                }
            }

            // 需求已经很高了（或者命中了明确的场景因子）：不能什么都不做，
            // 但也不该全屏弹窗 —— 用户正在全屏看演示 / 开会 / 写代码，
            // 一个全屏提醒会让他很难堪。
            let level = if urgent || !reasons.is_empty() {
                InterventionLevel::Notification
            } else {
                InterventionLevel::Ambient
            };

            return (level, reasons);
        }

        // 休息类提醒用全屏（这是产品的主场景，v0.1 的核心闭环）。
        // 其它三类用系统通知：喝水、起身、远眺都是「顺手就能做」的小事，
        // 为它们全屏覆盖用户的屏幕，打断成本远大于健康收益。
        let level = match kind {
            NeedKind::Rest => {
                if urgent {
                    InterventionLevel::FullScreen
                } else {
                    InterventionLevel::Notification
                }
            }
            NeedKind::Hydration | NeedKind::Movement | NeedKind::EyeRest => {
                InterventionLevel::Notification
            }
            NeedKind::Fused => InterventionLevel::Notification,
        };

        (level, Vec::new())
    }
}

/// 给用户的具体建议动作。
///
/// 产品原则 1 说「不机械」—— 一句「该休息了」是命令，
/// 一句「起身走两步，看看窗外」才是建议。差别就在这几个字里。
pub fn suggested_actions(kind: NeedKind) -> Vec<String> {
    match kind {
        NeedKind::Rest => vec![
            "离开屏幕，让眼睛看看远处".to_string(),
            "接杯水，顺便活动一下肩颈".to_string(),
        ],
        NeedKind::Hydration => vec!["喝几口水".to_string()],
        NeedKind::Movement => vec![
            "站起来走 2~3 分钟".to_string(),
            "伸个懒腰，转转脖子".to_string(),
        ],
        NeedKind::EyeRest => vec![
            "看向 6 米以外的地方 20 秒".to_string(),
            "闭眼休息一下".to_string(),
        ],
        NeedKind::Fused => vec!["起身活动一下，顺便喝口水".to_string()],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{AppCategory, ForegroundApp};
    use crate::time::MINUTE;

    fn t0() -> Timestamp {
        Timestamp::from_millis(1_700_000_000_000)
    }

    /// 造一个「用户正在工作、需求适中、没有特殊场景」的输入。
    fn base_input(needs: HealthNeeds) -> DecisionInput {
        DecisionInput {
            now: t0(),
            context: ContextSnapshot::unavailable(t0()),
            needs,
            preferences: UserPreferences::default(),
            recent: RecentHistory::default(),
            is_breaking: false,
            continuous_work_minutes: 60,
        }
    }

    fn needs_with(kind: NeedKind, score: f64) -> HealthNeeds {
        let mut needs = HealthNeeds::none();
        needs.set(kind, NeedScore::new(score));
        needs
    }

    #[test]
    fn 需求低时不打扰() {
        let engine = PolicyEngine::new();
        let input = base_input(needs_with(NeedKind::Rest, 0.30));

        let decision = engine.decide(&input);

        assert_eq!(decision.level, InterventionLevel::Silent);
        assert!(!decision.will_disturb());
        // 即使不说话，也要说清为什么不说 —— 用户查看日志时能看到
        assert!(decision
            .reasons
            .iter()
            .any(|r| matches!(r, Reason::NeedBelowThreshold { .. })));
    }

    #[test]
    fn 需求越过阈值且场景合适时全屏提醒休息() {
        let engine = PolicyEngine::new();
        let input = base_input(needs_with(NeedKind::Rest, 0.95));

        let decision = engine.decide(&input);

        assert_eq!(decision.kind, NeedKind::Rest);
        assert_eq!(decision.level, InterventionLevel::FullScreen);
        assert!(decision.will_disturb());
        assert!(!decision.actions.is_empty(), "提醒必须给出具体可做的事");
    }

    #[test]
    fn 全屏场景下绝不弹全屏提醒() {
        // 用户可能正在放演示、看视频、开线上会议。全屏覆盖是灾难。
        let engine = PolicyEngine::new();
        let mut input = base_input(needs_with(NeedKind::Rest, 1.0));
        input.context.fullscreen = true;
        input.context.foreground_app = Some(ForegroundApp::new(
            "com.apple.Safari",
            "Safari",
            AppCategory::Browser,
        ));

        let decision = engine.decide(&input);

        assert_eq!(decision.level, InterventionLevel::Notification);
        assert!(
            decision
                .reasons
                .iter()
                .any(|r| matches!(r, Reason::AppFullscreen { .. })),
            "降级的理由必须写清楚，用户才知道系统考虑过他的处境"
        );
    }

    #[test]
    fn 编辑器里工作时降级为通知() {
        // 写代码时突然被全屏盖住，是这类产品最招人恨的行为。
        let engine = PolicyEngine::new();
        let mut input = base_input(needs_with(NeedKind::Rest, 0.99));
        input.context.foreground_app = Some(ForegroundApp::new(
            "com.microsoft.VSCode",
            "Visual Studio Code",
            AppCategory::Editor,
        ));

        let decision = engine.decide(&input);

        assert_eq!(decision.level, InterventionLevel::Notification);
    }

    #[test]
    fn 勿扰模式优先于一切健康建议() {
        let engine = PolicyEngine::new();
        let mut input = base_input(needs_with(NeedKind::Rest, 1.0));
        input.preferences.do_not_disturb = true;

        let decision = engine.decide(&input);

        assert_eq!(decision.level, InterventionLevel::Silent);
        assert_eq!(decision.reasons, vec![Reason::DoNotDisturb]);
    }

    #[test]
    fn 休息中不重复提醒() {
        let engine = PolicyEngine::new();
        let mut input = base_input(needs_with(NeedKind::Rest, 1.0));
        input.is_breaking = true;

        assert_eq!(engine.decide(&input).level, InterventionLevel::Silent);
    }

    #[test]
    fn 冷却期内同类提醒不重复发出() {
        let engine = PolicyEngine::new();
        let mut input = base_input(needs_with(NeedKind::Hydration, 0.9));
        input.recent.last_intervention = Some(InterventionRecap {
            kind: NeedKind::Hydration,
            level: InterventionLevel::Notification,
            fired_at: t0().saturating_sub_millis(4 * MINUTE),
        });

        let decision = engine.decide(&input);

        assert_eq!(decision.level, InterventionLevel::Silent);
        match &decision.reasons[0] {
            Reason::RateLimited { kind, minutes_ago } => {
                assert_eq!(*kind, NeedKind::Hydration);
                assert_eq!(*minutes_ago, 4);
            }
            other => panic!("期望限流理由，实际 {other:?}"),
        }
    }

    #[test]
    fn 冷却期过去后可以再次提醒() {
        let engine = PolicyEngine::new();
        let mut input = base_input(needs_with(NeedKind::Hydration, 0.9));
        // 默认间隔 45 分钟 → 冷却 27 分钟。放到 30 分钟之前，应当已过冷却。
        input.recent.last_intervention = Some(InterventionRecap {
            kind: NeedKind::Hydration,
            level: InterventionLevel::Notification,
            fired_at: t0().saturating_sub_millis(30 * MINUTE),
        });

        assert_eq!(
            engine.decide(&input).level,
            InterventionLevel::Notification,
            "超过冷却期后应当可以再次提醒"
        );
    }

    /// 这条测试盯住一个真实的设计缺陷。
    ///
    /// ## 问题
    ///
    /// 早期版本把冷却写死成 10 分钟。用户把喝水间隔从 45 分钟调到 120 分钟，
    /// 表达的是「别那么频繁地提醒我」；但因为冷却还是 10 分钟，
    /// 只要需求分数保持在阈值之上，**每 10 分钟就会再提醒一次**。
    /// 用户会觉得设置根本没生效。
    ///
    /// ## 现在的行为
    ///
    /// 冷却时长 = 用户间隔 × 0.6（下限 10 分钟）。
    /// 所以间隔调大之后，同样的「上次提醒在 15 分钟前」，
    /// 在默认间隔下会放行，在 120 分钟间隔下会被拦住。
    #[test]
    fn 间隔调大后冷却期跟着变长() {
        let engine = PolicyEngine::new();

        let make_input = |interval_minutes: u32| {
            let mut input = base_input(needs_with(NeedKind::Hydration, 0.9));
            input.preferences.reminders.hydration.interval_minutes = interval_minutes;
            // 上次喝水提醒是 20 分钟前
            input.recent.last_intervention = Some(InterventionRecap {
                kind: NeedKind::Hydration,
                level: InterventionLevel::Notification,
                fired_at: t0().saturating_sub_millis(20 * MINUTE),
            });
            input
        };

        // 间隔 20 分钟 → 冷却 12 分钟 → 20 分钟前那次已经过了冷却，可以提醒
        assert_eq!(
            engine.decide(&make_input(20)).level,
            InterventionLevel::Notification,
            "间隔设得短，20 分钟后应当可以再次提醒"
        );

        // 间隔 120 分钟 → 冷却 72 分钟 → 20 分钟前那次还在冷却里，保持安静
        assert_eq!(
            engine.decide(&make_input(120)).level,
            InterventionLevel::Silent,
            "间隔设得长，就不该在 20 分钟后又来打扰 —— 否则设置形同虚设"
        );
    }

    /// 无论用户怎么设，都不允许把提醒频率调到「轰炸」级别。
    #[test]
    fn 冷却时间有下限兜底() {
        // 间隔设成最小值 5 分钟：5 × 0.6 = 3 分钟，但下限是 10 分钟
        assert_eq!(cooldown_for(5), MIN_COOLDOWN_MS);

        // 间隔 60 分钟：60 × 0.6 = 36 分钟
        assert_eq!(cooldown_for(60), 36 * MINUTE);

        // 间隔 240 分钟（上限）：240 × 0.6 = 144 分钟
        assert_eq!(cooldown_for(240), 144 * MINUTE);
    }

    #[test]
    fn 冷却期只针对同一类需求() {
        // 上次提醒的是喝水，这次要提醒活动，不该被拦。
        let engine = PolicyEngine::new();
        let mut input = base_input(needs_with(NeedKind::Movement, 0.9));
        input.recent.last_intervention = Some(InterventionRecap {
            kind: NeedKind::Hydration,
            level: InterventionLevel::Notification,
            fired_at: t0().saturating_sub_millis(MINUTE),
        });

        assert_eq!(engine.decide(&input).level, InterventionLevel::Notification);
    }

    #[test]
    fn 用户延后期间保持安静() {
        let engine = PolicyEngine::new();
        // 需求 0.80：已过提醒线（0.75）但还没到紧急线（0.95）
        let mut input = base_input(needs_with(NeedKind::Rest, 0.80));
        input.recent.snoozed_until = Some(t0().saturating_add_millis(3 * MINUTE));

        let decision = engine.decide(&input);
        assert_eq!(
            decision.level,
            InterventionLevel::Silent,
            "延后是承诺，不能言而无信"
        );

        // 延后时间到了，就可以再提醒
        input.now = t0().saturating_add_millis(4 * MINUTE);
        assert_eq!(
            engine.decide(&input).level,
            InterventionLevel::Notification,
            "需求还没到紧急线时，延后结束后先用通知"
        );
    }

    #[test]
    fn 延后结束的时刻本身不再安静() {
        let engine = PolicyEngine::new();
        let mut input = base_input(needs_with(NeedKind::Rest, 1.0));
        input.recent.snoozed_until = Some(t0());

        assert_eq!(engine.decide(&input).level, InterventionLevel::FullScreen);
    }

    #[test]
    fn 非休息类需求用通知而不是全屏() {
        // 喝水、起身、远眺都是顺手能做的小事，全屏盖住屏幕是不成比例的。
        let engine = PolicyEngine::new();

        for kind in [NeedKind::Hydration, NeedKind::Movement, NeedKind::EyeRest] {
            let input = base_input(needs_with(kind, 1.0));
            let decision = engine.decide(&input);

            assert_eq!(
                decision.level,
                InterventionLevel::Notification,
                "{kind:?} 应当用通知而非全屏"
            );
            assert_eq!(decision.kind, kind);
        }
    }

    #[test]
    fn 多条需求同时超过阈值时取最急的那个() {
        let engine = PolicyEngine::new();
        let needs = HealthNeeds {
            rest: NeedScore::new(0.80),
            hydration: NeedScore::new(0.99),
            movement: NeedScore::new(0.85),
            eye_rest: NeedScore::new(0.50),
        };

        let decision = engine.decide(&base_input(needs));
        assert_eq!(decision.kind, NeedKind::Hydration);
    }

    #[test]
    fn 理由里带上真实的时间线信息() {
        let engine = PolicyEngine::new();
        let mut input = base_input(needs_with(NeedKind::Hydration, 0.9));
        input.recent.last_water_logged_at = Some(t0().saturating_sub_millis(93 * MINUTE));

        let decision = engine.decide(&input);
        let lines = decision.why_lines();

        assert!(
            lines.iter().any(|l| l.contains("93 分钟")),
            "应当说明距上次喝水 93 分钟，实际文案：{lines:?}"
        );
        assert!(
            lines.iter().any(|l| l.contains("90%")),
            "应当说明需求强度，实际文案：{lines:?}"
        );
    }

    #[test]
    fn 休息类提醒的理由包含连续工作时长() {
        let engine = PolicyEngine::new();
        let mut input = base_input(needs_with(NeedKind::Rest, 0.9));
        input.continuous_work_minutes = 78;

        let decision = engine.decide(&input);

        assert_eq!(decision.reasons[0], Reason::ContinuousWork { minutes: 78 });
    }

    #[test]
    fn 决策可以转成待落库的干预记录() {
        let engine = PolicyEngine::new();
        let input = base_input(needs_with(NeedKind::Rest, 0.95));

        let decision = engine.decide(&input);
        let record = decision.to_intervention(input.now);

        assert_eq!(record.kind, NeedKind::Rest);
        assert_eq!(record.level, InterventionLevel::FullScreen);
        assert_eq!(record.fired_at, input.now);
        assert!(record.outcome.is_none(), "刚发出时还没有用户回应");
        assert!(record.counts_toward_acceptance_rate());
    }

    #[test]
    fn 同样的输入永远得到同样的决策() {
        // 可复现是「可解释」的前提。这条测试防的是有人往引擎里塞
        // HashMap、随机数或者「当前时间」这类隐藏输入。
        let engine = PolicyEngine::new();
        let input = base_input(HealthNeeds {
            rest: NeedScore::new(0.9),
            hydration: NeedScore::new(0.9),
            movement: NeedScore::new(0.9),
            eye_rest: NeedScore::new(0.9),
        });

        let first = engine.decide(&input);
        for _ in 0..50 {
            assert_eq!(engine.decide(&input), first);
        }
    }

    #[test]
    fn 自定义阈值生效() {
        let engine = PolicyEngine::with_threshold(0.5, 0.9);
        let input = base_input(needs_with(NeedKind::Rest, 0.6));

        assert_eq!(
            engine.decide(&input).level,
            InterventionLevel::Notification,
            "阈值降到 0.5 后，0.6 的需求应当触发提醒"
        );
    }

    #[test]
    fn 阈值被夹取到合法区间() {
        let engine = PolicyEngine::with_threshold(-1.0, 5.0);
        // 阈值 -1 → 0，任何正需求都触发；紧急阈值 5 → 1
        let input = base_input(needs_with(NeedKind::Rest, 1.0));
        assert_eq!(engine.decide(&input).level, InterventionLevel::FullScreen);
    }

    #[test]
    fn 建议动作贴合需求类型() {
        assert!(suggested_actions(NeedKind::Hydration)
            .iter()
            .any(|a| a.contains("喝")));
        assert!(suggested_actions(NeedKind::Movement)
            .iter()
            .any(|a| a.contains("走") || a.contains("伸")));
        assert!(suggested_actions(NeedKind::EyeRest)
            .iter()
            .any(|a| a.contains("远") || a.contains("闭眼")));
    }

    #[test]
    fn 未知上下文不影响基本判断() {
        // 渐进增强原则：拿不到上下文时，系统要照常工作。
        let engine = PolicyEngine::new();
        let input = base_input(needs_with(NeedKind::Rest, 0.95));

        assert!(input.context.foreground_app.is_none());
        assert_eq!(engine.decide(&input).level, InterventionLevel::FullScreen);
    }

    #[test]
    fn 极高需求在专注场景下也能达到通知级别() {
        // 「场景很敏感」不等于「永不提醒」。需求拉满时至少要给个轻提示，
        // 否则这个工具在用户最需要它的时候反而是哑的。
        let engine = PolicyEngine::new();
        let mut input = base_input(needs_with(NeedKind::Rest, 0.97));
        input.context.foreground_app = Some(ForegroundApp::new(
            "com.microsoft.VSCode",
            "Visual Studio Code",
            AppCategory::Editor,
        ));

        assert_eq!(engine.decide(&input).level, InterventionLevel::Notification);
    }
}
