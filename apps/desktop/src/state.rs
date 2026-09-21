//! 应用状态 —— 所有业务真值的持有者。
//!
//! ## 为什么用一个 `Mutex` 包住所有东西
//!
//! 有两个线程会同时访问这些状态：
//!
//! - **后台调度线程**：每 10 秒 tick 一次，更新状态机、算需求、做决策
//! - **界面线程**：用户点按钮时读写
//!
//! 最简单的正确做法就是一把锁。为什么不上更精细的锁：我们的操作是
//! 「每 10 秒做几十微秒的计算」和「用户偶尔点一下」——
//! 争用概率极低，而拆分锁带来的复杂度（死锁可能性、状态不一致窗口）
//! 是实打实的。**简单且正确，胜过精巧且需要论证。**
//!
//! ## 一次 tick 做什么
//!
//! ```text
//!   ① 采样上下文      platform → ContextSnapshot
//!   ② 推进状态机      idle_seconds → WorkClock（含空闲/休眠判定）
//!   ③ 读历史          storage → 距上次喝水/活动/休息多久
//!   ④ 算需求          health → HealthNeeds（四维分数 0~1）
//!   ⑤ 判时机          health::window → 现在适合打扰吗
//!   ⑥ 做决策          policy → InterventionDecision
//!   ⑦ 执行 + 记录     通知/Overlay + 写库
//! ```
//!
//! 这条链路是单向的，且每一步都能被单独测试（架构原则 3、5）。

use std::sync::{Mutex, MutexGuard};

use tacet_context::ContextEngine;
use tacet_core::event::EventBus;
use tacet_core::model::BehaviorKind;
use tacet_core::model::{InterventionLevel, InterventionOutcome, NeedKind, Reason};
use tacet_core::policy::{DecisionInput, InterventionDecision, PolicyEngine, RecentHistory};
use tacet_core::state::{WorkClock, WorkInput, WorkState};
use tacet_core::time::{Timestamp, MINUTE};
use tacet_health::need::{NeedCalculator, NeedInputs};
use tacet_health::window::WindowPolicy;
use tacet_platform::{Platform, PlatformError};
use tacet_storage::repo::{EventRepo, IntentRepo, InterventionRepo, SettingsRepo};
use tacet_storage::{Database, LocalOffset};

/// 一次 tick 的产物 —— 告诉调用方「这次需要做点什么」。
#[derive(Debug, Clone, PartialEq)]
pub enum TickOutcome {
    /// 什么都不用做。
    Quiet,
    /// 需要发出一次干预。
    Intervene(Box<InterventionDecision>),
}

/// 壳层的全部可变状态。
pub struct AppState {
    /// 平台能力（macOS 实现或测试替身）。
    pub platform: Box<dyn Platform>,
    /// 数据库。
    pub db: Database,
    /// 事件总线。
    pub bus: EventBus,
    /// 工作状态机。
    pub clock: WorkClock,
    /// 上下文引擎。
    pub context: ContextEngine,
    /// 需求计算器。
    pub calculator: NeedCalculator,
    /// 决策引擎。
    pub policy: PolicyEngine,
    /// 时机窗口策略。
    pub window_policy: WindowPolicy,
    /// 本机时区偏移（统计口径用）。
    pub offset: LocalOffset,
    /// 最近一次决策（界面展示「为什么」）。
    pub last_decision: Option<InterventionDecision>,
    /// 最近一次干预记录的 id（用户响应时要用）。
    pub last_intervention_id: Option<i64>,
    /// 最近一次真正打扰的时间（安静期判断用）。
    pub last_interruption_at: Option<Timestamp>,
    /// 本次休息的结束时刻；不在休息中时为 `None`。
    pub break_ends_at: Option<Timestamp>,
    /// 本次休息**计划的总时长**（秒）；不在休息中时为 `None`。
    ///
    /// 与 `break_ends_at` 是一对：一个记「什么时候结束」，一个记「一共多久」。
    /// 后者是进度环的分母，必须是整段休息里唯一不变的那个数 ——
    /// 详见 [`AppState::break_total_seconds`]。
    pub break_planned_seconds: Option<u32>,
    /// 用户延后到什么时候之前都不再打扰；没有延后时为 `None`。
    ///
    /// 与 `break_ends_at` 是两回事：前者是「用户正在休息，休息本身有结束时刻」，
    /// 后者是「用户拒绝了这次提醒，说稍后再说」。
    pub snooze_until: Option<Timestamp>,
    /// 用户手动暂停计时（「我离开一会儿」）。
    pub paused: bool,
    /// 用户最近一次「回到电脑前」的时刻（离开/空闲之后第一次开始工作）。
    ///
    /// ## 为什么需要它
    ///
    /// 休息与护眼这两类需求的信号之一是「距上次休息过了多久」，
    /// 而这个数是**按墙上时钟**算的 —— 它不会因为人不在就停下来。
    /// 于是「晚上 22 点离开、早上 9 点回来」会算出「距上次休息 11 小时」，
    /// 需求分数直接拉满，用户一坐下就被弹一个全屏「你该休息了」。
    ///
    /// 这在语义上就错了：**不在电脑前的那段时间，本身就是最彻底的休息**。
    /// 所以「回来」这一刻要把这个计时重新起算 —— 用户刚坐下时需求是 0，
    /// 之后随着他真的工作才慢慢涨上去。
    ///
    /// ## 为什么记在内存里而不是落库
    ///
    /// 它只影响「此刻该不该提醒」这一个判断，属于运行时状态，
    /// 不是用户行为的历史事实（历史事实由 `events` 表记录）。
    /// 应用重启后这个值会丢，但重启后 `continuous_work_minutes` 也是 0
    /// —— 同样是「从头开始算」，两者一致，不会产生矛盾。
    pub last_return_at: Option<Timestamp>,
}

impl AppState {
    /// 建立应用状态：打开数据库、读取设置、初始化各引擎。
    pub fn new(platform: Box<dyn Platform>) -> Result<Self, StateError> {
        let db = Database::open_default()?;
        let prefs = SettingsRepo::load_preferences(&db)?;

        let now = Timestamp::now();
        let idle_threshold_ms = prefs.idle_threshold_seconds() as i64 * 1000;

        let mut state = Self {
            platform,
            db,
            bus: EventBus::new(),
            clock: WorkClock::new(now, idle_threshold_ms),
            context: ContextEngine::new(),
            calculator: NeedCalculator::new(),
            policy: PolicyEngine::new(),
            window_policy: WindowPolicy::new(),
            offset: local_offset(),
            last_decision: None,
            last_intervention_id: None,
            last_interruption_at: None,
            break_ends_at: None,
            break_planned_seconds: None,
            snooze_until: None,
            paused: false,
            last_return_at: None,
        };

        state.close_dangling_break(now)?;
        Ok(state)
    }

    /// 收尾上一次运行留下的「未闭合的休息」。
    ///
    /// ## 为什么需要这件事
    ///
    /// 休息的结束时刻（`break_ends_at`）只存在于内存里，**不落库**。
    /// 所以应用在休息途中被重启（用户主动退出、崩溃、系统更新）时，
    /// 内存状态回到初始值，而 `break.started` 事件已经写进库了 ——
    /// 那条事件永远等不到它的 `break.completed`。
    ///
    /// 后果会在两个地方显现：
    ///
    /// 1. **统计失真**：查真实数据库时看到过一条休息记录了 1475 秒
    ///    （24.5 分钟），而用户设的是 5 分钟 —— 那多出来的时间其实是
    ///    应用重启到下一次有人操作之间的空档。
    /// 2. **状态悬空**：如果那条 `break.started` 之后又有新的行为被记录，
    ///    统计口径会把两段本不相干的时间连起来算。
    ///
    /// ## 为什么在启动时补一条完成事件，而不是删掉那条 started
    ///
    /// 删记录是篡改历史 —— 用户确实开始过那次休息，那是个事实。
    /// 补一条「完成」则如实说明了「这次休息结束了」。
    /// 只有在**确实存在**未闭合的休息时才写，正常启动不会留下多余记录。
    ///
    /// ## 结束时刻取的是「上界」，为什么可以接受
    ///
    /// 真实的结束时刻无从得知（那一刻应用已经死了，没人能记下来）。
    /// 能拿到的证据只有：[`EventRepo::first_after`] —— 那条 started
    /// **之后**最早出现的事件。它发生时应用一定已经重新活过来了，
    /// 所以休息必然已经结束。这是一个**上界**，可能比真实结束稍晚，
    /// 但它有界、有依据，而「永远悬空」是无限大的误差。
    ///
    /// ## 为什么必须扫全部历史，而不是只看最新一条
    ///
    /// 这里曾经写的是「最近一次 break.started 比最近一次 break.completed
    /// 更晚 → 判定悬空」，只看了一头一尾。这个写法漏掉了一类真实情况：
    ///
    /// ```text
    ///   18:45:17  break.started      ← 应用在这里被重启，永远没闭合
    ///   19:04:53  break.started      ← 用户又休息了一次，这次正常
    ///   19:09:52  break.completed
    /// ```
    ///
    /// 最新一条 started（19:04:53）确实早于最新一条 completed（19:09:52），
    /// 于是判定「没有悬空」——**而 18:45:17 那条就这么留在了库里**。
    /// 它还会一直留着：后续每次启动都只看最新一对，历史伤疤再也不会被看到。
    ///
    /// 真实数据库里就存在这样一条（2026-09-20 18:45:17），是这个测试
    /// 之外、靠人工查库才发现的。所以现在改成把整部历史配对一遍：
    /// 遇到「上一条还没收尾，下一条就开始了」，上一条就是悬空的。
    fn close_dangling_break(&mut self, now: Timestamp) -> Result<(), StateError> {
        // ── 第一遍：把整部休息史配对，找出所有没结尾的 started ──
        let mut open: Option<Timestamp> = None;
        let mut dangling: Vec<Timestamp> = Vec::new();

        for row in EventRepo::break_lifecycle(&self.db)? {
            match row.kind {
                BehaviorKind::BreakStarted => {
                    // 上一次还没收尾就又开始了一次 —— 上一次是悬空的
                    if let Some(started) = open.take() {
                        dangling.push(started);
                    }
                    open = Some(row.occurred_at);
                }
                // 这两种都是「这次休息结束了」的收尾。
                //
                // 显式列出而不是用 `_ =>`：万一 `break_lifecycle` 以后
                // 扩大了读取范围（比如加了 `break.snoozed`），新的类型
                // 应该落进「什么都没匹配上」那个分支、保持 `open` 不动，
                // 而不是被静默当成一次闭合 —— 后者会让真正悬空的那条
                // 又被漏掉，正是这个函数要修的毛病。
                BehaviorKind::BreakCompleted | BehaviorKind::BreakSkipped => open = None,
                _ => {}
            }
        }
        // 扫到最后还开着的那一条（被本次重启打断的那次休息）
        if let Some(started) = open {
            dangling.push(started);
        }

        if dangling.is_empty() {
            return Ok(());
        }

        // ── 第二遍：为每条悬空的休息算结束时刻，然后统一写入 ──
        //
        // 先把所有时刻算完再写：`first_after` 查的是「之后最早的事件」，
        // 如果我们边算边写，刚补进去的那条 completed 就可能被当成
        // 「后面还有事件」的证据，把下一条的上界算错。
        let closes: Vec<(Timestamp, Timestamp)> = dangling
            .into_iter()
            .map(|started| {
                let ended_at = EventRepo::first_after(&self.db, started)?
                    .unwrap_or(now)
                    // 兜底：上界不可能晚于「现在」
                    .min(now);
                Ok((started, ended_at))
            })
            .collect::<Result<_, StateError>>()?;

        for (started, ended_at) in closes {
            crate::logging::info(&format!(
                "发现未闭合的休息（开始于 {}），补记为完成（结束时刻取 {} 秒后的上界）",
                started.as_millis(),
                ended_at.millis_since(started).max(0) / 1000
            ));
            EventRepo::append(&self.db, BehaviorKind::BreakCompleted, "{}", ended_at)?;
        }

        Ok(())
    }

