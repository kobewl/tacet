//! 工作状态机 —— 回答「现在算不算在工作」「这一口气连着干了多久」。
//!
//! ## 为什么这块逻辑值得单独写一个模块
//!
//! 验收标准里有两条专门盯着它：
//!
//! > ☐ 空闲超过 5 分钟后停止累计工作计时，返回后恢复
//! > ☐ Mac 休眠唤醒后计时状态正确（不出现"睡了 8 小时算 8 小时工作"）
//!
//! 这两条决定了整个产品可不可信。一个健康工具如果把「午休的两小时」记成
//! 「连续工作两小时」，然后弹窗说「你已经连续工作太久了」—— 用户会立刻卸载它。
//!
//! ## 四个状态
//!
//! ```text
//!             观测到活动                   空闲 ≥ 阈值
//!   Idle ──────────────► Working ──────────────► Away
//!     ▲                    │  ▲                   │
//!     │                    │  └───────────────────┘
//!     │              开始休息│      观测到活动
//!     │                    ▼
//!     └──────────────  Breaking
//!        休息结束
//! ```
//!
//! - [`WorkState::Idle`] —— 应用刚起来，还没看到人的动静
//! - [`WorkState::Working`] —— 在干活，累计计时中
//! - [`WorkState::Away`] —— 人离开了（空闲超过阈值），**不累计**
//! - [`WorkState::Breaking`] —— 正在休息，**不累计**，且不会因为人离开而改变状态
//!
//! ## 两条防呆设计
//!
//! 1. **累加而不是求差**：内部记的是「已经累计了多少毫秒」，每次更新加上一段增量，
//!    而不是记「开始时间」然后拿现在去减。这样即使系统时钟被校准跳过几秒，
//!    也不会凭空多出几个小时。
//! 2. **大跳步封顶**：如果两次更新之间的间隔超过 [`WorkClock::MAX_STEP_MS`]，
//!    说明进程被挂起过（系统休眠、资源紧张），超出的部分不计入工作。
//!
//!    注意是**封顶**而不是**丢弃**：后者看起来更保守，但会让计时器
//!    系统性少算 —— 只要系统稍微节流一下，整段增量就被吃掉，
//!    连续工作三小时可能只记下十分钟。宁可少算一点，也不要有系统性偏差。

use serde::{Deserialize, Serialize};

use crate::time::{Timestamp, MINUTE};

/// 工作状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkState {
    /// 还没有观测到用户活动（应用刚启动时就是这里）。
    Idle,
    /// 正在工作，计时累计中。
    Working,
    /// 正在休息。
    Breaking,
    /// 人离开了（空闲超过阈值），计时暂停。
    Away,
}

impl WorkState {
    /// 是否正在累计工作计时。
    pub const fn is_working(self) -> bool {
        matches!(self, WorkState::Working)
    }

    /// 界面显示名。
    pub const fn display_name(self) -> &'static str {
        match self {
            WorkState::Idle => "空闲",
            WorkState::Working => "工作中",
            WorkState::Breaking => "休息中",
            WorkState::Away => "离开了",
        }
    }

    /// 稳定字符串（落库、事件载荷、日志用）。
    ///
    /// 与 [`WorkState::display_name`] 的分工要分清楚：显示名是**给人看的**，
    /// 随时可以改文案；这个字符串是**给机器看的**，一旦写进数据库就不能再改，
    /// 否则已有的历史数据会对不上号。
    pub const fn as_str(self) -> &'static str {
        match self {
            WorkState::Idle => "idle",
            WorkState::Working => "working",
            WorkState::Breaking => "breaking",
            WorkState::Away => "away",
        }
    }
}

/// 送给状态机的观测或指令。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkInput {
    /// 周期性更新，附带**平台层观测到的空闲秒数**。
    ///
    /// 为什么让它带原始观测值而不是「用户动了 / 用户没动」的布尔量：
    /// 阈值是可配置的（默认 5 分钟），如果由平台层判断，改配置就得改平台层。
    /// 让状态机拿着原始数据自己判断，配置的归属就清晰了。
    Observe { idle_seconds: u32 },
    /// 系统即将休眠 / 锁屏。
    Sleep,
    /// 系统唤醒 / 解锁。
    Wake,
    /// 用户开始休息。
    StartBreak,
    /// 休息结束。
    EndBreak,
}

