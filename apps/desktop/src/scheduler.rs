//! 后台调度 —— 应用的心跳。
//!
//! ## 为什么是 10 秒
//!
//! 架构文档 §8 的性能预算写得很明确：
//!
//! > 抓包轮询频率：空闲检测 10s / 麦克风 30s / 需求评估 10s
//!
//! 10 秒是「够及时」与「够省电」之间的平衡点：比它更密没有意义
//! （空闲阈值是分钟级的），更疏则会让「用户回来」的检测有可感知的延迟。
//!
//! ## 性能红线：这个循环必须很轻
//!
//! 一次 tick 做的事情是：几次系统 API 查询（都是纳秒级的只读调用）、
//! 几次内存里的算术、以及**大多数时候不写数据库**。
//! 空闲 CPU 预算 < 1%（架构文档 §8），这个量级完全够用。
//!
//! 需要警惕的是「每次 tick 都查数据库」。当前的实现里，`tick` 只在
//! **真的发出提醒时**才写库；读历史（`last_occurrence`）虽然每次都会
//! 查几条 SQL，但那些查询都走了索引，且数据量极小（一天几十行）。
//!
//! ## 关于 macOS 的 App Nap
//!
//! 后台应用会被系统降频，定时器可能从 10 秒变成十几秒甚至更久。
//! 这是**正常现象**，不是 bug —— 状态机的 `MAX_STEP_MS` 就是为它设计的
//! （见 `tacet-core::state` 的说明）。所以这里不需要对抗 App Nap。

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use tacet_core::model::InterventionLevel;
use tacet_core::time::Timestamp;
use tauri::{AppHandle, Emitter};

use crate::state::{AppState, TickOutcome};
use crate::windows;

/// 一次 tick 的间隔。
pub const TICK_INTERVAL: Duration = Duration::from_secs(10);

/// 启动后台调度线程。
///
/// 它会一直跑到进程结束 —— 没有「停止」接口，因为 Tacet 是一个
/// 随菜单栏常驻的应用，不存在「暂停后台」这种状态。
/// 用户想暂停用的是「暂停计时」，那是业务状态，不是线程状态。
pub fn spawn(app: AppHandle, state: Arc<Mutex<AppState>>) {
    thread::Builder::new()
        .name("tacet-scheduler".to_string())
        .spawn(move || run_loop(app, state))
        .expect("调度线程应当能启动");
}

/// 调度循环。
fn run_loop(app: AppHandle, state: Arc<Mutex<AppState>>) {
    loop {
        thread::sleep(TICK_INTERVAL);
        tick_once(&app, &state);
    }
}

/// 执行一次 tick 并把结果反映到界面与系统。
///
/// 这个函数被单独拆出来，是为了让「tick 之后应该发生什么」这件事
/// 可以被读清楚 —— 它是整个应用里唯一的「执行」入口。
fn tick_once(app: &AppHandle, state: &Arc<Mutex<AppState>>) {
    let now = Timestamp::now();

    let outcome = {
        let mut guard = AppState::lock(state);
        match guard.tick(now) {
            Ok(outcome) => outcome,
            Err(err) => {
                // tick 失败不该让线程退出 —— 那样应用会静默地不再计时，
                // 而用户完全不知道。记录后继续。
                crate::logging::error(&format!("tick 失败：{err}"));
                return;
            }
        }
    };

    match outcome {
        TickOutcome::Quiet => {}

        TickOutcome::Intervene(decision) => {
            // Level 4（全屏）：开 Overlay 窗口
            if decision.level == InterventionLevel::FullScreen {
                if let Err(err) = windows::show_break_window(app) {
                    crate::logging::error(&format!("打不开全屏提醒窗口：{err}"));
                }
            } else {
                // Level 2（通知）：发系统通知
                //
                // Level 3（浮卡）属于 v0.2，v0.1 不会产生这个等级。
                let (title, body) = crate::state::notification_text(&decision);
                if let Err(err) = windows::send_notification(app, &title, &body) {
                    crate::logging::warn(&format!("发通知失败：{err}"));
                }
            }
        }
    }

    // 无论这次做了什么，都推一份新快照给界面 ——
    // 面板上的「距提醒还有多久」需要持续更新。
    push_snapshot(app, state);

    // 兜底：主休息窗口不在时，副屏的幕布也不能留着。
    //
    // 放在这里而不是各个关闭路径里，是因为「正常路径」覆盖不了全部情况
    // （webview 崩溃、将来新增的关闭方式漏调）。详见
    // `windows::reconcile_break_windows` 的说明。
    windows::reconcile_break_windows(app);
}

