//! 事件总线 —— 模块之间唯一的「广播通道」。
//!
//! ## 为什么用事件而不是直接调用
//!
//! 架构原则 4 说「模块间通信以事件为主，直接调用为辅」，理由是**可回放**：
//!
//! - 传感器发现用户切到了 VS Code → 发一个事件，它不知道谁在听
//! - 上下文引擎听到后更新快照 → 发一个事件
//! - 健康引擎听到后重算需求 → 发一个事件
//! - 决策引擎听到后决定要不要说话
//!
//! 这条链上每一环都只认识「事件」，不认识彼此。于是我们可以在测试里
//! 把一串录好的事件重新灌进去，看决策会不会复现 —— 出了 bug 能倒带重放，
//! 这在「为什么昨天下午三点它没提醒我」这类问题上是决定性的。
//!
//! ## 线程模型
//!
//! 总线是同步的、`Send + Sync` 的：`emit` 会**在当前线程**依次调用所有订阅者。
//! 这听起来不够「高级」，但对 v0.1 完全够用 —— 我们的所有事件都来自一个
//! 后台调度线程，同步派发没有调度延迟，也让调试时的栈回溯是可读的。
//!
//! 真正需要跨线程的地方（比如 UI 通知）由订阅者自己处理。

use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::model::{InterventionLevel, NeedKind};
use crate::Timestamp;

/// 事件是谁发出来的。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventSource {
    /// 平台层（传感器）。
    Platform,
    /// 上下文引擎。
    Context,
    /// 健康引擎。
    Health,
    /// 决策引擎。
    Policy,
    /// 存储层。
    Storage,
    /// 界面层（用户操作）。
    Ui,
    /// 系统壳层（休眠、唤醒、启动）。
    System,
}

impl EventSource {
    /// 稳定字符串。
    pub const fn as_str(self) -> &'static str {
        match self {
            EventSource::Platform => "platform",
            EventSource::Context => "context",
            EventSource::Health => "health",
            EventSource::Policy => "policy",
            EventSource::Storage => "storage",
            EventSource::Ui => "ui",
            EventSource::System => "system",
        }
    }
}

/// 事件的具体内容。
///
/// 与架构文档 §5.1 的事件表一一对应。新增事件时**必须**在这里加变体，
/// 不允许各模块自己发明事件类型（架构红线 4）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EventPayload {
    /// 前台应用切换了。只带 Bundle ID 与显示名，**不含窗口标题**。
    AppForegroundChanged {
        /// 应用唯一标识。
        bundle_id: String,
        /// 显示名。
        name: String,
    },
    /// 用户开始空闲。
    UserIdleStarted,
    /// 用户空闲结束。
    UserIdleEnded,
    /// 用户回来了（离开状态 → 工作状态）。
    UserReturned,
    /// 系统即将休眠 / 锁屏。
    SystemSleep,
    /// 系统唤醒 / 解锁。
    SystemWake,
    /// 全屏状态变化。
    ScreenFullscreenChanged {
        /// 是否进入全屏。
        fullscreen: bool,
    },
    /// 某类需求越过提醒阈值。
    NeedThresholdReached {
        /// 需求类型。
        kind: NeedKind,
        /// 当前强度。
        score: f64,
    },
    /// 提醒已经发出去。
    InterventionFired {
        /// 需求类型。
        kind: NeedKind,
        /// 使用的等级。
        level: InterventionLevel,
    },
    /// 用户开始了一次休息。
    BreakStarted,
    /// 用户完成了休息。
    BreakCompleted,
    /// 用户跳过了休息。
    BreakSkipped,
    /// 用户延后了休息。
    BreakSnoozed {
        /// 延后多少分钟。
        minutes: u32,
    },
    /// 记录了一次喝水。
    WaterLogged,
    /// 记录了一次活动。
    ActivityLogged,
    /// 记录了一次远眺。
    EyeRestLogged,
    /// Intent 已记录。
    IntentCaptured {
        /// 数据库主键。
        id: i64,
    },
    /// Intent 已恢复展示。
    IntentRestored {
        /// 数据库主键。
        id: i64,
    },
    /// 工作状态发生了变化（由状态机发出）。
    WorkStateChanged {
        /// 变化前的状态（字符串形式）。
        from: String,
        /// 变化后的状态。
        to: String,
    },
}