/// 一次状态变化。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StateChange {
    /// 变化前的状态。
    pub from: WorkState,
    /// 变化后的状态。
    pub to: WorkState,
    /// 变化发生的时刻。
    pub at: Timestamp,
}

/// 状态机的对外快照，供决策引擎与界面读取。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkClockSnapshot {
    /// 当前状态。
    pub state: WorkState,
    /// 这一口气连续工作了多少毫秒（离开或休息会清零）。
    pub continuous_work_ms: i64,
    /// 当前这段工作的起始时刻；不在工作中时为 `None`。
    pub segment_started_at: Option<Timestamp>,
    /// 已经多久没有输入了（毫秒）。
    pub idle_ms: i64,
}

impl WorkClockSnapshot {
    /// 连续工作了多少分钟（向下取整）。
    pub const fn continuous_work_minutes(&self) -> u32 {
        (self.continuous_work_ms / MINUTE) as u32
    }
}

/// 连续工作计时器。
#[derive(Debug, Clone)]
pub struct WorkClock {
    state: WorkState,
    /// 上一次更新时的时刻（用于算增量）。
    last_tick: Timestamp,
    /// 当前工作段的起始时刻。
    segment_started_at: Option<Timestamp>,
    /// 当前工作段已累计的毫秒数。
    accumulated_ms: i64,
    /// 最近一次观测到用户活动的时刻（按观测值推算）。
    last_activity_at: Timestamp,
    /// 空闲多久算离开（毫秒）。
    idle_threshold_ms: i64,
    /// 系统是否处于休眠中。
    suspended: bool,
}

impl WorkClock {
    /// 单次更新最多能把多少时间计入工作。
    ///
    /// ## 为什么需要这个上限
    ///
    /// 两次更新之间的间隔如果很大，说明进程没能按时被唤醒：系统休眠、内存压力下被
    /// 冻结，或者最常见的 —— **macOS 的 App Nap 把后台应用降频了**。
    /// 这段时间里用户在不在工作，我们其实不知道。
    ///
    /// ## 为什么是 60 秒
    ///
    /// 正常情况下轮询是 10 秒一次（架构文档 §8），60 秒已经能容忍
    /// 被 App Nap 节流到十几秒甚至半分钟才跑一次的最坏情况。
    /// 再往上放就会让「睡了三分钟但没收到休眠事件」这种场景多算工时。
    ///
    /// ## 为什么是「封顶」而不是「清零」
    ///
    /// 早期版本在这里直接丢弃整段增量，结果是：只要系统稍微节流一下，
    /// 工时就被整段吃掉，计时器会**系统性少算** —— 用户干了三小时，
    /// 它以为只干了两小时，于是提醒永远来得太晚。这比多算还糟，
    /// 因为它悄无声息。
    ///
    /// 真正的休眠由两道保险兜底：显式的 [`WorkInput::Sleep`] 事件，
    /// 以及休眠后必然出现的巨大 `idle_seconds`（人不可能在睡眠期间敲键盘）。
    /// 所以这里只需要把「来源不明的那一段」截断，不必整段作废。
    pub const MAX_STEP_MS: i64 = 60_000;

    /// 新建一个计时器。
    ///
    /// 初始状态是 [`WorkState::Idle`]：还不知道用户在不在。
    /// 等到平台层观测到输入，才会进入工作状态。
    pub fn new(now: Timestamp, idle_threshold_ms: i64) -> Self {
        Self {
            state: WorkState::Idle,
            last_tick: now,
            segment_started_at: None,
            accumulated_ms: 0,
            last_activity_at: now,
            idle_threshold_ms,
            suspended: false,
        }
    }

    /// 当前状态。
    pub const fn state(&self) -> WorkState {
        self.state
    }

    /// 这一口气连续工作了多少毫秒。
    pub const fn continuous_work_ms(&self) -> i64 {
        self.accumulated_ms
    }

    /// 取一份完整快照。
    pub fn snapshot(&self, now: Timestamp) -> WorkClockSnapshot {
        WorkClockSnapshot {
            state: self.state,
            continuous_work_ms: self.accumulated_ms,
            segment_started_at: self.segment_started_at,
            idle_ms: self.idle_ms(now),
        }
    }