    /// 供测试使用：用内存数据库建一个干净的状态。
    #[cfg(test)]
    pub fn in_memory(platform: Box<dyn Platform>) -> Result<Self, StateError> {
        let db = Database::open_in_memory()?;
        let now = Timestamp::now();

        Ok(Self {
            platform,
            db,
            bus: EventBus::new(),
            clock: WorkClock::new(now, 5 * MINUTE),
            context: ContextEngine::new(),
            calculator: NeedCalculator::new(),
            policy: PolicyEngine::new(),
            window_policy: WindowPolicy::new(),
            offset: LocalOffset::utc(),
            last_decision: None,
            last_intervention_id: None,
            last_interruption_at: None,
            break_ends_at: None,
            break_planned_seconds: None,
            snooze_until: None,
            paused: false,
            last_return_at: None,
        })
    }

    /// 用户偏好（每次都从库里读，保证与设置页的改动一致）。
    pub fn preferences(&self) -> Result<tacet_core::model::UserPreferences, StateError> {
        Ok(SettingsRepo::load_preferences(&self.db)?)
    }

    /// 当前工作状态。
    pub fn work_state(&self) -> WorkState {
        self.clock.state()
    }

    /// 连续工作了多久（分钟）。
    pub fn continuous_work_minutes(&self) -> u32 {
        (self.clock.continuous_work_ms() / MINUTE) as u32
    }

    /// 收集最近发生过什么（决策的「记忆」部分）。
    pub fn recent_history(&self, now: Timestamp) -> Result<RecentHistory, StateError> {
        // 延后是否还有效：只把「还没到期」的延后传下去。
        //
        // 如果用户在 10 分钟前选了「3 分钟后」，现在早就该重新提醒了；
        // 传一个已经过去的时刻会让决策引擎永远保持安静 ——
        // 那是「提醒莫名其妙不再出现」这类 bug 的典型来源。
        let snoozed_until = self.snooze_until.filter(|until| now < *until);

        Ok(RecentHistory {
            last_intervention: InterventionRepo::last_disturbing(&self.db)?.map(|record| {
                tacet_core::policy::InterventionRecap {
                    kind: record.kind,
                    level: record.level,
                    fired_at: record.fired_at,
                }
            }),
            last_break_completed_at: EventRepo::last_occurrence(
                &self.db,
                BehaviorKind::BreakCompleted,
            )?,
            last_water_logged_at: EventRepo::last_occurrence(&self.db, BehaviorKind::WaterLogged)?,
            last_activity_logged_at: EventRepo::last_occurrence(
                &self.db,
                BehaviorKind::ActivityLogged,
            )?,
            last_eye_rest_logged_at: EventRepo::last_occurrence(
                &self.db,
                BehaviorKind::EyeRestLogged,
            )?,
            snoozed_until,
        })
    }

    /// 算一次四类需求。
    ///
    /// ## 「距上次休息」的口径：取「上次休息」与「回到电脑前」中较晚的那个
    ///
    /// 休息与护眼这两类需求，看的是「距上次满足过了多久」。这个数有两个
    /// 可能的来源：
    ///
    /// - 数据库里那条 `break.completed`（用户真的完成过一次休息）
    /// - 内存里的 `last_return_at`（用户刚从离开状态回到电脑前）
    ///
    /// 取较晚的那个，理由是**离开本身就是休息**：
    ///
    /// > 用户晚上 22 点关机走人、早上 9 点回来。库里最后一条
    /// > `break.completed` 停在 18:40 —— 距现在 14 小时。如果只看它，
    /// > 用户一坐下，需求分数就是满的，立刻弹「你该休息了」。
    /// > 但他刚睡了 8 小时，身体比谁都休息得充分。
    ///
    /// 取较晚者之后，「回来」那一刻需求归零，之后随着他真的坐下工作
    /// 才慢慢涨上去 —— 这才是这个数字应该表达的语义。
    ///
    /// 注意这**不是**在篡改历史：库里那条 `break.completed` 原样保留，
    /// 变的只是「此刻该不该提醒」这个判断的输入。
    pub fn needs(&self, now: Timestamp) -> Result<tacet_core::model::HealthNeeds, StateError> {
        let prefs = self.preferences()?;

        // 「最后一次满足了休息需求」的时刻 —— 完成休息、或刚从离开中回来
        let last_rest_satisfied = most_recent_of(
            EventRepo::last_occurrence(&self.db, BehaviorKind::BreakCompleted)?,
            self.last_return_at,
        );
        // 护眼同理（用户离开屏幕时，眼睛本身就在休息）
        let last_eye_rest_satisfied = most_recent_of(
            EventRepo::last_occurrence(&self.db, BehaviorKind::EyeRestLogged)?,
            self.last_return_at,
        );

        let inputs = NeedInputs {
            now,
            continuous_work_minutes: self.continuous_work_minutes(),
            last_break_completed_at: last_rest_satisfied,
            last_water_logged_at: EventRepo::last_occurrence(&self.db, BehaviorKind::WaterLogged)?,
            last_activity_logged_at: EventRepo::last_occurrence(
                &self.db,
                BehaviorKind::ActivityLogged,
            )?,
            last_eye_rest_logged_at: last_eye_rest_satisfied,
            settings: prefs.reminders,
        };

        Ok(self.calculator.needs(&inputs))
    }

    /// 工作状态发生变化时的统一处理：广播事件 + 落库。
    ///
    /// ## 为什么状态变化必须落库
    ///
    /// 今日统计里的「累计工作」「最长连续」都建立在 `work.started` /
    /// `work.paused` 这两类事件上（数据模型 §3.1 的 `events.kind` 里
    /// 明确列了它们）。如果只发到事件总线而不写库，`events` 表永远是空的，
    /// 界面上的今日统计就永远是 0。
    ///
    /// ## 为什么不会把数据库写爆
    ///
    /// 状态机的 `handle` 只在**真的切换了状态**时才返回 `Some`，
    /// 所以这里一天只会写几十条记录（每次离开、回来各一条），
    /// 而不是每 10 秒一条。这与「高频观测不落库」的原则不冲突。
    fn on_work_state_changed(
        &mut self,
        from: WorkState,
        to: WorkState,
        now: Timestamp,
    ) -> Result<(), StateError> {
        // 「用户回来了」是需求计时的重新起算点。
        //
        // 休息与护眼类需求看的是「距上次休息 / 远眺过了多久」，那个数是按
        // 墙上时钟算的。用户离开 8 小时（睡觉、出门）之后，那个数会累积到
        // 荒谬的程度，一坐下就被弹「你该休息了」—— 而他的身体刚休息完。
        // 详见 `last_return_at` 字段的说明。
        //
        // 只有从「离开/空闲」回到「工作」才算回来：
        // Working → Working 是不可能的（`handle` 只在真变化时返回 Some），
        // 而从 Breaking 结束回到 Working 由 `finish_break` 走，
        // 那次休息本身就是一次满足，不需要额外重置。
        if to == WorkState::Working && matches!(from, WorkState::Away | WorkState::Idle) {
            self.last_return_at = Some(now);
            crate::logging::info(&format!(
                "用户回到电脑前（从 {}），休息类需求的计时从此刻重新起算",
                from.as_str()
            ));
        }

        // 只有「开始工作」和「停止工作」两件事值得记。
        // Idle -> Working 与 Away -> Working 都算开始工作；
        // Working -> Away 与 Working -> Idle 都算停止。
        let (behavior, payload) = match to {
            WorkState::Working => (
                Some(BehaviorKind::WorkStarted),
                tacet_core::event::EventPayload::UserReturned,
            ),
            WorkState::Away => (
                Some(BehaviorKind::WorkPaused),
                tacet_core::event::EventPayload::UserIdleStarted,
            ),
            // 进入休息由 start_break 负责记录，这里不重复。
            // Idle 只是「还没开始」，不是一个值得记的事件。
            WorkState::Breaking | WorkState::Idle => {
                (None, tacet_core::event::EventPayload::UserIdleEnded)
            }
        };

        if let Some(kind) = behavior {
            // 带上来源状态，方便将来复盘「这段时间是怎么被切分的」
            EventRepo::append(
                &self.db,
                kind,
                &format!("{{\"from\":\"{}\"}}", from.as_str()),
                now,
            )?;
        }

        self.bus.emit(tacet_core::event::Event::new(
            now,
            tacet_core::event::EventSource::Context,
            payload,
        ));

        Ok(())
    }

    /// 执行一次完整的 tick。
    ///
    /// 这是整个应用的心跳。返回 [`TickOutcome`] 而不是直接发通知，
    /// 是为了让调度线程决定「怎么执行」，而状态层只管「该不该执行」——
    /// 这样这个函数可以被完整地单元测试。
    pub fn tick(&mut self, now: Timestamp) -> Result<TickOutcome, StateError> {
        // ① 采样上下文
        let context = self.context.sample(self.platform.as_ref(), now);

        // ② 推进状态机
        let idle_seconds = context.idle_seconds;
        if let Some(change) = self.clock.handle(WorkInput::Observe { idle_seconds }, now) {
            self.on_work_state_changed(change.from, change.to, now)?;
        }

        // 用户在暂停中：不做任何决策（但计时状态照常维护）
        if self.paused {
            return Ok(TickOutcome::Quiet);
        }

        // 休息到点了：自动结束
        //
        // 这里记一条日志，理由和 `commands::end_break` 那条一样：
        // 「休息怎么结束的」有两条路径（tick 到点收的 vs 用户主动结束），
        // 排查方向完全相反。用户报「自己就结束了」时，
        // 日志里这条「休息到点」就是自动路径的直接证据。
        if let Some(ends_at) = self.break_ends_at {
            if now >= ends_at {
                let overtime_ms = now.millis_since(ends_at).max(0);
                crate::logging::info(&format!(
                    "休息到点，自动结束（超时 {overtime_ms} ms，说明 tick 间隔正常）"
                ));
                self.finish_break(now)?;
                return Ok(TickOutcome::Quiet);
            }
        }

        // ③④ 读历史 + 算需求
        let recent = self.recent_history(now)?;
        let prefs = self.preferences()?;
        let needs = self.needs(now)?;

        // ⑤ 判时机
        let (_, top_score) = needs.highest();
        let urgent = top_score.get() >= 0.95;

        let window = self.window_policy.evaluate(
            now,
            &context,
            self.last_interruption_at,
            prefs.idle_threshold_seconds(),
            urgent,
        );

        if !window.is_open {
            // 窗口关着：记一条「为什么不说话」的决策，但等级为静默。
            //
            // 为什么连静默决策也记？因为「为什么刚才没提醒我」是一个
            // 真实会被问到的问题。有了这条记录才能回答它。
            let kind = needs.highest().0;
            self.last_decision = Some(InterventionDecision {
                kind,
                level: InterventionLevel::Silent,
                reasons: vec![match window.closed_reason {
                    Some(tacet_health::window::WindowClosedReason::Away) => Reason::UserAway,
                    Some(tacet_health::window::WindowClosedReason::Night) => {
                        Reason::ContextUnavailable
                    }
                    Some(tacet_health::window::WindowClosedReason::JustInterrupted) => {
                        Reason::RateLimited {
                            kind,
                            minutes_ago: self
                                .last_interruption_at
                                .map(|at| now.minutes_since(at).max(0) as u32)
                                .unwrap_or(0),
                        }
                    }
                    None => Reason::ContextUnavailable,
                }],
                actions: Vec::new(),
                fused: Vec::new(),
            });
            return Ok(TickOutcome::Quiet);
        }

        // ⑥ 做决策
        //
        // 先把空闲秒数取出来：`context` 接下来会被 move 进 `DecisionInput`，
        // 而决定之后那条日志还要用它（它是判断「人到底在不在」的关键证据）。
        let idle_seconds_at_decision = context.idle_seconds;

        let decision = self.policy.decide(&DecisionInput {
            now,
            context,
            needs,
            preferences: prefs,
            recent,
            is_breaking: self.work_state() == WorkState::Breaking,
            continuous_work_minutes: self.continuous_work_minutes(),
        });

        self.last_decision = Some(decision.clone());

        if decision.level == InterventionLevel::Silent {
            return Ok(TickOutcome::Quiet);
        }

        // 真的要打扰用户了 —— 记一笔。
        //
        // ## 为什么这条日志必须有
        //
        // 用户报过「我一直在息屏，还提醒休息」。查日志时发现**什么都没有**：
        // 提醒确实发出去了（`interventions` 表里有 10 条记录），但日志里
        // 一个字都没写，只能靠翻数据库才看出来。没有日志，就只能猜。
        //
        // 这条记录要把「当时是什么情况」一起写下来 —— 尤其是空闲秒数，
        // 它是判断「人到底在不在」的关键证据：
        //
        // - 日志里空闲 0 秒 → 用户确实在电脑前，提醒是合理的
        // - 日志里空闲几千秒 → 人不在却发了提醒，那就是判定逻辑有问题
        //
        // 有了这条，同类问题下次一眼就能定位，不用再翻数据库。
        crate::logging::info(&format!(
            "发出提醒：{:?} 等级 {:?}（空闲 {} 秒，连续工作 {} 分钟）",
            decision.kind,
            decision.level,
            idle_seconds_at_decision,
            self.continuous_work_minutes()
        ));

        // ⑦ 记录这次干预（执行由调用方完成）
        let record = decision.to_intervention(now);
        let id = InterventionRepo::insert(&self.db, &record)?;

        self.last_intervention_id = Some(id);
        self.last_interruption_at = Some(now);

        self.bus.emit(tacet_core::event::Event::new(
            now,
            tacet_core::event::EventSource::Policy,
            tacet_core::event::EventPayload::InterventionFired {
                kind: decision.kind,
                level: decision.level,
            },
        ));

        Ok(TickOutcome::Intervene(Box::new(decision)))
    }