impl EventPayload {
    /// 事件的简短名字（日志用）。
    pub const fn name(&self) -> &'static str {
        match self {
            EventPayload::AppForegroundChanged { .. } => "app.foreground.changed",
            EventPayload::UserIdleStarted => "user.idle.started",
            EventPayload::UserIdleEnded => "user.idle.ended",
            EventPayload::UserReturned => "user.returned",
            EventPayload::SystemSleep => "system.sleep",
            EventPayload::SystemWake => "system.wake",
            EventPayload::ScreenFullscreenChanged { .. } => "screen.fullscreen.changed",
            EventPayload::NeedThresholdReached { .. } => "need.threshold.reached",
            EventPayload::InterventionFired { .. } => "intervention.fired",
            EventPayload::BreakStarted => "break.started",
            EventPayload::BreakCompleted => "break.completed",
            EventPayload::BreakSkipped => "break.skipped",
            EventPayload::BreakSnoozed { .. } => "break.snoozed",
            EventPayload::WaterLogged => "water.logged",
            EventPayload::ActivityLogged => "activity.logged",
            EventPayload::EyeRestLogged => "eye_rest.logged",
            EventPayload::IntentCaptured { .. } => "intent.captured",
            EventPayload::IntentRestored { .. } => "intent.restored",
            EventPayload::WorkStateChanged { .. } => "work.state.changed",
        }
    }

    /// 这条事件是否值得写进决策日志。
    ///
    /// 架构文档 §5.2：「关键事件（intervention.fired、用户操作）同时写入决策日志」。
    /// 状态机的高频事件（每 10 秒一次的状态观测）显然不该落库，
    /// 那会让数据库一年长到几百兆 —— 而预算是 50MB/年。
    pub const fn is_journalworthy(&self) -> bool {
        matches!(
            self,
            EventPayload::InterventionFired { .. }
                | EventPayload::BreakStarted
                | EventPayload::BreakCompleted
                | EventPayload::BreakSkipped
                | EventPayload::BreakSnoozed { .. }
                | EventPayload::WaterLogged
                | EventPayload::ActivityLogged
                | EventPayload::EyeRestLogged
                | EventPayload::IntentCaptured { .. }
                | EventPayload::IntentRestored { .. }
        )
    }
}

/// 一条事件。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Event {
    /// 发生时刻（UTC）。
    pub at: Timestamp,
    /// 由谁发出。
    pub source: EventSource,
    /// 发生了什么。
    pub payload: EventPayload,
}

impl Event {
    /// 构造一条事件。
    pub fn new(at: Timestamp, source: EventSource, payload: EventPayload) -> Self {
        Self {
            at,
            source,
            payload,
        }
    }

    /// 事件名（转发给 payload）。
    pub const fn name(&self) -> &'static str {
        self.payload.name()
    }
}

/// 订阅者的函数类型。
type Subscriber = Arc<dyn Fn(&Event) + Send + Sync>;

/// 一条「发送即忘」的事件总线。
///
/// ```rust
/// use std::sync::{Arc, Mutex};
/// use tacet_core::event::{Event, EventBus, EventPayload, EventSource};
/// use tacet_core::Timestamp;
///
/// let bus = EventBus::new();
/// let seen = Arc::new(Mutex::new(0usize));
///
/// let counter = Arc::clone(&seen);
/// bus.subscribe(move |_event| {
///     *counter.lock().expect("锁不应中毒") += 1;
/// });
///
/// bus.emit(Event::new(
///     Timestamp::from_millis(0),
///     EventSource::Ui,
///     EventPayload::WaterLogged,
/// ));
///
/// assert_eq!(*seen.lock().expect("锁不应中毒"), 1);
/// ```
#[derive(Default)]
pub struct EventBus {
    subscribers: Mutex<Vec<Subscriber>>,
}

impl EventBus {
    /// 新建一条空总线。
    pub fn new() -> Self {
        Self {
            subscribers: Mutex::new(Vec::new()),
        }
    }

    /// 订阅所有事件。
    pub fn subscribe<F>(&self, handler: F)
    where
        F: Fn(&Event) + Send + Sync + 'static,
    {
        let mut guard = self.lock();
        guard.push(Arc::new(handler));
    }