    /// 已经多久没有输入了。
    pub fn idle_ms(&self, now: Timestamp) -> i64 {
        now.millis_since(self.last_activity_at).max(0)
    }

    /// 处理一次输入，返回状态变化（没变化就是 `None`）。
    pub fn handle(&mut self, input: WorkInput, now: Timestamp) -> Option<StateChange> {
        match input {
            WorkInput::Observe { idle_seconds } => self.observe(idle_seconds, now),
            WorkInput::Sleep => {
                self.accrue(now);
                self.suspended = true;
                // 睡觉不算工作：这一口气就此打住，醒来重新开始计。
                self.transition(WorkState::Away, now)
            }
            WorkInput::Wake => {
                self.suspended = false;
                // 关键一行：把时间基准重置到现在，避免把睡眠的整段时间算进来。
                // （accrue 里的大跳步保护也拦得住，这是第二道保险。）
                self.last_tick = now;
                self.transition(WorkState::Idle, now)
            }
            WorkInput::StartBreak => {
                self.accrue(now);
                self.transition(WorkState::Breaking, now)
            }
            WorkInput::EndBreak => {
                self.accrue(now);
                // 休息结束就当作回到工作 —— 用户刚在休息界面点完按钮，
                // 一定是人在屏幕前的。这样不必等下一次输入观测，计时立刻恢复。
                self.last_activity_at = now;
                self.transition(WorkState::Working, now)
            }
        }
    }

    /// 处理一次周期性观测。
    fn observe(&mut self, idle_seconds: u32, now: Timestamp) -> Option<StateChange> {
        self.accrue(now);

        let idle_ms = idle_seconds as i64 * 1000;

        // 休息中不因为人离开而改变状态：去倒水、去窗边远眺本来就会离开电脑，
        // 这时候把状态切成「离开」反而会让「休息结束」的按钮逻辑变得复杂。
        if self.state == WorkState::Breaking {
            return None;
        }

        if idle_ms >= self.idle_threshold_ms {
            // 人走了：结束这一段连续工作。
            self.last_activity_at = now.saturating_sub_millis(idle_ms);
            self.transition(WorkState::Away, now)
        } else if self.suspended {
            // 休眠还没正式结束（Wake 事件没收到）：先不动，等明确的唤醒信号。
            None
        } else {
            // 人在：记录活动时刻，必要时开始新的一段工作。
            // 活动其实发生在 idle_ms 之前，所以段起点回推一点点，
            // 并把这期间的时间算作已工作时长 —— 用户盯着屏幕看代码
            // 虽然没敲键盘，但那确实是在工作。
            let activity_at = now.saturating_sub_millis(idle_ms);
            self.last_activity_at = activity_at;

            match self.state {
                WorkState::Idle | WorkState::Away => {
                    self.transition_to_working_with_backlog(activity_at, idle_ms, now)
                }
                _ => None,
            }
        }
    }

    /// 从「不在工作」切到工作，并把空档里已经在工作的时间补上。
    fn transition_to_working_with_backlog(
        &mut self,
        activity_at: Timestamp,
        backlog_ms: i64,
        now: Timestamp,
    ) -> Option<StateChange> {
        let from = self.state;
        self.state = WorkState::Working;
        self.segment_started_at = Some(activity_at);
        self.accumulated_ms = backlog_ms.clamp(0, Self::MAX_STEP_MS);

        if from == WorkState::Working {
            None
        } else {
            Some(StateChange {
                from,
                to: WorkState::Working,
                at: now,
            })
        }
    }

    /// 把从上次更新到现在的时间累加进去。
    fn accrue(&mut self, now: Timestamp) {
        let delta = now.millis_since(self.last_tick);
        self.last_tick = now;

        // 时钟回拨或同一时刻重复更新：没有增量可加。
        if delta <= 0 {
            return;
        }

        if self.state == WorkState::Working {
            // 来源不明的大间隔只计入上限内的部分，见 MAX_STEP_MS 的说明。
            self.accumulated_ms += delta.min(Self::MAX_STEP_MS);
        }
    }