/// 把当前状态推给所有界面窗口。
pub fn push_snapshot(app: &AppHandle, state: &Arc<Mutex<AppState>>) {
    let snapshot = {
        let guard = AppState::lock(state);
        match build_snapshot(&guard) {
            Some(snapshot) => snapshot,
            None => return,
        }
    };

    // 广播给所有窗口；没有窗口在听也不报错（这是常态：
    // 面板只在用户点开时存在）。
    let _ = app.emit(
        "tacet:event",
        serde_json::json!({ "type": "snapshot", "snapshot": snapshot }),
    );

    // 让托盘图标的标题跟着更新（Level 1 Ambient 的实现方式）
    update_tray_title(app, &snapshot);
}

/// 组装给界面的状态快照。
pub fn build_snapshot(state: &AppState) -> Option<serde_json::Value> {
    use tacet_core::model::BehaviorKind;
    use tacet_storage::repo::{EventRepo, IntentRepo, InterventionRepo};
    use tacet_storage::DateWindow;

    let now = Timestamp::now();
    let prefs = state.preferences().ok()?;
    let needs = state.needs(now).ok()?;

    // 距上次各类行为多久
    let minutes_since = |kind: BehaviorKind| -> Option<u32> {
        EventRepo::last_occurrence(&state.db, kind)
            .ok()
            .flatten()
            .map(|at| (now.millis_since(at) / tacet_core::time::MINUTE).max(0) as u32)
    };

    // 今日统计 —— 日期边界在这里算好（数据模型 §8：UI 禁止自行计算）
    let today = DateWindow::day_of(now, state.offset);
    let today_summary = build_today_summary(state, &today);

    let pending_intent = IntentRepo::latest_unrestored(&state.db)
        .ok()
        .flatten()
        .map(|intent| {
            serde_json::json!({
                "id": intent.id.unwrap_or(0),
                "text": intent.text,
                "createdAtMs": intent.created_at.as_millis(),
            })
        });

    let last_decision = state.last_decision.as_ref().map(|decision| {
        serde_json::json!({
            "kind": need_kind_str(decision.kind),
            "level": decision.level.as_i64(),
            "reasons": decision.reasons,
            "actions": decision.actions,
        })
    });

    // 平台能力报告
    let report = state.platform.capabilities();
    let capabilities: Vec<serde_json::Value> = tacet_platform::Capability::ALL
        .into_iter()
        .map(|capability| {
            let reason = report
                .unavailable
                .iter()
                .find(|(c, _)| *c == capability)
                .map(|(_, reason)| reason.clone());

            serde_json::json!({
                "name": capability.as_str(),
                "displayName": capability.display_name(),
                "available": report.supports(capability),
                "reason": reason,
            })
        })
        .collect();

    let _ = InterventionRepo::stats_in_window(&state.db, &today);

    Some(serde_json::json!({
        "state": state.work_state().as_str(),
        "continuousWorkMinutes": state.continuous_work_minutes(),
        "idleSeconds": state.clock.snapshot(now).idle_ms / 1000,
        "needs": {
            "rest": needs.rest.get(),
            "hydration": needs.hydration.get(),
            "movement": needs.movement.get(),
            "eyeRest": needs.eye_rest.get(),
        },
        "reminders": {
            "rest": rule_json(&prefs.reminders.rest),
            "hydration": rule_json(&prefs.reminders.hydration),
            "movement": rule_json(&prefs.reminders.movement),
            "eyeRest": rule_json(&prefs.reminders.eye_rest),
        },
        "lastWaterMinutesAgo": minutes_since(BehaviorKind::WaterLogged),
        "lastActivityMinutesAgo": minutes_since(BehaviorKind::ActivityLogged),
        "lastEyeRestMinutesAgo": minutes_since(BehaviorKind::EyeRestLogged),
        "today": today_summary,
        "doNotDisturb": prefs.do_not_disturb,
        "pendingIntent": pending_intent,
        "lastDecision": last_decision,
        "breakRemainingSeconds": state.break_remaining_seconds(now),
        "capabilities": capabilities,
    }))
}