    /// 广播一条事件。
    ///
    /// ## 两个刻意的设计
    ///
    /// **一、先克隆订阅者列表再释放锁，然后才逐个调用。**
    /// 这是为了防死锁：如果某个订阅者在自己被调用时又 `emit` 了一条事件
    /// （「决策触发执行，执行又发事件」的链路里很常见），
    /// 持有锁去回调就会自己把自己卡住。克隆 `Arc` 很便宜，
    /// 拿这点开销换一个不会死锁的总线，值。
    ///
    /// **二、每个订阅者单独隔离 panic，最后再把 panic 重新抛出。**
    /// 一个订阅者崩了不该让同一次广播里排在它后面的订阅者收不到事件 ——
    /// 那会导致「界面上点了喝水，但数据库没记上」这类静默的数据丢失。
    /// 但我们也不静默吞掉 panic：那会把 bug 藏起来。
    /// 所以做法是「全部送达 → 重新抛出第一个 panic」，
    /// 调用方（壳层）能记录到异常，订阅者们也都收到了消息。
    pub fn emit(&self, event: Event) {
        let subscribers: Vec<Subscriber> = {
            let guard = self.lock();
            guard.clone()
        };

        let mut first_panic: Option<Box<dyn std::any::Any + Send>> = None;

        for subscriber in subscribers {
            // AssertUnwindSafe 在这里是安全的：订阅者拿到的是 `&Event`（不可变借用），
            // 我们不会跨 panic 边界去观察任何被它改动的状态。
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                subscriber(&event);
            }));

            if let Err(payload) = outcome {
                if first_panic.is_none() {
                    first_panic = Some(payload);
                }
            }
        }

        if let Some(payload) = first_panic {
            std::panic::resume_unwind(payload);
        }
    }

    /// 当前订阅者数量（测试与诊断用）。
    pub fn subscriber_count(&self) -> usize {
        self.lock().len()
    }

    /// 取锁；万一有订阅者 panic 导致锁中毒，也照常工作。
    ///
    /// 「中毒」（poisoning）是 Rust 标准库的保护机制：某个线程持锁时 panic 了，
    /// 后续取锁会返回 `Err`，提醒你数据可能不一致。
    /// 对一条事件总线来说，一个订阅者崩掉不该让整个程序再也发不出事件 ——
    /// 那会让「安静」变成「哑巴」。所以我们选择忽略中毒标记继续用。
    fn lock(&self) -> std::sync::MutexGuard<'_, Vec<Subscriber>> {
        match self.subscribers.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::MINUTE;

    fn now() -> Timestamp {
        Timestamp::from_millis(1_700_000_000_000)
    }

    #[test]
    fn 订阅者能收到事件() {
        let bus = EventBus::new();
        let captured: Arc<Mutex<Vec<&'static str>>> = Arc::new(Mutex::new(Vec::new()));

        let sink = Arc::clone(&captured);
        bus.subscribe(move |event| {
            if let Ok(mut v) = sink.lock() {
                v.push(event.name());
            }
        });

        bus.emit(Event::new(
            now(),
            EventSource::Ui,
            EventPayload::WaterLogged,
        ));
        bus.emit(Event::new(
            now(),
            EventSource::Ui,
            EventPayload::BreakCompleted,
        ));

        let names = captured.lock().expect("锁不应中毒");
        assert_eq!(*names, vec!["water.logged", "break.completed"]);
    }

    #[test]
    fn 多个订阅者都会收到同一条事件() {
        let bus = EventBus::new();
        let a = Arc::new(Mutex::new(0usize));
        let b = Arc::new(Mutex::new(0usize));

        let (a2, b2) = (Arc::clone(&a), Arc::clone(&b));
        bus.subscribe(move |_| {
            if let Ok(mut v) = a2.lock() {
                *v += 1;
            }
        });
        bus.subscribe(move |_| {
            if let Ok(mut v) = b2.lock() {
                *v += 1;
            }
        });

        bus.emit(Event::new(
            now(),
            EventSource::Health,
            EventPayload::UserIdleStarted,
        ));

        assert_eq!(*a.lock().expect("锁不应中毒"), 1);
        assert_eq!(*b.lock().expect("锁不应中毒"), 1);
        assert_eq!(bus.subscriber_count(), 2);
    }

    #[test]
    fn 订阅者内部再发事件不会死锁() {
        // 这是真实链路：决策发「提醒已发出」→ 存储订阅者收到后要写库、
        // 写库成功又发一条「已落盘」事件。
        let bus = Arc::new(EventBus::new());
        let nested = Arc::new(Mutex::new(0usize));

        let bus2 = Arc::clone(&bus);
        let nested2 = Arc::clone(&nested);
        bus.subscribe(move |event| {
            if event.name() == "intervention.fired" {
                // 在回调里再次广播
                bus2.emit(Event::new(
                    now(),
                    EventSource::Storage,
                    EventPayload::BreakCompleted,
                ));
            } else if let Ok(mut v) = nested2.lock() {
                *v += 1;
            }
        });

        bus.emit(Event::new(
            now(),
            EventSource::Policy,
            EventPayload::InterventionFired {
                kind: NeedKind::Rest,
                level: InterventionLevel::FullScreen,
            },
        ));

        assert_eq!(
            *nested.lock().expect("锁不应中毒"),
            1,
            "嵌套发出的事件也应该送达订阅者"
        );
    }

    #[test]
    fn 订阅者崩溃不影响同批其他订阅者() {
        let bus = EventBus::new();
        let delivered = Arc::new(Mutex::new(0usize));

        bus.subscribe(|_| panic!("这个订阅者故意崩掉"));
        let counter = Arc::clone(&delivered);
        bus.subscribe(move |_| {
            if let Ok(mut v) = counter.lock() {
                *v += 1;
            }
        });

        // panic 会向上传播给调用方（不静默吞掉），
        // 但排在崩溃订阅者**之后**的订阅者依然收到了事件 ——
        // 否则就会出现「用户点了喝水，界面更新了但数据库没记上」这类静默数据丢失。
        let first = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            bus.emit(Event::new(
                now(),
                EventSource::Ui,
                EventPayload::WaterLogged,
            ));
        }));
        assert!(first.is_err(), "panic 应当向上传播给调用方");
        assert_eq!(
            *delivered.lock().expect("锁不应中毒"),
            1,
            "崩溃不应阻断同一次广播里的其它订阅者"
        );

        // 那个坏订阅者还在，所以再次广播依然会抛 panic —— 但它照样不阻断别人。
        let second = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            bus.emit(Event::new(
                now(),
                EventSource::Ui,
                EventPayload::BreakCompleted,
            ));
        }));
        assert!(second.is_err());
        assert_eq!(
            *delivered.lock().expect("锁不应中毒"),
            2,
            "一次崩溃之后总线仍应继续派发"
        );
    }

    #[test]
    fn 没有订阅者时广播是空操作() {
        let bus = EventBus::new();
        assert_eq!(bus.subscriber_count(), 0);
        bus.emit(Event::new(
            now(),
            EventSource::System,
            EventPayload::SystemWake,
        ));
    }

    #[test]
    fn 只有关键事件值得落库() {
        assert!(EventPayload::WaterLogged.is_journalworthy());
        assert!(EventPayload::BreakSkipped.is_journalworthy());
        assert!(EventPayload::InterventionFired {
            kind: NeedKind::Rest,
            level: InterventionLevel::Notification
        }
        .is_journalworthy());

        // 高频的状态观测不该落库，否则一年能撑爆 50MB 的预算
        assert!(!EventPayload::WorkStateChanged {
            from: "idle".into(),
            to: "working".into()
        }
        .is_journalworthy());
        assert!(!EventPayload::UserIdleStarted.is_journalworthy());
    }

    #[test]
    fn 事件可序列化以支持回放() {
        let event = Event::new(
            now(),
            EventSource::Policy,
            EventPayload::BreakSnoozed { minutes: 3 },
        );

        let json = serde_json::to_string(&event).expect("序列化不应失败");
        let back: Event = serde_json::from_str(&json).expect("反序列化不应失败");

        assert_eq!(back, event);
        assert_eq!(back.at, now());
        assert_eq!(
            back.at.minutes_since(Timestamp::from_millis(0)),
            1_700_000_000_000 / MINUTE
        );
    }
}