    /// 状态切换，并维护工作段的生命周期。
    fn transition(&mut self, to: WorkState, now: Timestamp) -> Option<StateChange> {
        let from = self.state;
        if from == to {
            return None;
        }

        self.state = to;

        match to {
            WorkState::Working => {
                self.segment_started_at = Some(now);
                self.accumulated_ms = 0;
            }
            // 离开、休息、待机都会结束当前这一段连续工作。
            // 这正是「连续工作 = 中间无 ≥ 5 分钟空闲中断」这条定义的落点。
            WorkState::Idle | WorkState::Away | WorkState::Breaking => {
                self.segment_started_at = None;
                self.accumulated_ms = 0;
            }
        }

        Some(StateChange { from, to, at: now })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::MINUTE;

    fn t0() -> Timestamp {
        Timestamp::from_millis(1_700_000_000_000)
    }

    /// 默认 5 分钟空闲阈值的计时器。
    fn clock() -> WorkClock {
        WorkClock::new(t0(), 5 * MINUTE)
    }

    #[test]
    fn 初始状态是待机且不计时() {
        let c = clock();
        assert_eq!(c.state(), WorkState::Idle);
        assert_eq!(c.continuous_work_ms(), 0);
    }

    #[test]
    fn 观测到活动后开始计时() {
        let mut c = clock();
        let mut now = t0();

        let change = c.handle(WorkInput::Observe { idle_seconds: 0 }, now);
        assert_eq!(
            change,
            Some(StateChange {
                from: WorkState::Idle,
                to: WorkState::Working,
                at: now
            })
        );

        // 走过 10 分钟
        for _ in 0..60 {
            now = now.saturating_add_millis(10_000);
            c.handle(WorkInput::Observe { idle_seconds: 0 }, now);
        }

        assert_eq!(c.state(), WorkState::Working);
        assert!(
            (c.continuous_work_ms() - 10 * MINUTE).abs() <= 1,
            "应累计约 10 分钟，实际 {} ms",
            c.continuous_work_ms()
        );
    }

    #[test]
    fn 空闲超过阈值后暂停累计并可以在回来时恢复() {
        let mut c = clock();
        let mut now = t0();

        // 工作 20 分钟
        c.handle(WorkInput::Observe { idle_seconds: 0 }, now);
        for _ in 0..120 {
            now = now.saturating_add_millis(10_000);
            c.handle(WorkInput::Observe { idle_seconds: 0 }, now);
        }
        let before_leave = c.continuous_work_ms();
        assert!((before_leave - 20 * MINUTE).abs() <= 1);

        // 人走了 6 分钟（超过 5 分钟阈值）
        for _ in 0..36 {
            now = now.saturating_add_millis(10_000);
            c.handle(WorkInput::Observe { idle_seconds: 360 }, now);
        }
        assert_eq!(c.state(), WorkState::Away, "空闲超过阈值应判定离开");
        assert_eq!(c.continuous_work_ms(), 0, "离开应结束当前连续工作段");

        // 人回来了
        let change = c.handle(WorkInput::Observe { idle_seconds: 0 }, now);
        assert_eq!(change.map(|c| c.to), Some(WorkState::Working));
        assert_eq!(c.state(), WorkState::Working);
        assert!(
            c.continuous_work_ms() < MINUTE,
            "回来之后应重新开始计，而不是接着之前的 20 分钟"
        );
    }

    #[test]
    fn 短暂的空闲不打断连续工作() {
        // 「连续工作定义：中间无 ≥ 5 分钟的空闲中断」—— 4 分钟的空闲不该打断。
        let mut c = clock();
        let mut now = t0();
        c.handle(WorkInput::Observe { idle_seconds: 0 }, now);

        // 工作 10 分钟
        for _ in 0..60 {
            now = now.saturating_add_millis(10_000);
            c.handle(WorkInput::Observe { idle_seconds: 0 }, now);
        }

        // 盯着屏幕看了 4 分钟没动键盘（idle 240 秒 < 300 秒阈值）
        for _ in 0..24 {
            now = now.saturating_add_millis(10_000);
            c.handle(WorkInput::Observe { idle_seconds: 240 }, now);
        }

        assert_eq!(c.state(), WorkState::Working, "4 分钟空闲不该判定离开");
        assert!(
            (c.continuous_work_ms() - 14 * MINUTE).abs() <= 1,
            "空闲期间仍应累计（人在看屏幕也是工作），实际 {} ms",
            c.continuous_work_ms()
        );
    }

    #[test]
    fn 系统休眠不把睡眠时长算成工作() {
        // 这是验收标准点名的那条：不能出现「睡了 8 小时算 8 小时工作」。
        let mut c = clock();
        let mut now = t0();
        c.handle(WorkInput::Observe { idle_seconds: 0 }, now);

        for _ in 0..180 {
            now = now.saturating_add_millis(10_000);
            c.handle(WorkInput::Observe { idle_seconds: 0 }, now);
        }
        assert!((c.continuous_work_ms() - 30 * MINUTE).abs() <= 1);

        // 合盖睡觉 8 小时
        c.handle(WorkInput::Sleep, now);
        assert_eq!(c.state(), WorkState::Away);
        assert_eq!(c.continuous_work_ms(), 0);

        now = now.saturating_add_millis(8 * 60 * MINUTE);
        c.handle(WorkInput::Wake, now);
        assert_eq!(c.state(), WorkState::Idle);
        assert_eq!(c.continuous_work_ms(), 0, "睡眠 8 小时不应计入工作");

        // 唤醒后有输入，重新开始计
        now = now.saturating_add_millis(5_000);
        c.handle(WorkInput::Observe { idle_seconds: 0 }, now);
        assert_eq!(c.state(), WorkState::Working);
        assert!(c.continuous_work_ms() < MINUTE);
    }

    #[test]
    fn 进程被冻结的大间隔只按上限计入() {
        // 没有收到 Sleep 事件，但进程被系统冻结了 2 小时（App Nap / 内存压力 / 断点调试）。
        let mut c = clock();
        let mut now = t0();
        c.handle(WorkInput::Observe { idle_seconds: 0 }, now);

        for _ in 0..30 {
            now = now.saturating_add_millis(10_000);
            c.handle(WorkInput::Observe { idle_seconds: 0 }, now);
        }
        let before = c.continuous_work_ms();

        // 一觉醒来 2 小时过去了，期间没有任何 tick
        now = now.saturating_add_millis(2 * 60 * MINUTE);
        c.handle(WorkInput::Observe { idle_seconds: 0 }, now);

        let credited = c.continuous_work_ms() - before;
        assert!(
            credited <= WorkClock::MAX_STEP_MS,
            "来源不明的大间隔最多只能计入上限，实际计入了 {credited} ms"
        );
        assert!(credited < 2 * MINUTE, "绝不能把冻结的两小时当成工作");
    }

    #[test]
    fn 系统休眠后计时不会被多算() {
        // 即便没收到 Sleep 事件，休眠期间不可能有输入，
        // 所以醒来时的 idle_seconds 一定很大 —— 这条保险必须生效。
        let mut c = clock();
        let mut now = t0();
        c.handle(WorkInput::Observe { idle_seconds: 0 }, now);

        for _ in 0..180 {
            now = now.saturating_add_millis(10_000);
            c.handle(WorkInput::Observe { idle_seconds: 0 }, now);
        }

        // 睡了 8 小时，期间没有 Sleep 事件，醒来第一次观测带上真实空闲时长
        now = now.saturating_add_millis(8 * 60 * MINUTE);
        let change = c.handle(
            WorkInput::Observe {
                idle_seconds: 28_800,
            },
            now,
        );

        assert_eq!(
            change.map(|c| c.to),
            Some(WorkState::Away),
            "巨量空闲应当被判定为离开"
        );
        assert_eq!(c.continuous_work_ms(), 0, "睡眠 8 小时不应计入连续工作");
    }

    #[test]
    fn 被节流几十秒不会丢掉工时() {
        // macOS 的 App Nap 会把后台应用的定时器降频到十几秒甚至更久。
        // 这类间隔必须照常计入，否则计时器会系统性少算，提醒永远来得太晚。
        let mut c = clock();
        let mut now = t0();
        c.handle(WorkInput::Observe { idle_seconds: 0 }, now);

        for _ in 0..20 {
            now = now.saturating_add_millis(25_000);
            c.handle(WorkInput::Observe { idle_seconds: 0 }, now);
        }

        assert!(
            (c.continuous_work_ms() - 8 * MINUTE - 20_000).abs() <= 1,
            "25 秒一次的观测应全额计入，实际 {} ms",
            c.continuous_work_ms()
        );
    }

    #[test]
    fn 休息期间人离开也不会切走状态() {
        // 去倒水、去窗边远眺本来就会离开电脑，休息状态不该被打断。
        let mut c = clock();
        let mut now = t0();
        c.handle(WorkInput::Observe { idle_seconds: 0 }, now);
        c.handle(WorkInput::StartBreak, now);
        assert_eq!(c.state(), WorkState::Breaking);

        for _ in 0..60 {
            now = now.saturating_add_millis(10_000);
            c.handle(WorkInput::Observe { idle_seconds: 600 }, now);
        }

        assert_eq!(c.state(), WorkState::Breaking, "休息中不应因空闲切状态");
    }

    #[test]
    fn 开始休息会结束当前工作段() {
        let mut c = clock();
        let mut now = t0();
        c.handle(WorkInput::Observe { idle_seconds: 0 }, now);
        for _ in 0..300 {
            now = now.saturating_add_millis(10_000);
            c.handle(WorkInput::Observe { idle_seconds: 0 }, now);
        }
        assert!(c.continuous_work_ms() > 40 * MINUTE);

        c.handle(WorkInput::StartBreak, now);
        assert_eq!(c.state(), WorkState::Breaking);
        assert_eq!(c.continuous_work_ms(), 0, "休息开始应清零连续工作时长");
        assert!(c.snapshot(now).segment_started_at.is_none());
    }

    #[test]
    fn 休息结束后立刻恢复计时() {
        let mut c = clock();
        let now = t0();
        c.handle(WorkInput::Observe { idle_seconds: 0 }, now);
        c.handle(WorkInput::StartBreak, now);

        let change = c.handle(WorkInput::EndBreak, now.saturating_add_millis(5 * MINUTE));

        assert_eq!(change.map(|c| c.to), Some(WorkState::Working));
        assert_eq!(c.state(), WorkState::Working);
        assert_eq!(c.continuous_work_ms(), 0, "休息后重新起算");
    }

    #[test]
    fn 重复的休眠或唤醒不会产生多余状态变化() {
        let mut c = clock();
        let now = t0();

        assert!(
            c.handle(WorkInput::Sleep, now).is_some(),
            "首次睡眠应切换状态"
        );
        assert!(c.handle(WorkInput::Sleep, now).is_none(), "重复睡眠无变化");
        assert!(
            c.handle(WorkInput::Wake, now).is_some(),
            "首次唤醒应切换状态"
        );
        assert!(c.handle(WorkInput::Wake, now).is_none(), "重复唤醒无变化");
    }

    #[test]
    fn 时钟回拨不会让计时变成负数() {
        let mut c = clock();
        let now = t0();
        c.handle(WorkInput::Observe { idle_seconds: 0 }, now);

        // 系统时间被往前校了 1 小时
        let backwards = now.saturating_sub_millis(60 * MINUTE);
        c.handle(WorkInput::Observe { idle_seconds: 0 }, backwards);

        assert!(c.continuous_work_ms() >= 0, "计时永远不应为负");
        assert!(c.idle_ms(backwards) >= 0, "空闲时长永远不应为负");
    }

    #[test]
    fn 快照里的连续工作分钟数() {
        let mut c = clock();
        let mut now = t0();
        c.handle(WorkInput::Observe { idle_seconds: 0 }, now);
        for _ in 0..78 {
            now = now.saturating_add_millis(MINUTE);
            c.handle(WorkInput::Observe { idle_seconds: 0 }, now);
        }

        let snapshot = c.snapshot(now);
        assert_eq!(snapshot.state, WorkState::Working);
        assert_eq!(snapshot.continuous_work_minutes(), 78);
        assert!(snapshot.segment_started_at.is_some());
    }

    #[test]
    fn 空闲阈值可配置() {
        // 用户把阈值调成 1 分钟
        let mut c = WorkClock::new(t0(), MINUTE);
        let mut now = t0();
        c.handle(WorkInput::Observe { idle_seconds: 0 }, now);

        now = now.saturating_add_millis(70_000);
        c.handle(WorkInput::Observe { idle_seconds: 70 }, now);

        assert_eq!(
            c.state(),
            WorkState::Away,
            "1 分钟阈值下 70 秒空闲应判定离开"
        );
    }
}