    /// 记录一次用户行为（喝水 / 活动 / 远眺）。
    pub fn log_behavior(&mut self, kind: BehaviorKind, now: Timestamp) -> Result<(), StateError> {
        EventRepo::append(&self.db, kind, "{}", now)?;

        self.bus.emit(tacet_core::event::Event::new(
            now,
            tacet_core::event::EventSource::Ui,
            match kind {
                BehaviorKind::WaterLogged => tacet_core::event::EventPayload::WaterLogged,
                BehaviorKind::ActivityLogged => tacet_core::event::EventPayload::ActivityLogged,
                BehaviorKind::EyeRestLogged => tacet_core::event::EventPayload::EyeRestLogged,
                _ => tacet_core::event::EventPayload::ActivityLogged,
            },
        ));

        // 记录行为也算「用户回应了提醒」：如果刚才有一条待响应的干预，
        // 并且类型对得上，就把它标成已完成。
        if let Some(id) = self.last_intervention_id.take() {
            let matching = match kind {
                BehaviorKind::WaterLogged => NeedKind::Hydration,
                BehaviorKind::ActivityLogged => NeedKind::Movement,
                BehaviorKind::EyeRestLogged => NeedKind::EyeRest,
                _ => NeedKind::Rest,
            };

            let should_resolve = self
                .last_decision
                .as_ref()
                .is_some_and(|decision| decision.kind == matching);

            if should_resolve {
                InterventionRepo::resolve(&self.db, id, InterventionOutcome::Completed, None, now)?;
            }
        }

        Ok(())
    }

    /// 开始一次休息。
    ///
    /// ## 幂等保护：已经在休息中就直接返回
    ///
    /// 这个方法有两条调用路径（界面上点「现在休息」、提交 Intent 之后
    /// 真正进入休息，见 `BreakFlow.tsx`），IPC 命令本身也可以被直接调用。
    ///
    /// 没有保护的话，第二次调用会：
    /// - 重复写入 `break.started` 事件 → 「今天休息了几次」这类统计被算多
    /// - 重置 `break_ends_at` → 用户在 Intent 页面花的时间被「退回」，
    ///   实际休息时长超出设置值
    ///
    /// 已经在休息中时什么都不做，是最符合直觉的语义 ——
    /// 重复点「开始休息」不该有任何副作用。
    pub fn start_break(&mut self, now: Timestamp) -> Result<(), StateError> {
        if self.clock.state() == WorkState::Breaking {
            return Ok(());
        }

        let prefs = self.preferences()?;
        let duration_ms = prefs.break_duration_ms();

        // 先记一条「工作暂停」—— 休息的起点就是这一段连续工作的终点。
        // 今日统计里的「最长连续工作」正是靠这对事件算出来的
        //（见 scheduler::compute_longest_streak），漏了这条会让那项统计失真。
        if self.clock.state() == WorkState::Working {
            EventRepo::append(
                &self.db,
                BehaviorKind::WorkPaused,
                &format!("{{\"from\":\"{}\"}}", WorkState::Working.as_str()),
                now,
            )?;
        }

        self.clock.handle(WorkInput::StartBreak, now);
        self.break_ends_at = Some(now.saturating_add_millis(duration_ms));
        // 总时长和结束时刻一起定下来，整段休息里都不再变（进度环的分母）
        self.break_planned_seconds = Some((duration_ms / 1000) as u32);

        EventRepo::append(&self.db, BehaviorKind::BreakStarted, "{}", now)?;

        self.bus.emit(tacet_core::event::Event::new(
            now,
            tacet_core::event::EventSource::Ui,
            tacet_core::event::EventPayload::BreakStarted,
        ));

        Ok(())
    }

    /// 结束一次休息（正常结束或用户提前退出）。
    ///
    /// 返回这次休息期间记录的 Intent（如果有一条尚未恢复的），
    /// 由界面负责展示。
    ///
    /// ## 为什么 `clock.handle` 的返回值一定要接住
    ///
    /// `WorkClock::handle` 返回的是 [`StateChange`]（状态真的变了才有值），
    /// 而「写事件 + 广播」的活儿挂在 [`Self::on_work_state_changed`] 上。
    /// 早期这里写的是 `self.clock.handle(...)`，返回值被丢掉 ——
    /// 状态机确实回到了 Working，但**一条 `work.started` 都没落库**。
    ///
    /// 这个疏漏不会让界面立刻出错，所以它藏了很久，直到查数据库时发现
    /// `break.completed` 后面直接跟着下一次 `work.started {"from":"idle"}`，
    /// 中间少了一条本该由休息结束产生的事件。
    ///
    /// 代价落在统计上：`compute_longest_streak` 靠 `work.started` 划分工作
    /// 区间，缺一条就把「休息前」和「休息后」两段糊成一段，
    /// 今日的「最长连续工作」会系统性偏大。
    pub fn finish_break(
        &mut self,
        now: Timestamp,
    ) -> Result<Option<tacet_core::model::Intent>, StateError> {
        if let Some(change) = self.clock.handle(WorkInput::EndBreak, now) {
            self.on_work_state_changed(change.from, change.to, now)?;
        }
        self.break_ends_at = None;
        self.break_planned_seconds = None;

        EventRepo::append(&self.db, BehaviorKind::BreakCompleted, "{}", now)?;

        // 把最近一条未恢复的 Intent 标成已恢复，并返回给界面
        let intent = match IntentRepo::latest_unrestored(&self.db)? {
            Some(intent) => {
                if let Some(id) = intent.id {
                    IntentRepo::mark_restored(&self.db, id, now)?;
                }
                self.bus.emit(tacet_core::event::Event::new(
                    now,
                    tacet_core::event::EventSource::Ui,
                    tacet_core::event::EventPayload::IntentRestored {
                        id: id_intent(&intent),
                    },
                ));
                Some(intent)
            }
            None => None,
        };

        // 完成一次休息也算回应了「休息」这个需求
        if let Some(id) = self.last_intervention_id.take() {
            let was_rest = self
                .last_decision
                .as_ref()
                .is_some_and(|decision| decision.kind == NeedKind::Rest);
            if was_rest {
                InterventionRepo::resolve(&self.db, id, InterventionOutcome::Completed, None, now)?;
            }
        }

        Ok(intent)
    }

    /// 用户跳过这次休息。
    ///
    /// ## 为什么这里也要动状态机
    ///
    /// 多数情况下这个方法是安全的空操作：跳过发生在「询问」阶段，
    /// 那时用户还没真正开始休息，状态机处于 Working，
    /// `EndBreak` 从 Working 到 Working 不会产生任何变化。
    ///
    /// 但它**可以被在休息开始之后调用**（比如用户先点了「现在休息」，
    /// 之后又想跳过）。早期版本在这里只清 `break_ends_at`、不碰状态机，
    /// 结果是状态**永远卡在 Breaking**：
    ///
    /// - `WorkClock` 在 Breaking 态不累计工作计时
    /// - `observe` 里有一条「休息中不因为人离开而改变状态」的保护，
    ///   连「人离开」都救不回来
    ///
    /// 而 `break_ends_at` 已经被清空，tick 里的「到点自动结束」也永远不会
    /// 触发（它需要 `break_ends_at` 有值）。用户看到的是**计时再也不走了**。
    ///
    /// 所以这里显式地把状态机推回 Working，代价只是一次无副作用的调用。
    pub fn skip_break(&mut self, now: Timestamp) -> Result<(), StateError> {
        EventRepo::append(&self.db, BehaviorKind::BreakSkipped, "{}", now)?;

        if let Some(id) = self.last_intervention_id.take() {
            InterventionRepo::resolve(&self.db, id, InterventionOutcome::Skipped, None, now)?;
        }

        if let Some(change) = self.clock.handle(WorkInput::EndBreak, now) {
            self.on_work_state_changed(change.from, change.to, now)?;
        }
        self.break_ends_at = None;
        self.break_planned_seconds = None;
        Ok(())
    }

    /// 用户延后这次提醒。
    ///
    /// ## 为什么只有休息类才记 `break.snoozed` 事件
    ///
    /// 这个事件喂的是「今天休息了几次、延后了几次」那组统计。
    /// 早上十点弹出一条「该喝水了」，用户按了「3 分钟后」——
    /// 那和「休息被延后」是两回事，记进去会让休息的统计虚高。
    ///
    /// 判断依据是**这次提醒是为了哪类需求**（`last_decision.kind`），
    /// 而不是调用方传来的参数：延后这件事语义一致（都是「现在别烦我」），
    /// 区别只在记不记账。
    pub fn snooze(&mut self, minutes: u32, now: Timestamp) -> Result<(), StateError> {
        let is_rest = match self.last_decision.as_ref() {
            Some(decision) => decision.kind == NeedKind::Rest,
            None => true,
        };

        if is_rest {
            EventRepo::append(
                &self.db,
                BehaviorKind::BreakSnoozed,
                &format!("{{\"minutes\":{minutes}}}"),
                now,
            )?;
        }

        if let Some(id) = self.last_intervention_id.take() {
            InterventionRepo::resolve(
                &self.db,
                id,
                InterventionOutcome::Snoozed,
                Some(minutes),
                now,
            )?;
        }

        // 延后期间不打扰 —— 记下延后到什么时候。
        //
        // 这里用专门的 `snooze_until` 字段，而不是去拨动
        // `last_interruption_at`。后者是个 hack：那样会让「上次打扰是在
        // 几分钟前」这个事实被篡改，而这个事实还要用于限流判断和界面显示。
        self.snooze_until = Some(now.saturating_add_millis(minutes as i64 * MINUTE));

        Ok(())
    }

    /// 暂停 / 恢复计时。
    ///
    /// 返回 `Err` 只可能来自写库失败 —— 计时状态的切换本身不会失败。
    pub fn set_paused(&mut self, paused: bool, now: Timestamp) -> Result<(), StateError> {
        self.paused = paused;

        // 暂停 = 主动离开：结束当前这一段连续工作。
        // 「用户按了暂停」和「用户离开电脑」在产品上是一回事 ——
        // 这段离开时间不该被算进连续工作时长。
        let input = if paused {
            WorkInput::Sleep
        } else {
            WorkInput::Wake
        };

        if let Some(change) = self.clock.handle(input, now) {
            self.on_work_state_changed(change.from, change.to, now)?;
        }

        Ok(())
    }