/// 组装今日统计。
fn build_today_summary(state: &AppState, today: &tacet_storage::DateWindow) -> serde_json::Value {
    use tacet_core::model::BehaviorKind;
    use tacet_storage::repo::{EventRepo, InterventionRepo};

    let count = |kind: BehaviorKind| -> u32 {
        EventRepo::count_in_window(&state.db, kind, today).unwrap_or(0)
    };

    // 累计工作与时长的口径比较复杂（要扣除空闲段），属于 v0.3 统计页的工作。
    // v0.1 先用「最长的连续工作」与「今日事件数」给出可用的数字。
    let longest = compute_longest_streak(state, today);

    let stats = InterventionRepo::stats_in_window(&state.db, today).unwrap_or_default();

    serde_json::json!({
        "workMinutes": longest,
        "longestStreakMinutes": longest,
        "waterCount": count(BehaviorKind::WaterLogged),
        "activityCount": count(BehaviorKind::ActivityLogged),
        "breakCompletedCount": count(BehaviorKind::BreakCompleted),
        "breakSkippedCount": count(BehaviorKind::BreakSkipped),
        "breakSnoozedCount": count(BehaviorKind::BreakSnoozed),
        "acceptanceRate": stats.acceptance_rate(),
    })
}

/// 从今日事件里估出「最长连续工作」时长（分钟）。
///
/// ## 口径
///
/// 读今天的 `work.started` / `work.paused` 事件，把成对的区间长度算出来，
/// 取最长的一段。
///
/// ## 为什么是「估」
///
/// 严格的口径要扣除 Idle ≥ 5 分钟的时间（数据模型 §8），那需要记录
/// 每一次空闲开始/结束的完整序列。v0.1 的 `events` 表里只有
/// `work.started` / `work.paused` 这类节点事件，中间的空闲段没有落库。
///
/// 所以这里是**基于现有数据能做到的最好估计**。v0.3 的统计页会引入
/// 完整的区间口径。现在这个数字用于「今天大概干了多久」是够用的，
/// 而不会声称它精确。
fn compute_longest_streak(state: &AppState, today: &tacet_storage::DateWindow) -> u32 {
    use tacet_core::model::BehaviorKind;
    use tacet_storage::repo::EventRepo;

    let events = match EventRepo::in_window(&state.db, today) {
        Ok(events) => events,
        Err(_) => return 0,
    };

    let mut longest_ms: i64 = 0;
    let mut started_at: Option<Timestamp> = None;

    for event in events {
        match event.kind {
            BehaviorKind::WorkStarted => {
                started_at = Some(event.occurred_at);
            }
            BehaviorKind::WorkPaused | BehaviorKind::BreakStarted => {
                if let Some(start) = started_at.take() {
                    let span = event.occurred_at.millis_since(start).max(0);
                    longest_ms = longest_ms.max(span);
                }
            }
            _ => {}
        }
    }

    // 还在进行中的这一段也算上
    if let Some(start) = started_at {
        let span = Timestamp::now().millis_since(start).max(0);
        longest_ms = longest_ms.max(span);
    }

    (longest_ms / tacet_core::time::MINUTE) as u32
}

fn rule_json(rule: &tacet_core::model::ReminderRule) -> serde_json::Value {
    serde_json::json!({
        "enabled": rule.enabled,
        "intervalMinutes": rule.interval_minutes,
    })
}

fn need_kind_str(kind: tacet_core::model::NeedKind) -> &'static str {
    match kind {
        tacet_core::model::NeedKind::Rest => "rest",
        tacet_core::model::NeedKind::Hydration => "hydration",
        tacet_core::model::NeedKind::Movement => "movement",
        tacet_core::model::NeedKind::EyeRest => "eyeRest",
        tacet_core::model::NeedKind::Fused => "rest",
    }
}

/// 更新托盘图标上的文字（Level 1 Ambient）。
///
/// ## 这是「最轻的干预」
///
/// 菜单栏上显示「1h 32m」是产品里最克制的一种提醒：不发声、不弹窗、
/// 不抢焦点，但用户抬头就能看到自己坐了多久。
///
/// ## 什么时候显示
///
/// 只在工作状态下显示计时。空闲、休息中、暂停时都清空 ——
/// 屏幕上一直挂着一个数字会变成压力源，而那不是我们想要的。
fn update_tray_title(app: &AppHandle, snapshot: &serde_json::Value) {
    let Some(tray) = app.tray_by_id("main") else {
        return;
    };

    let state = snapshot
        .get("state")
        .and_then(|v| v.as_str())
        .unwrap_or("idle");
    let minutes = snapshot
        .get("continuousWorkMinutes")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    let title = if state == "working" && minutes >= 1 {
        if minutes >= 60 {
            format!("{}h {}m", minutes / 60, minutes % 60)
        } else {
            format!("{minutes}m")
        }
    } else {
        String::new()
    };

    let _ = tray.set_title(Some(title));
}