    /// 系统即将休眠。
    pub fn on_sleep(&mut self, now: Timestamp) -> Result<(), StateError> {
        if let Some(change) = self.clock.handle(WorkInput::Sleep, now) {
            self.on_work_state_changed(change.from, change.to, now)?;
        }
        Ok(())
    }

    /// 系统唤醒。
    pub fn on_wake(&mut self, now: Timestamp) -> Result<(), StateError> {
        if let Some(change) = self.clock.handle(WorkInput::Wake, now) {
            self.on_work_state_changed(change.from, change.to, now)?;
        }
        Ok(())
    }

    /// 剩下的休息时间（秒）；不在休息中时为 `None`。
    pub fn break_remaining_seconds(&self, now: Timestamp) -> Option<u32> {
        let ends_at = self.break_ends_at?;
        let remaining = ends_at.millis_since(now).max(0) / 1000;
        Some(remaining as u32)
    }

    /// 这次休息计划的总时长（秒）；不在休息中时为 `None`。
    ///
    /// ## 为什么界面需要一个「另外的」数字
    ///
    /// 休息界面中间那个环要画「已经过去多少比例」，比例就得有分母。
    /// 直觉上分母是「休息总时长」，但早期实现偷懒用了**剩余秒数**：
    ///
    /// ```text
    ///   progress = 1 - remaining / remaining_of_last_snapshot
    /// ```
    ///
    /// 剩余秒数每 10 秒（调度器的 tick 间隔）随新快照刷新一次，分母于是
    /// 跟着分子一起变小，比例被反复拉回 0 —— 环每 10 秒倒退一次。
    /// 又因为 CSS 上挂了 1 秒的过渡，倒退那一下比前进快 10 倍，
    /// 看起来就是用户报的「**刚开始慢，然后快**」。
    ///
    /// 修法是让分母回到它本来就该是的东西：一个整段休息里恒定不变的数。
    /// 它只在 `start_break` 那一刻确定，中途任何快照刷新都不会改动它，
    /// 只有在休息真正结束时才被清掉。
    ///
    /// ## 为什么不做成「总时长 = 剩余 + 已过去」
    ///
    /// 那样算出来的值同样会随快照抖动（快照晚到几百毫秒就会差一秒），
    /// 分母一抖，环就跟着抖 —— 毛病还在。所以这里是**存下来**，
    /// 而不是**算出来**。
    pub fn break_total_seconds(&self) -> Option<u32> {
        self.break_planned_seconds
    }

    /// 取锁——给调度线程与命令层用的访问入口。
    ///
    /// 参数是 `&Mutex<Self>`，但调用方手上往往是
    /// `tauri::State<'_, Arc<Mutex<AppState>>>`。`State` 与 `Arc` 都能
    /// `Deref` 到 `Mutex`，所以 `AppState::lock(&state)` 能自动解引用 ——
    /// 这是 Rust 的 deref coercion 在这里帮了个忙。
    pub fn lock(state: &Mutex<Self>) -> MutexGuard<'_, Self> {
        match state.lock() {
            Ok(guard) => guard,
            // 锁中毒时继续用：某个线程在处理状态时崩了，我们宁可继续服务，
            // 也不要让整个应用变成哑巴（那会让所有记录静默丢失）。
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}

/// 从 Intent 里取出 id（用于事件载荷）。
fn id_intent(intent: &tacet_core::model::Intent) -> i64 {
    intent.id.unwrap_or(0)
}

/// 两个可选时刻里**较晚**的那个（都缺则为 `None`）。
///
/// 用于「最后一次满足了某类需求」这类口径：数据库里有一条历史记录，
/// 内存里可能还有一个更近的运行时事件，取较晚者才是「最近一次」。
///
/// 全是 `None` 时返回 `None` 而不是「现在」—— 「没有依据」和
/// 「刚刚满足过」是两回事，前者应当让需求保持为 0（见需求引擎里
/// 「没有任何记录时不报高需求」的说明）。
fn most_recent_of(a: Option<Timestamp>, b: Option<Timestamp>) -> Option<Timestamp> {
    match (a, b) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (Some(only), None) | (None, Some(only)) => Some(only),
        (None, None) => None,
    }
}

/// 读取本机时区偏移。
///
/// ## 为什么用系统命令而不是纯 Rust 方案
///
/// 核心层刻意不引入日期库（见 `tacet-core::time` 的说明），所以「本机
/// 相对 UTC 差多少」这件事只能从系统问。macOS 上最可靠的做法是读
/// `/etc/localtime` 链接指向的时区文件，但那需要解析二进制格式。
///
/// `date +%z` 是最直接的答案，代价是起一个子进程 —— 而这件事
/// 只在启动时做一两次（[`AppState::new`] 一次、日志初始化一次），
/// 成本完全可以接受。
///
/// 拿不到时退回 UTC：统计的「今天」会与用户预期差几小时，
/// 但不会崩，也不会丢数据。
///
/// ## 调用方注意
///
/// **每次调用都会真的起一个子进程。** 不要在循环里或者每个 tick 里调它 ——
/// 那会变成一个隐蔽的性能问题（每 10 秒 fork 一次进程）。
/// 需要反复使用时，调用一次把结果存起来。
pub fn local_offset() -> LocalOffset {
    let output = std::process::Command::new("date").arg("+%z").output();

    let Ok(output) = output else {
        return LocalOffset::utc();
    };

    let text = String::from_utf8_lossy(&output.stdout);
    parse_offset(&text).unwrap_or_else(LocalOffset::utc)
}

/// 解析 `+0800` / `-0500` 形式的时区偏移。
fn parse_offset(text: &str) -> Option<LocalOffset> {
    let text = text.trim();
    if text.len() < 5 {
        return None;
    }

    let sign = match text.as_bytes().first()? {
        b'+' => 1,
        b'-' => -1,
        _ => return None,
    };

    let hours: i32 = text.get(1..3)?.parse().ok()?;
    let minutes: i32 = text.get(3..5)?.parse().ok()?;

    Some(LocalOffset::from_minutes(sign * (hours * 60 + minutes)))
}

/// 状态层的错误。
#[derive(Debug, thiserror::Error)]
pub enum StateError {
    /// 存储层出错。
    #[error(transparent)]
    Storage(#[from] tacet_storage::StorageError),

    /// 核心层出错。
    #[error(transparent)]
    Core(#[from] tacet_core::CoreError),

    /// 平台层出错。
    #[error(transparent)]
    Platform(#[from] PlatformError),
}

/// 发一条系统通知（把决策翻译成通知文案）。
///
/// 这个函数放在状态层之外，因为它需要窗口身份（通知插件由壳层提供）。
pub fn notification_text(decision: &InterventionDecision) -> (String, String) {
    let (title, body) = match decision.kind {
        NeedKind::Rest => (
            "建议休息一下".to_string(),
            decision
                .reasons
                .first()
                .map(Reason::to_text)
                .unwrap_or_else(|| "该歇一会儿了".to_string()),
        ),
        NeedKind::Hydration => (
            "如果方便，记得喝点水".to_string(),
            decision
                .reasons
                .first()
                .map(Reason::to_text)
                .unwrap_or_else(|| "该喝水了".to_string()),
        ),
        NeedKind::Movement => (
            "起来活动一下".to_string(),
            decision
                .reasons
                .first()
                .map(Reason::to_text)
                .unwrap_or_else(|| "坐得有点久了".to_string()),
        ),
        NeedKind::EyeRest => (
            "让眼睛歇一会儿".to_string(),
            decision
                .reasons
                .first()
                .map(Reason::to_text)
                .unwrap_or_else(|| "看屏幕有点久了".to_string()),
        ),
        NeedKind::Fused => (
            "提醒一下".to_string(),
            decision
                .reasons
                .first()
                .map(Reason::to_text)
                .unwrap_or_default(),
        ),
    };

    (title, body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tacet_platform::MockPlatform;

    fn state() -> AppState {
        AppState::in_memory(Box::new(MockPlatform::new())).expect("建立状态")
    }

    // ======================================================== 未闭合的休息

    /// 回归测试：应用在休息途中重启，那条 `break.started` 必须被补上结束。
    ///
    /// ## 为什么这条重要
    ///
    /// 休息的结束时刻只在内存里，不落库。应用重启后内存状态清零，
    /// 但 `break.started` 已经写进库了 —— 那条事件永远等不到它的
    /// `break.completed`，于是下一次统计会把它和后来的时间连起来算。
    ///
    /// 真实数据库里出现过一次「休息 1475 秒」（设的是 300 秒），
    /// 多出来的 19 分钟正是应用重启到下一次操作之间的空档。
    #[test]
    fn 未闭合的休息会在启动时被补记完成() {
        let mut state = state();
        let now = Timestamp::now();
        let started_at = now.saturating_sub_millis(30 * MINUTE);

        // 造出「重启前刚开始休息」的库状态：只有 break.started，没有结束
        EventRepo::append(&state.db, BehaviorKind::BreakStarted, "{}", started_at).expect("写入");

        state.close_dangling_break(now).expect("收尾");

        let completed =
            EventRepo::last_occurrence(&state.db, BehaviorKind::BreakCompleted).expect("查询");
        assert!(
            completed.is_some(),
            "未闭合的休息应当被补记一条完成，否则统计永远悬空"
        );
    }

    /// 已经正常结束的休息**不该**被再补一条完成事件。
    ///
    /// 这个用例防的是「每次启动都往库里塞一条 break.completed」——
    /// 那会把「今天休息了几次」这类统计越算越多，
    /// 而且每次重启都多一条，很难被发现。
    #[test]
    fn 已正常结束的休息不会被重复补记() {
        let mut state = state();
        let now = Timestamp::now();

        state.start_break(now).expect("开始休息");
        state
            .finish_break(now.saturating_add_millis(5 * MINUTE))
            .expect("结束休息");

        let before = EventRepo::count_in_window(
            &state.db,
            BehaviorKind::BreakCompleted,
            &tacet_storage::DateWindow::day_of(now, state.offset),
        )
        .expect("计数");

        // 模拟一次重启后的收尾检查
        state
            .close_dangling_break(now.saturating_add_millis(10 * MINUTE))
            .expect("收尾");

        let after = EventRepo::count_in_window(
            &state.db,
            BehaviorKind::BreakCompleted,
            &tacet_storage::DateWindow::day_of(now, state.offset),
        )
        .expect("计数");

        assert_eq!(before, after, "已经闭合的休息不该被再次补记");
    }

    /// 跳过也算闭合，不该被补记。
    #[test]
    fn 跳过的休息不会被补记完成() {
        let mut state = state();
        let now = Timestamp::now();

        state.start_break(now).expect("开始休息");
        state
            .skip_break(now.saturating_add_millis(MINUTE))
            .expect("跳过");

        let before = EventRepo::count_in_window(
            &state.db,
            BehaviorKind::BreakCompleted,
            &tacet_storage::DateWindow::day_of(now, state.offset),
        )
        .expect("计数");

        state
            .close_dangling_break(now.saturating_add_millis(10 * MINUTE))
            .expect("收尾");

        let after = EventRepo::count_in_window(
            &state.db,
            BehaviorKind::BreakCompleted,
            &tacet_storage::DateWindow::day_of(now, state.offset),
        )
        .expect("计数");

        assert_eq!(before, after, "跳过已经是一次闭合，不该再补完成");
    }

    /// 全新库（从没休息过）不该产生任何记录。
    #[test]
    fn 全新的库不会被补记任何事件() {
        let mut state = state();
        let now = Timestamp::now();

        state.close_dangling_break(now).expect("收尾");

        let completed =
            EventRepo::last_occurrence(&state.db, BehaviorKind::BreakCompleted).expect("查询");
        assert!(
            completed.is_none(),
            "从没休息过的库不该凭空多出一条完成记录"
        );
    }

    /// 回归测试：**历史**留下的未闭合休息也必须被补上，哪怕后面还有正常的休息。
    ///
    /// ## 这条测试守的是「只看最新一对」这个错误的写法
    ///
    /// 真实数据库里发生过这样一段（2026-09-20）：
    ///
    /// ```text
    ///   18:45:17  break.started      ← 应用在这里被重启，永远没闭合
    ///   19:04:53  break.started      ← 用户又休息了一次，这次正常
    ///   19:09:52  break.completed
    /// ```
    ///
    /// 早期实现是「最近一次 started 比最近一次 closed 更晚才算悬空」，
    /// 于是它只看 19:04:53 与 19:09:52 —— 判定「没有悬空」，
    /// 而 18:45:17 那条永远留在了库里。
    ///
    /// 更糟的是它**无法自愈**：后面每换一次休息，最新一对都是闭合的，
    /// 那道旧伤疤再也不会被任何一次启动看到。
    ///
    /// 所以这条测试直接照搬真实数据的时间线。
    #[test]
    fn 历史遗留的未闭合休息也会被补记() {
        let mut state = state();
        let now = Timestamp::now();

        // 第一段：18:45:17 开始，被重启打断（没有结尾）
        let old_start = now.saturating_sub_millis(60 * MINUTE);
        EventRepo::append(&state.db, BehaviorKind::BreakStarted, "{}", old_start).expect("写入");

        // 用户重启后又做了一件别的事 —— 这是判断「上界」的依据
        let evidence = old_start.saturating_add_millis(44_000);
        EventRepo::append(&state.db, BehaviorKind::WorkStarted, "{}", evidence).expect("写入");

        // 第二段：一次完整正常的休息（开始 + 完成）
        let later_start = old_start.saturating_add_millis(20 * MINUTE);
        EventRepo::append(&state.db, BehaviorKind::BreakStarted, "{}", later_start).expect("写入");
        EventRepo::append(
            &state.db,
            BehaviorKind::BreakCompleted,
            "{}",
            later_start.saturating_add_millis(5 * MINUTE),
        )
        .expect("写入");

        state.close_dangling_break(now).expect("收尾");

        // 补齐之后，整部休息史里不该再有任何未闭合的 started。
        //
        // 这里按**时间**重排后再配对，而不是按 `break_lifecycle` 给的
        // 写入顺序 —— 因为补记的那条 `completed` 的时间戳可能早于
        // 后续事件（上界本来就落在两者之间），写入顺序会把它排到后面去。
        // 而「统计口径」看的是发生时间，所以测试也要按时间验证。
        let mut rows = EventRepo::break_lifecycle(&state.db).expect("读取");
        rows.sort_by_key(|row| (row.occurred_at.as_millis(), row.id));

        // 用一个「当前开着的那次休息」来配对：遇到 started 记下，
        // 遇到 completed / skipped 清空。跑完之后还开着的才是未闭合。
        let mut open: Option<Timestamp> = None;
        for row in &rows {
            match row.kind {
                BehaviorKind::BreakStarted => open = Some(row.occurred_at),
                BehaviorKind::BreakCompleted | BehaviorKind::BreakSkipped => open = None,
                _ => {}
            }
        }
        assert!(
            open.is_none(),
            "补记之后不该还有未闭合的休息，但这一条还开着：{open:?}"
        );

        // 补记的结束时刻应当是「之后最早那条事件」——44 秒，而不是现在（一小时）
        let closes: Vec<i64> = rows
            .iter()
            .filter(|row| row.kind == BehaviorKind::BreakCompleted)
            .map(|row| row.occurred_at.as_millis())
            .collect();
        assert!(
            closes.contains(&evidence.as_millis()),
            "结束时刻应当取「之后最早那条事件」作为上界（{evidence:?}），实际：{closes:?}"
        );
    }

    #[test]
    fn 全新状态下不工作() {
        let state = state();
        assert_eq!(state.work_state(), WorkState::Idle);
        assert_eq!(state.continuous_work_minutes(), 0);
    }

    #[test]
    fn 观测到活动后进入工作态() {
        let mut state = state();
        let now = Timestamp::now();

        state.tick(now).expect("tick");
        assert_eq!(state.work_state(), WorkState::Working);
    }

    #[test]
    fn 没有任何历史记录时不会提醒() {
        // 刚装好应用就弹「你已经 4 小时没喝水了」是最糟的第一印象。
        let mut state = state();
        let now = Timestamp::now();

        for _ in 0..10 {
            let outcome = state.tick(now).expect("tick");
            assert_eq!(outcome, TickOutcome::Quiet, "没有历史记录时不该提醒");
        }
    }

    /// 回归测试：状态变化必须落库。
    ///
    /// ## 这个 bug 是怎么被发现的
    ///
    /// 早期版本在状态切换时只做了 `bus.emit(...)`，没有写数据库。
    /// 结果是在单元测试里一切正常（它们只断言「状态机现在处于哪个状态」），
    /// 但真实跑起来时 `events` 表是**空的** —— 今日统计里的
    /// 「最长连续工作」永远显示 0。
    ///
    /// 这类 bug 的可怕之处在于它**完全静默**：没有报错，没有崩溃，
    /// 界面正常显示，只是数字永远不对。所以需要一条测试专门盯住
    /// 「状态变化 → 事件表多了一行」这个因果，而不只是盯住状态机。
    #[test]
    fn 工作状态变化会写进事件表() {
        use std::sync::Arc;

        let platform = MockPlatform::new();
        let control = Arc::clone(&platform.control);
        let mut state = AppState::in_memory(Box::new(platform)).expect("建立状态");
        let now = Timestamp::now();

        assert_eq!(
            EventRepo::last_occurrence(&state.db, BehaviorKind::WorkStarted).expect("查询"),
            None,
            "还没动过，不该有记录"
        );

        // ① Idle → Working：用户开始干活了
        state.tick(now).expect("tick");
        assert_eq!(state.work_state(), WorkState::Working);
        assert_eq!(
            EventRepo::last_occurrence(&state.db, BehaviorKind::WorkStarted).expect("查询"),
            Some(now),
            "开始工作必须落库，否则今日统计永远是 0"
        );

        // ② Working → Away：人离开了（空闲超过阈值）
        control.set_idle_seconds(10 * 60);
        let away = now.saturating_add_millis(11 * MINUTE);
        state.tick(away).expect("tick");
        assert_eq!(state.work_state(), WorkState::Away);
        assert_eq!(
            EventRepo::last_occurrence(&state.db, BehaviorKind::WorkPaused).expect("查询"),
            Some(away),
            "离开也必须落库 —— 最长连续工作是靠 started/paused 配对算出来的"
        );
    }

    /// 开始休息要同时留下「暂停工作」和「开始休息」两条记录。
    ///
    /// 只有 `break.started` 而没有 `work.paused` 的话，
    /// `compute_longest_streak` 会把休息之后的整段时间都算进同一段连续工作里，
    /// 「最长连续工作」就会越滚越大。
    #[test]
    fn 开始休息会封住上一段连续工作() {
        let mut state = state();
        let now = Timestamp::now();

        state.tick(now).expect("tick");
        assert_eq!(state.work_state(), WorkState::Working);

        let break_at = now.saturating_add_millis(50 * MINUTE);
        state.start_break(break_at).expect("开始休息");

        assert_eq!(
            EventRepo::last_occurrence(&state.db, BehaviorKind::WorkPaused).expect("查询"),
            Some(break_at),
            "休息的起点就是上一段连续工作的终点"
        );
        assert_eq!(
            EventRepo::last_occurrence(&state.db, BehaviorKind::BreakStarted).expect("查询"),
            Some(break_at)
        );
    }

    /// 回归测试：重复调用 `start_break` 必须是空操作。
    ///
    /// ## 这个 bug 是怎么发现的
    ///
    /// 界面上「点现在休息」和「提交 Intent」两步都会调用 `startBreak`，
    /// 而 Rust 侧没有幂等保护。后果是：
    /// - `break.started` 事件被写两次，今日统计里的休息次数偏多
    /// - `break_ends_at` 被重置，用户在 Intent 页面停留的时间白送了
    #[test]
    fn 重复开始休息不会产生副作用() {
        use std::sync::Arc;

        let platform = MockPlatform::new();
        let control = Arc::clone(&platform.control);
        let mut state = AppState::in_memory(Box::new(platform)).expect("建立状态");
        let now = Timestamp::now();

        state.tick(now).expect("tick");
        state.start_break(now).expect("第一次开始休息");

        let count_breaks = |s: &AppState| {
            tacet_storage::repo::EventRepo::count_in_window(
                &s.db,
                BehaviorKind::BreakStarted,
                &tacet_storage::DateWindow::day_of(now, s.offset),
            )
            .expect("统计")
        };

        assert_eq!(count_breaks(&state), 1);

        // 10 分钟后再次调用（模拟用户提交 Intent）
        let later = now.saturating_add_millis(10 * MINUTE);
        control.set_idle_seconds(0);
        state.start_break(later).expect("第二次开始休息");

        assert_eq!(
            count_breaks(&state),
            1,
            "重复调用不该再记一次 break.started"
        );

        // 倒计时终点不该被重置 —— 否则用户的实际休息时长会超出设置值
        let remaining = state.break_remaining_seconds(later);
        assert!(
            remaining.is_some_and(|s| s <= 300),
            "倒计时终点被重置了，剩余时长 {remaining:?} 超过了设置的 5 分钟"
        );
    }

    /// 端到端验证：**改提醒间隔 → 提醒频率真的变了**。
    ///
    /// ## 这条测试为什么重要
    ///
    /// 设置页里改一个数字，用户期待的是「提醒节奏跟着变」。
    /// 但从界面上的数字到真实的提醒行为，中间要穿过整整一条链路：
    ///
    /// ```text
    ///   设置页 → save_preferences 命令 → settings 表
    ///        → AppState::preferences() → NeedInputs.settings
    ///        → NeedCalculator 算需求分数
    ///        → PolicyEngine 用同一个间隔算冷却时长
    ///        → 决定这次要不要开口
    /// ```
    ///
    /// 这条链上的任何一环没接上，用户都会觉得「设置根本没用」——
    /// 而单元测试全绿，因为每一环单独看都是对的。
    ///
    /// ## 做法
    ///
    /// 走完整的 `tick()` 流程，在同样的时刻检查两次：
    /// 一次用短间隔（应当提醒），一次用长间隔（应当保持安静）。
    #[test]
    fn 把间隔调长之后提醒真的变少了() {
        use tacet_core::model::SettingsKey;
        use tacet_storage::repo::SettingsRepo;

        // ## 关于这个时间戳
        //
        // 用固定时刻而不是 `Timestamp::now()`，因为**这条业务链路会看时间**：
        // 深夜（23:00~06:00 UTC）一律不打扰。用 now() 的话，
        // 测试在白天跑会过、半夜跑会挂 —— 一个只在特定时刻失败的测试
        // 比没有测试更糟，它会让人开始不信任测试结果。
        //
        // 这里选 UTC 14:00（下午时段，允许打扰），并把它当成「现在」。
        // 取整到当天 14:00：先去掉不足一天的部分，再加上 14 小时。
        const DAY_MS: i64 = 24 * 60 * MINUTE;
        let now = Timestamp::from_millis(1_700_000_000_000 / DAY_MS * DAY_MS + 14 * 60 * MINUTE);

        // 先制造一次「很久没喝水」的历史：60 分钟前记过一次喝水
        let hour_ago = now.saturating_sub_millis(60 * MINUTE);

        /// 造一个「60 分钟没喝水、间隔设为 interval」的干净状态。
        fn scene(now: Timestamp, hour_ago: Timestamp, interval: u32) -> AppState {
            let mut state = state();

            // 让工作状态进入 Working —— 否则状态机还没开始计时
            state.tick(now).expect("tick");

            state
                .log_behavior(BehaviorKind::WaterLogged, hour_ago)
                .expect("记录喝水");

            let mut prefs = state.preferences().expect("读偏好");
            prefs.reminders.hydration.interval_minutes = interval;
            SettingsRepo::save_preferences(&state.db, &prefs).expect("保存");

            // 再 tick 一次，让状态机与需求都基于新设置稳定下来
            state.tick(now).expect("tick");
            state
        }

        // ── 第一步：间隔 45 分钟 ──
        // 需求 = 60 / 45 = 1.33 → 分数封顶 1.0，越过触发线。
        let mut short = scene(now, hour_ago, 45);

        let needs = short.needs(now).expect("算需求");
        assert!(
            needs.hydration.get() >= 1.0,
            "60 分钟没喝水、间隔 45 分钟，需求分数应当封顶，实际 {}",
            needs.hydration.get()
        );

        // 再 tick 一次取「本次是否真的开口」。
        //
        // 注意时间要推进到**冷却期之外**：`scene()` 里那次 tick 已经
        // 发出过提醒（并把 `last_interruption_at` 设成了 now），
        // 而冷却时长 = 用户间隔 × 1.0 = 45 分钟（冷却与设定值 1:1，
        // 见 `tacet_core::policy::cooldown_for` 的说明）。
        // 所以推进 46 分钟才是一次干净的「该不该再提醒」的检验。
        //
        // 这条断言本身也顺带验证了「短间隔下隔一个间隔就能再提醒」。
        let after_cooldown = now.saturating_add_millis(46 * MINUTE);
        let outcome = short.tick(after_cooldown).expect("tick");
        assert!(
            matches!(outcome, TickOutcome::Intervene(_)),
            "间隔 45 分钟时，过了 46 分钟（冷却一个完整间隔）应当可以再次提醒。\
             实际决策：{:?}，工作状态：{:?}",
            short.last_decision,
            short.work_state()
        );

        // ── 第二步：把间隔调长到 180 分钟 ──
        //
        // 需求变成 60 / 180 = 0.33，远低于触发线。
        // 这正是用户调大间隔时期待的效果：「别那么频繁地烦我」。
        let mut long = scene(now, hour_ago, 180);

        let needs = long.needs(now).expect("算需求");
        assert!(
            needs.hydration.get() < 1.0,
            "间隔调成 180 分钟之后需求应当降到触发线以下，实际 {}",
            needs.hydration.get()
        );

        assert_eq!(
            long.tick(after_cooldown).expect("tick"),
            TickOutcome::Quiet,
            "间隔调成 180 分钟之后，一小时没喝水不该再触发提醒 —— \
             否则设置就形同虚设"
        );

        // 顺带确认这个间隔确实能被读出来（写进去了 ≠ 读得出来）
        let saved = SettingsRepo::get(&long.db, SettingsKey::ReminderHydrationInterval)
            .expect("读设置")
            .expect("应当有值");
        assert_eq!(saved, serde_json::json!(180));
    }

    #[test]
    fn 记录喝水会被写库() {
        let mut state = state();
        let now = Timestamp::now();

        state
            .log_behavior(BehaviorKind::WaterLogged, now)
            .expect("记录");

        let last = EventRepo::last_occurrence(&state.db, BehaviorKind::WaterLogged).expect("查询");
        assert_eq!(last, Some(now));
    }

    #[test]
    fn 新增行为会改变需求分数() {
        let mut state = state();
        let now = Timestamp::now();

        // 先记录一次喝水
        state
            .log_behavior(BehaviorKind::WaterLogged, now)
            .expect("记录");

        // 45 分钟后（默认间隔），喝水需求应当到顶
        let later = now.saturating_add_millis(45 * MINUTE);
        let needs = state.needs(later).expect("算需求");

        assert!(
            needs.hydration.get() > 0.9,
            "45 分钟没喝水，需求应当接近满值，实际 {}",
            needs.hydration.get()
        );
    }

    #[test]
    fn 休息流程会记录开始与完成() {
        let mut state = state();
        let now = Timestamp::now();

        state.start_break(now).expect("开始休息");
        assert_eq!(state.work_state(), WorkState::Breaking);
        assert!(state.break_remaining_seconds(now).is_some());

        state
            .finish_break(now.saturating_add_millis(5 * MINUTE))
            .expect("结束休息");
        assert_eq!(state.work_state(), WorkState::Working, "休息后应当回到工作");

        let completed =
            EventRepo::last_occurrence(&state.db, BehaviorKind::BreakCompleted).expect("查询");
        assert!(completed.is_some(), "应当记录了休息完成");
    }

    /// 回归测试：休息结束后必须落一条 `work.started`。
    ///
    /// ## 这个 bug 是怎么被发现的
    ///
    /// 查真实数据库时注意到：`break.completed` 之后直接是下一次
    /// `work.started`，而且 payload 写的是 `{"from":"idle"}` ——
    /// 说明那条事件来自「空闲后重新观测到活动」，**不是**休息结束本身。
    ///
    /// 也就是说：每次休息结束，状态机确实回到了 Working，
    /// 但**没有任何事件被写下来**。原因是 `finish_break` 里
    /// `self.clock.handle(...)` 的返回值被丢掉了 —— 而状态变更的回调
    /// （`on_work_state_changed`，负责写事件 + 广播）正是挂在这个返回值上的。
    ///
    /// 后果：`compute_longest_streak` 依赖 `work.started` 来划分工作区间，
    /// 少一条就等于把「休息前」和「休息后」两段工作糊成了一段，
    /// 今日统计里的「最长连续工作」会系统性偏大。
    #[test]
    fn 休息结束会记录一条工作开始事件() {
        let mut state = state();
        let now = Timestamp::now();

        state.start_break(now).expect("开始休息");
        state
            .finish_break(now.saturating_add_millis(5 * MINUTE))
            .expect("结束休息");

        let started = EventRepo::recent_of_kind(&state.db, BehaviorKind::WorkStarted, 5)
            .expect("查询")
            .into_iter()
            .find(|row| row.occurred_at == now.saturating_add_millis(5 * MINUTE));

        let row = started.expect("休息结束应当落一条 work.started，否则统计会少算一段工作");
        assert!(
            row.payload.contains("breaking"),
            "这条 work.started 应当标明来源是 breaking，实际：{}",
            row.payload
        );
    }

    /// 回归测试：已经开始休息后再「跳过」，状态机必须回到工作。
    ///
    /// ## 为什么这条重要
    ///
    /// `skip_break` 早期只清掉了 `break_ends_at`，**没有动状态机**。
    /// 如果调用它时状态已经是 Breaking（用户先点了「现在休息」，
    /// 之后又想跳过），状态就会**永远卡在 Breaking**：
    ///
    /// - `WorkClock` 在 Breaking 态不累计工作计时
    /// - `observe` 里有一条「休息中不因为人离开而改变状态」的保护，
    ///   所以它会一直卡着，连「人离开」都救不回来
    /// - 面板会一直显示「休息中」，而休息窗口早就关了
    ///
    /// 用户看到的现象是「计时再也不走了」。
    #[test]
    fn 休息中跳过会回到工作状态() {
        let mut state = state();
        let now = Timestamp::now();

        state.start_break(now).expect("开始休息");
        assert_eq!(state.work_state(), WorkState::Breaking);

        state
            .skip_break(now.saturating_add_millis(MINUTE))
            .expect("跳过");

        assert_eq!(
            state.work_state(),
            WorkState::Working,
            "跳过之后必须回到工作，否则状态机会永远卡在 Breaking"
        );
        assert!(
            state.break_remaining_seconds(now).is_none(),
            "跳过之后不该还有休息倒计时"
        );
    }

    #[test]
    fn 休息到点会自动结束() {
        let mut state = state();
        let now = Timestamp::now();

        state.start_break(now).expect("开始休息");

        // 把时间推过休息时长
        let after = now.saturating_add_millis(6 * MINUTE);
        state.tick(after).expect("tick");

        assert_eq!(state.work_state(), WorkState::Working);
        assert!(state.break_remaining_seconds(after).is_none());
    }

    /// 回归测试：休息期间被反复 tick 打搅，不能提前结束。
    ///
    /// ## 为什么这个测试必须存在
    ///
    /// 真实用户报过「显示 5 分钟，过了一会儿就自己结束了」。
    /// 调度线程每 10 秒 tick 一次，休息期间这一次 tick 会：
    /// 采样上下文（读前台应用、空闲时长、全屏状态）→ 推进状态机 →
    /// 读历史 → 算需求 → 判时机。整条链路上任何一个环节误判，
    /// 都可能把用户从休息里「踢」出来。
    ///
    /// 这个测试把那条链路完整走一遍：每 10 秒 tick 一次，连续走满
    /// 4 分 50 秒（比默认的 5 分钟短 10 秒），期间**每一次**都必须
    /// 仍是 `Breaking`。只有真正到点后才能结束。
    ///
    /// 它保护的不只是「自动结束」那一个分支，而是整个 tick 链路
    /// 在休息态下的行为 —— 包括空闲观测、状态变更回调、需求计算。
    #[test]
    fn 休息期间反复_tick_不会提前结束() {
        use tacet_core::time::SECOND;

        let mut state = state();
        let now = Timestamp::now();
        state.start_break(now).expect("开始休息");

        // 每 10 秒一次，走 29 次 = 290 秒（默认时长 300 秒之内）
        for i in 1..=29u32 {
            let at = now.saturating_add_millis(i as i64 * 10 * SECOND);
            state.tick(at).expect("tick");

            assert_eq!(
                state.work_state(),
                WorkState::Breaking,
                "第 {i} 次 tick（第 {} 秒）时不该结束休息",
                i * 10
            );
            assert!(
                state.break_remaining_seconds(at).is_some(),
                "第 {i} 次 tick 后剩余时间不该消失"
            );
        }

        // 299 秒时仍在休息（再差 1 秒才到点）
        let almost = now.saturating_add_millis(299 * SECOND);
        state.tick(almost).expect("tick");
        assert_eq!(
            state.work_state(),
            WorkState::Breaking,
            "299 秒时还差 1 秒，不该已经结束"
        );

        // 301 秒：这次才该结束
        let past = now.saturating_add_millis(301 * SECOND);
        state.tick(past).expect("tick");
        assert_eq!(state.work_state(), WorkState::Working, "过了时长就该结束");
    }

    /// 回归测试：进度环的分母（休息总时长）在整段休息里必须恒定。
    ///
    /// ## 这个 bug 是怎么被发现的
    ///
    /// 用户报「休息时外面那个圈刚开始慢，然后快」。
    ///
    /// 环画的是 `已过去 / 总共`，而前端把**剩余秒数**当成了分母 ——
    /// 剩余每秒在减，分母跟着一起缩，比例于是被反复拉回 0：
    ///
    /// ```text
    ///   第 0 秒   比例 0
    ///   第 1~9 秒 比例 1/300 … 9/300（慢）
    ///   第 10 秒  新快照到，分母变成 290 → 比例退回 0
    ///   第 11 秒  从 1/290 重新爬（这一格比上一格大 3%）
    /// ```
    ///
    /// 每 10 秒（调度器的 tick 间隔）重复一次，看上去就是「慢慢爬，
    /// 然后突然跳快」。
    ///
    /// 这条测试守住修法的核心：`break_total_seconds` 从开始休息到
    /// 结束之前**一个数都不许变**，而且它必须和一开始设置的时长一致。
    #[test]
    fn 休息总时长在整段休息里恒定不变() {
        use tacet_core::time::SECOND;

        let mut state = state();
        let now = Timestamp::now();

        // 没在休息时没有总时长
        assert!(
            state.break_total_seconds().is_none(),
            "不在休息中时不该有总时长"
        );

        state.start_break(now).expect("开始休息");

        let planned = state.break_total_seconds().expect("开始休息后应当有总时长");
        assert_eq!(planned, 300, "默认休息时长是 5 分钟");

        // 走完整段休息，每一步都核对分母没变
        // （特别是 tick 之后 —— 快照正是在 tick 里生成的）
        for i in 1..=29u32 {
            let at = now.saturating_add_millis(i as i64 * 10 * SECOND);
            state.tick(at).expect("tick");

            assert_eq!(
                state.break_total_seconds(),
                Some(planned),
                "第 {i} 次 tick（第 {} 秒）后总时长被改动了 —— \
                 分母一变，进度环就会倒退重画",
                i * 10
            );
        }

        // 299 秒时仍在休息，分母依然不变
        let almost = now.saturating_add_millis(299 * SECOND);
        state.tick(almost).expect("tick");
        assert_eq!(
            state.break_total_seconds(),
            Some(planned),
            "休息即将结束时总时长仍不该变"
        );

        // 结束后才清空 —— 下一次休息是新的分母
        let past = now.saturating_add_millis(301 * SECOND);
        state.tick(past).expect("tick");
        assert_eq!(state.work_state(), WorkState::Working, "该结束了");
        assert!(
            state.break_total_seconds().is_none(),
            "休息结束后应当清空，否则下一次休息会沿用旧分母"
        );
    }

    /// 回归测试：休息总时长与剩余秒数必须自洽。
    ///
    /// 这两个数是一对：`总 = 剩余 + 已过去`。分母一旦和倒计时对不上，
    /// 环画出来的比例就是错的 —— 要么永远走不满，要么提前转完。
    ///
    /// 这里逐秒核对两者的关系，顺便钉死「剩余确实在随真实时间减少」
    /// 这个前提（如果剩余本身不动，环当然也不动，那是另一种故障）。
    #[test]
    fn 休息总时长与剩余秒数自洽() {
        use tacet_core::time::SECOND;

        let mut state = state();
        let now = Timestamp::now();
        state.start_break(now).expect("开始休息");

        let total = state.break_total_seconds().expect("总时长");

        for elapsed in [0i64, 1, 30, 60, 150, 299] {
            let at = now.saturating_add_millis(elapsed * SECOND);
            let remaining = state.break_remaining_seconds(at).expect("剩余");

            // 整秒对齐时，剩余 + 已过去 应当正好等于总时长
            assert_eq!(
                remaining + elapsed as u32,
                total,
                "第 {elapsed} 秒：剩余 {remaining} + 已过去 {elapsed} \
                 应当等于总时长 {total}"
            );
        }
    }

    /// 回归测试：休息期间收到「用户离开」的观测，休息不该被打断。
    ///
    /// 去倒水、去窗边远眺本来就会离开电脑 —— 这恰恰是休息该有的样子。
    /// 如果状态机因为「观测到空闲」而把状态切走，休息就废了。
    ///
    /// 注意这里**刻意把 tick 控制在休息时长之内**：超过 300 秒本来
    /// 就该正常结束，那属于另一条路径（见上一个测试）。这个测试要盯的是
    /// 「人离开」这个信号本身。
    #[test]
    fn 休息期间观测到长时间空闲不会打断休息() {
        use tacet_core::time::SECOND;

        let platform = MockPlatform::new();
        let control = std::sync::Arc::clone(&platform.control);
        let mut state = AppState::in_memory(Box::new(platform)).expect("建立状态");

        let now = Timestamp::now();
        state.start_break(now).expect("开始休息");

        // 用户起身去倒水：空闲时长一路涨到远超阈值（默认 5 分钟）
        for i in 1..=8u32 {
            control.set_idle_seconds(600 + i * 30);
            let at = now.saturating_add_millis(i as i64 * 30 * SECOND);
            state.tick(at).expect("tick");

            assert_eq!(
                state.work_state(),
                WorkState::Breaking,
                "第 {i} 次 tick（空闲 {} 秒）时休息被打断了",
                600 + i * 30
            );
        }
    }

    /// 回归测试：**离开一整天之后回来，不该立刻被弹休息提醒**。
    ///
    /// ## 这个 bug 是怎么被发现的
    ///
    /// 用户报「我一直在息屏，还提醒休息」。除了窗口策略那条（人不在
    /// 仍然放行），还有这第二个原因：
    ///
    /// 休息需求看的是「距上次满足休息过了多久」，而这个数是按**墙上时钟**
    /// 算的 —— 人不在，它照样在涨。用户晚上 22 点离开、早上 9 点回来，
    /// 这个数已经累积了 11 小时，需求分数爆表，一坐下就吃一个全屏提醒。
    ///
    /// 但语义上完全说不通：**不在电脑前的那段时间，本身就是最彻底的休息**。
    /// 他刚睡了 8 小时，是该被提醒「你该休息了」吗？
    ///
    /// 这条测试照搬真实数据的时间线。
    #[test]
    fn 离开很久之后回来不会被立刻提醒休息() {
        use tacet_core::time::HOUR;

        let platform = MockPlatform::new();
        let control = std::sync::Arc::clone(&platform.control);
        let mut state = AppState::in_memory(Box::new(platform)).expect("建立状态");

        let now = Timestamp::now();

        // 上一次完成休息是 14 小时前（昨天傍晚）
        let long_ago = now.saturating_sub_millis(14 * HOUR);
        EventRepo::append(&state.db, BehaviorKind::BreakCompleted, "{}", long_ago).expect("写入");

        // 用户昨晚离开、现在刚回来。先让他进入 Away 状态。
        control.set_idle_seconds(11 * 3600);
        state.tick(now).expect("tick");
        assert_eq!(state.work_state(), WorkState::Away, "应当已判定为离开");

        // 此刻的需求确实是满的 —— 这是事实，不是 bug
        let while_away = state.needs(now).expect("算需求");
        assert!(
            while_away.rest.get() >= 0.99,
            "离开 14 小时，需求分数本来就该是满的（实际 {}）",
            while_away.rest.get()
        );

        // 用户回来了：动了一下鼠标
        control.set_idle_seconds(0);
        let back = now.saturating_add_millis(MINUTE);
        state.tick(back).expect("tick");
        assert_eq!(state.work_state(), WorkState::Working, "应当已回到工作态");

        // 关键：刚回来的这一刻，休息需求必须被清空
        let just_back = state.needs(back).expect("算需求");
        assert!(
            just_back.rest.get() < 0.05,
            "刚回到电脑前，休息需求应当接近零（实际 {}）—— \
             离开的 11 小时本身就是休息，不该让用户一坐下就被提醒",
            just_back.rest.get()
        );
    }

    /// 回归测试：回来之后，需求要随着真正的工作重新涨上去。
    ///
    /// 上一条测试守的是「回来那一刻归零」。但如果归零之后**永远不涨**，
    /// 那提醒就彻底失效了 —— 用一个「需求永远为零」的 bug 去修
    /// 「需求虚高」的 bug，等于把产品功能删掉。
    ///
    /// 所以这条测试要走完一个完整的间隔，确认提醒能力还在。
    ///
    /// ## 为什么要 tick 很多次，而不是一次跳到一小时之后
    ///
    /// `WorkClock` 有一条**大跳步封顶**（`MAX_STEP_MS = 60 秒`）：
    /// 两次更新之间超过 60 秒的部分不计入工作时长。这是有意为之的保护 ——
    /// 系统休眠、进程被挂起时，那段时间不该被算成「连续工作」。
    ///
    /// 所以「一小时的工作」必须由许多次小步长的 tick 累积出来，
    /// 就像真实应用那样（调度器每 10 秒 tick 一次）。一次跳一小时
    /// 只会记下 60 秒 —— 那是在测封顶保护，不是在测需求累积。
    #[test]
    fn 回来之后需求会随工作时间重新累积() {
        use tacet_core::time::{MINUTE, SECOND};

        let platform = MockPlatform::new();
        let control = std::sync::Arc::clone(&platform.control);
        let mut state = AppState::in_memory(Box::new(platform)).expect("建立状态");

        let now = Timestamp::now();
        // 上一次休息是很久以前
        EventRepo::append(
            &state.db,
            BehaviorKind::BreakCompleted,
            "{}",
            now.saturating_sub_millis(600 * MINUTE),
        )
        .expect("写入");

        // 离开后回来
        control.set_idle_seconds(3600);
        state.tick(now).expect("tick");
        control.set_idle_seconds(0);
        let back = now.saturating_add_millis(MINUTE);
        state.tick(back).expect("tick");

        let just_back = state.needs(back).expect("算需求");
        assert!(
            just_back.rest.get() < 0.05,
            "刚回来时应当接近零（实际 {}）",
            just_back.rest.get()
        );

        // 用户真的工作了一小时 —— 按真实节奏每 30 秒 tick 一次
        // （步长必须小于 MAX_STEP_MS，否则会被封顶保护吃掉）
        let mut at = back;
        for _ in 0..122 {
            at = at.saturating_add_millis(30 * SECOND);
            state.tick(at).expect("tick");
        }

        let after_work = state.needs(at).expect("算需求");
        assert!(
            after_work.rest.get() > 0.9,
            "工作一小时（{} 分钟）后需求应当重新涨上来（实际 {}）—— \
             否则提醒就再也不工作了",
            at.millis_since(back) / MINUTE,
            after_work.rest.get()
        );
    }

    #[test]
    fn 记录行为会结算对应的干预() {
        let mut state = state();
        let now = Timestamp::now();

        // 手工造一条「刚提醒过喝水」的状态
        let decision = InterventionDecision {
            kind: NeedKind::Hydration,
            level: InterventionLevel::Notification,
            reasons: vec![Reason::SinceLastHydration { minutes: 93 }],
            actions: vec!["喝几口水".to_string()],
            fused: Vec::new(),
        };
        let id = InterventionRepo::insert(&state.db, &decision.to_intervention(now)).expect("写入");
        state.last_intervention_id = Some(id);
        state.last_decision = Some(decision);

        // 用户点了「+1 杯水」
        state
            .log_behavior(BehaviorKind::WaterLogged, now)
            .expect("记录");

        let record = InterventionRepo::find(&state.db, id)
            .expect("查询")
            .expect("存在");
        assert_eq!(
            record.outcome,
            Some(InterventionOutcome::Completed),
            "记录喝水应当把对应的提醒结算为已完成"
        );
    }

    #[test]
    fn 类型不匹配的行为不会结算干预() {
        let mut state = state();
        let now = Timestamp::now();

        let decision = InterventionDecision {
            kind: NeedKind::Hydration,
            level: InterventionLevel::Notification,
            reasons: Vec::new(),
            actions: Vec::new(),
            fused: Vec::new(),
        };
        let id = InterventionRepo::insert(&state.db, &decision.to_intervention(now)).expect("写入");
        state.last_intervention_id = Some(id);
        state.last_decision = Some(decision);

        // 用户记录的是「活动」，不是「喝水」——不该结算那条喝水提醒
        state
            .log_behavior(BehaviorKind::ActivityLogged, now)
            .expect("记录");

        let record = InterventionRepo::find(&state.db, id)
            .expect("查询")
            .expect("存在");
        assert!(
            record.outcome.is_none(),
            "不相关的行为不该结算提醒，否则统计会失真"
        );
    }

    #[test]
    fn 暂停时不做决策() {
        let mut state = state();
        let now = Timestamp::now();

        state.set_paused(true, now).expect("暂停");
        let outcome = state.tick(now).expect("tick");

        assert_eq!(outcome, TickOutcome::Quiet);
        assert_eq!(state.work_state(), WorkState::Away, "暂停等于离开");
    }

    #[test]
    fn 恢复后重新开始计时() {
        let mut state = state();
        let now = Timestamp::now();

        state.set_paused(true, now).expect("暂停");
        state
            .set_paused(false, now.saturating_add_millis(MINUTE))
            .expect("恢复");

        assert_eq!(state.work_state(), WorkState::Idle);
        state
            .tick(now.saturating_add_millis(2 * MINUTE))
            .expect("tick");
        assert_eq!(state.work_state(), WorkState::Working);
    }

    #[test]
    fn 系统休眠与唤醒() {
        let mut state = state();
        let now = Timestamp::now();

        state.tick(now).expect("tick");
        assert_eq!(state.work_state(), WorkState::Working);

        state.on_sleep(now).expect("休眠");
        assert_eq!(state.work_state(), WorkState::Away);
        assert_eq!(state.continuous_work_minutes(), 0, "休眠应清零连续工作");

        state
            .on_wake(now.saturating_add_millis(8 * 60 * MINUTE))
            .expect("唤醒");
        assert_eq!(state.work_state(), WorkState::Idle);
    }

    #[test]
    fn 延后会推迟打扰() {
        let mut state = state();
        let now = Timestamp::now();

        state.snooze(3, now).expect("延后");

        // 紧接着 tick：应当因为安静期而保持静默
        let outcome = state.tick(now.saturating_add_millis(30_000)).expect("tick");
        assert_eq!(outcome, TickOutcome::Quiet);
    }

    #[test]
    fn 跳过会记录并结算() {
        let mut state = state();
        let now = Timestamp::now();

        let decision = InterventionDecision {
            kind: NeedKind::Rest,
            level: InterventionLevel::FullScreen,
            reasons: Vec::new(),
            actions: Vec::new(),
            fused: Vec::new(),
        };
        let id = InterventionRepo::insert(&state.db, &decision.to_intervention(now)).expect("写入");
        state.last_intervention_id = Some(id);
        state.last_decision = Some(decision);

        state.skip_break(now).expect("跳过");

        let record = InterventionRepo::find(&state.db, id)
            .expect("查询")
            .expect("存在");
        assert_eq!(record.outcome, Some(InterventionOutcome::Skipped));
    }

    #[test]
    fn 结束休息会取回未恢复的待办() {
        let mut state = state();
        let now = Timestamp::now();

        // 用户记录了 Intent
        let intent = tacet_core::model::Intent::new("完成 Auth 模块测试", now).expect("创建");
        IntentRepo::save(&state.db, &intent).expect("保存");

        state.start_break(now).expect("开始休息");
        let restored = state
            .finish_break(now.saturating_add_millis(5 * MINUTE))
            .expect("结束休息");

        let restored = restored.expect("应当取回一条 Intent");
        assert_eq!(restored.text, "完成 Auth 模块测试");

        // 第二次结束不该再取回同一条
        state.start_break(now).expect("开始休息");
        let again = state
            .finish_break(now.saturating_add_millis(MINUTE))
            .expect("结束休息");
        assert!(again.is_none(), "已恢复过的 Intent 不该重复还给用户");
    }

    #[test]
    fn 通知文案符合语气规范() {
        // PRD §5：像一位懂分寸的同事，不像监工。
        let decision = InterventionDecision {
            kind: NeedKind::Hydration,
            level: InterventionLevel::Notification,
            reasons: vec![Reason::SinceLastHydration { minutes: 93 }],
            actions: Vec::new(),
            fused: Vec::new(),
        };

        let (title, body) = notification_text(&decision);

        assert_eq!(title, "如果方便，记得喝点水");
        for forbidden in ["必须", "应该", "又没", "立刻"] {
            assert!(!title.contains(forbidden), "标题不该含「{forbidden}」");
            assert!(!body.contains(forbidden), "正文不该含「{forbidden}」");
        }
    }

    #[test]
    fn 休息相关的通知建议的是休息() {
        let decision = InterventionDecision {
            kind: NeedKind::Rest,
            level: InterventionLevel::FullScreen,
            reasons: vec![Reason::ContinuousWork { minutes: 78 }],
            actions: Vec::new(),
            fused: Vec::new(),
        };

        let (title, body) = notification_text(&decision);
        assert_eq!(title, "建议休息一下");
        assert!(body.contains("78"));
    }

    #[test]
    fn 时区偏移解析() {
        assert_eq!(parse_offset("+0800").map(|o| o.minutes()), Some(480));
        assert_eq!(parse_offset("-0500").map(|o| o.minutes()), Some(-300));
        assert_eq!(parse_offset("+0530").map(|o| o.minutes()), Some(330));
        assert_eq!(parse_offset("+0000").map(|o| o.minutes()), Some(0));
        assert_eq!(parse_offset("").map(|o| o.minutes()), None);
        assert_eq!(parse_offset("abc").map(|o| o.minutes()), None);
    }

    #[test]
    fn 本机时区偏移可读且合理() {
        let offset = local_offset();
        // 现实中的时区都在 ±14 小时内
        assert!(
            offset.minutes().abs() <= 14 * 60,
            "读到的时区偏移 {} 分钟不合理",
            offset.minutes()
        );
    }

    #[test]
    fn 状态锁在中毒后仍可用() {
        use std::sync::Arc;

        let state = Arc::new(Mutex::new(state()));

        // 制造一次中毒
        let clone = Arc::clone(&state);
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = AppState::lock(&clone);
            panic!("故意崩掉");
        }));

        // 之后仍然能取到锁
        let guard = AppState::lock(&state);
        assert_eq!(guard.work_state(), WorkState::Idle);
    }

    // ======================================================== 端到端：到点提醒

    /// 用一个固定时刻（UTC 下午）造状态 —— 避开「深夜不打扰」的规则，
    /// 也避开「测试在半夜跑就挂」这个坑。
    fn afternoon() -> Timestamp {
        const DAY_MS: i64 = 24 * 60 * MINUTE;
        Timestamp::from_millis(1_700_000_000_000 / DAY_MS * DAY_MS + 14 * 60 * MINUTE)
    }

    /// 造一个「用户把休息间隔设成 minutes 分钟、其余三类关掉」的干净状态。
    ///
    /// ## 为什么要关掉其余三类
    ///
    /// 默认设置里护眼是 40 分钟、休息是 50 分钟 —— 护眼会先到点。
    /// 这条测试要验证的是「休息的 45 分钟是否精确」，
    /// 留着其它需求只会让第一次开口被别的类型抢走。
    /// 关掉它们，测的才是单纯的休息这一条时间线。
    fn state_with_rest_interval(start: Timestamp, minutes: u32) -> AppState {
        let mut state = state();

        let mut prefs = state.preferences().expect("读偏好");
        prefs.reminders.rest = tacet_core::model::ReminderRule::new(true, minutes);
        prefs.reminders.hydration.enabled = false;
        prefs.reminders.movement.enabled = false;
        prefs.reminders.eye_rest.enabled = false;
        SettingsRepo::save_preferences(&state.db, &prefs).expect("保存偏好");

        // 让状态机进入 Working 并开始计时
        state.tick(start).expect("首次 tick");
        state
    }

    /// 验收测试：**设多少分钟，就在第几分钟提醒**。
    ///
    /// ## 这条测试盯住的是用户报的那个问题
    ///
    /// > 「我设置的 45 分钟，但是时间到了没有提醒我休息？」
    ///
    /// 根因之一是触发线被打了 0.75 的折：设 45 分钟，第 34 分钟就触发了。
    /// 用户第 45 分钟抬头看时，那次提醒早就过去了（何况它还看不见）。
    ///
    /// 所以这里逐分钟 tick，检查**第一次开口恰好落在第 45 分钟**。
    /// 用真实的 `AppState` 而不是孤立函数，是因为这个问题横跨
    /// 需求评分 → 时机窗口 → 决策引擎 → 状态机四层，
    /// 只测任何一层都抓不住它。
    #[test]
    fn 设多少分钟就在第几分钟提醒() {
        use tacet_core::model::BehaviorKind;
        use tacet_storage::repo::EventRepo;

        let start = afternoon();
        let mut state = state_with_rest_interval(start, 45);

        // 「距上次休息」需要有一个起点，否则需求一直是「无记录」= 0 分。
        // 真实的起点是「用户回到电脑前」（`last_return_at`），
        // 这里用同一件事的另一种记录方式：记一次完成的休息。
        EventRepo::append(&state.db, BehaviorKind::BreakCompleted, "{}", start).expect("记录休息");

        let mut fired_at: Option<u32> = None;

        for minute in 1..=120u32 {
            let now = start.saturating_add_millis(minute as i64 * MINUTE);
            if matches!(state.tick(now).expect("tick"), TickOutcome::Intervene(_)) {
                fired_at = Some(minute);
                break;
            }

            // 顺带确认它没有提前开口
            let score = state.needs(now).map(|n| n.rest.get()).unwrap_or(0.0);
            assert!(
                score < 1.0,
                "第 {minute} 分钟休息需求就已经封顶了（{score}），\
                 但触发线是 1.0 —— 它不该在到点前提醒"
            );
        }

        assert_eq!(
            fired_at,
            Some(45),
            "设了 45 分钟休息间隔，就必须在第 45 分钟提醒。\
             实际在第 {fired_at:?} 分钟 —— 这正是用户报的那个问题"
        );
    }

    /// 回归测试：用户点「3 分钟后」，3 分钟后必须**真的**再提醒一次。
    ///
    /// ## 这个 bug 长什么样
    ///
    /// 用户按了「3 分钟后」，然后就没有然后了。
    ///
    /// 两层原因叠在一起：`interventions` 表里那条 `snoozed` 记录
    /// 仍被当成「最近一次打扰」，而冷却期从**它的原始时刻**起算
    /// （设 45 分钟 → 冷却 45 分钟）。于是延后窗口（3 分钟）
    /// 整个落在冷却期里面，延后到点时又被冷却拦下。
    ///
    /// 现在 `last_disturbing` 排除了被延后的记录，
    /// 静默期只由 `snooze_until` 一道闸门负责。
    ///
    /// ## 为什么要走完整的 tick 链路
    ///
    /// 「延后 → 到点 → 重新开口」要同时穿过状态机的 `snooze_until`、
    /// 仓库层的记录过滤、决策引擎的冷却判断。单独测任何一处
    /// 都测不出这个 bug —— 它恰恰是三层叠加的产物。
    #[test]
    fn 点了几分钟后就真的会在几分钟后再提醒() {
        use tacet_core::model::BehaviorKind;

        let start = afternoon();
        let mut state = state_with_rest_interval(start, 45);

        // 记录一次完成的休息，让需求有计时起点
        EventRepo::append(&state.db, BehaviorKind::BreakCompleted, "{}", start).expect("记录休息");

        // 跑到第一次提醒
        let mut snoozed_at = None;
        for minute in 1..=60u32 {
            let now = start.saturating_add_millis(minute as i64 * MINUTE);
            if matches!(state.tick(now).expect("tick"), TickOutcome::Intervene(_)) {
                snoozed_at = Some(now);
                break;
            }
        }
        let snoozed_at = snoozed_at.expect("第 45 分钟应当有一次提醒");

        // 用户点「3 分钟后」
        state.snooze(3, snoozed_at).expect("延后");

        // 延后期间必须安静
        let during = snoozed_at.saturating_add_millis(MINUTE);
        assert!(
            matches!(state.tick(during).expect("tick"), TickOutcome::Quiet),
            "延后期间不该打扰 —— 用户刚说了「等会儿」"
        );

        // 延后到点：必须重新开口
        let after = snoozed_at.saturating_add_millis(3 * MINUTE + 10_000);
        assert!(
            matches!(state.tick(after).expect("tick"), TickOutcome::Intervene(_)),
            "用户点了「3 分钟后」，3 分钟后就**必须**再提醒一次。\
             这是「延后」这个承诺的全部意义 —— 原来它被冷却期拦住了，\
             用户按完之后就再也等不到提醒"
        );
    }

    /// 非休息类的「稍后」不该污染休息统计。
    ///
    /// 早上十点弹出「该喝水了」，用户按「3 分钟后」——
    /// 那不是「休息被延后」。记进去会让今日的休息统计虚高。
    #[test]
    fn 非休息类提醒的延后不写进休息统计() {
        use tacet_core::model::{InterventionLevel, NeedKind, Reason};
        use tacet_storage::repo::EventRepo;

        let now = afternoon();
        let mut state = state();

        // 造一个「刚提醒过喝水」的现场
        state.last_decision = Some(InterventionDecision {
            kind: NeedKind::Hydration,
            level: InterventionLevel::FullScreen,
            reasons: vec![Reason::SinceLastHydration { minutes: 45 }],
            actions: Vec::new(),
            fused: Vec::new(),
        });

        state.snooze(3, now).expect("延后");

        let snoozes = EventRepo::count_in_window(
            &state.db,
            BehaviorKind::BreakSnoozed,
            &tacet_storage::DateWindow::day_of(now, state.offset),
        )
        .expect("统计");

        assert_eq!(snoozes, 0, "喝水提醒的「稍后」不该被记成一次「休息被延后」");
    }
}
