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

        TickOutcome::BreakEnded => {
            // 状态已经结束，窗口必须跟着收。前端倒计时在休眠/App Nap
            // 之后可能冻住，不会自己走到 remaining === 0 那条关窗路径。
            windows::hide_break_window(app);
        }

        TickOutcome::Intervene(decision) => {
            // 整屏提醒是 v0.1.2 里唯一的打扰形态（四类需求都一样）。
            //
            // ## 为什么不再按等级分两条路
            //
            // 早期版本在这里分了岔：Level 4 开全屏窗口，其它等级发系统通知。
            // 但那条通知路径在真实环境里是**断的** —— 应用从未向 macOS
            // 申请过通知授权（桌面端插件的 permission API 恒返回 Granted，
            // 是个空操作），所以通知只进通知中心、不弹横幅。
            // 用户的实际体验是「设了 45 分钟，到点什么都没发生」。
            //
            // 决策层现在已经只产出 FullScreen（见 `PolicyEngine::choose_level`），
            // 所以这个 match 的其它分支是**防御性**的：万一将来新增了等级，
            // 也不会静默地什么都不做。
            match decision.level {
                InterventionLevel::FullScreen => {
                    if let Err(err) = windows::show_break_window(app) {
                        crate::logging::error(&format!("打不开整屏提醒窗口：{err}"));
                    }
                }

                // 兜底：理论上到不了这里。真到了就发系统通知 ——
                // 它虽然弹不出横幅，但至少在通知中心留得下一条记录，
                // 比完全静默要好。
                other => {
                    crate::logging::warn(&format!(
                        "出现了非整屏的打扰等级 {other:?}，改用系统通知兜底"
                    ));
                    let (title, body) = crate::state::notification_text(&decision);
                    if let Err(err) = windows::send_notification(app, &title, &body) {
                        crate::logging::warn(&format!("发通知失败：{err}"));
                    }
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
    let today_summary = build_today_summary(state, &today, now);
    let week_summary = build_week_summary(state, now);

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
        "week": week_summary,
        "doNotDisturb": prefs.do_not_disturb,
        "pendingIntent": pending_intent,
        "lastDecision": last_decision,
        "breakRemainingSeconds": state.break_remaining_seconds(now),
        // 休息的**总**时长，与上面那个「剩余」成对出现。
        // 界面用它当进度环的分母：剩余每秒在变，总时长整段不变，
        // 分母跟着剩余变正是「环每 10 秒倒退一次」的原因。
        "breakTotalSeconds": state.break_total_seconds(),
        "capabilities": capabilities,
    }))
}

/// 组装今日统计。
pub(crate) fn build_today_summary(
    state: &AppState,
    today: &tacet_storage::DateWindow,
    now: Timestamp,
) -> serde_json::Value {
    use tacet_core::model::BehaviorKind;
    use tacet_storage::repo::{EventRepo, InterventionRepo};

    let count = |kind: BehaviorKind| -> u32 {
        EventRepo::count_in_window(&state.db, kind, today).unwrap_or(0)
    };

    let work = compute_work_summary(state, today, now);

    let stats = InterventionRepo::stats_in_window(&state.db, today).unwrap_or_default();

    serde_json::json!({
        "workMinutes": work.total_minutes,
        "longestStreakMinutes": work.longest_minutes,
        "waterCount": count(BehaviorKind::WaterLogged),
        "activityCount": count(BehaviorKind::ActivityLogged),
        "breakCompletedCount": count(BehaviorKind::BreakCompleted),
        "breakSkippedCount": count(BehaviorKind::BreakSkipped),
        "breakSnoozedCount": count(BehaviorKind::BreakSnoozed),
        "acceptanceRate": stats.acceptance_rate(),
    })
}

/// 组装「最近 7 天」统计（含今天，旧 → 新）。
///
/// ## 为什么是「滚动 7 天」而不是「本周」
///
/// 周一早晨打开应用，「本周一到今天」几乎是空的 —— 用户看到的是一个
/// 没法解读的空图。滚动 7 天永远有内容，语义也更好解释：
/// 人们说「这周怎么样」时，想问的其实就是「最近这几天」。
///
/// ## 性能
///
/// 事件用**一次**区间查询拿全量、在内存里按天分桶 —— 而不是
/// 7 天 × 4 类 = 28 次查询。每天一次的 carried 判定与一次整周的接受率
/// 例外，合计约 9 次走索引的查询，快照级的调用频率下毫无压力。
pub(crate) fn build_week_summary(state: &AppState, now: Timestamp) -> serde_json::Value {
    use tacet_core::model::BehaviorKind;
    use tacet_storage::repo::{EventRepo, InterventionRepo};

    let offset = state.offset;
    let days: Vec<tacet_storage::DateWindow> = (0..7)
        .rev()
        .map(|back| {
            tacet_storage::DateWindow::day_of(
                now.saturating_sub_millis(back as i64 * 86_400_000),
                offset,
            )
        })
        .collect();

    let window = tacet_storage::DateWindow {
        start: days[0].start,
        end: days[6].end,
        day_index: days[0].day_index,
    };
    let events = EventRepo::in_window(&state.db, &window).unwrap_or_default();
    let working_now = state.clock.state() == tacet_core::state::WorkState::Working;

    let mut day_values = Vec::with_capacity(days.len());
    let mut total_work = 0u32;
    let mut total_water = 0u32;
    let mut total_activity = 0u32;
    let mut total_breaks = 0u32;

    for (index, day) in days.iter().enumerate() {
        // 跨午夜的工作段是否在继续：看这一天零点前最后一次工作状态变化
        let carried = matches!(
            EventRepo::last_work_boundary_before(&state.db, day.start),
            Ok(Some(BehaviorKind::WorkStarted))
        );
        let day_events: Vec<tacet_storage::repo::EventRow> = events
            .iter()
            .filter(|event| day.contains(event.occurred_at))
            .cloned()
            .collect();

        // 「进行中的工作续算到此刻」只对今天成立：历史日子没有「此刻」，
        // 未闭合的段在那里只会来自崩溃或强退，续算等于编造数据。
        let summary = work_summary_from_events(
            &day_events,
            day,
            now,
            carried,
            working_now && index == days.len() - 1,
            state.idle_threshold_ms,
        );

        let count = |kind: BehaviorKind| {
            day_events.iter().filter(|event| event.kind == kind).count() as u32
        };
        let water = count(BehaviorKind::WaterLogged);
        let activity = count(BehaviorKind::ActivityLogged);
        let breaks = count(BehaviorKind::BreakCompleted);
        total_work += summary.total_minutes;
        total_water += water;
        total_activity += activity;
        total_breaks += breaks;

        day_values.push(serde_json::json!({
            "date": day.format_date(),
            // 0 = 周一 … 6 = 周日（1970-01-01 是周四，+3 对齐到周一）
            "weekday": (day.day_index() + 3).rem_euclid(7),
            "isToday": day.is_today(now, offset),
            "workMinutes": summary.total_minutes,
            "longestStreakMinutes": summary.longest_minutes,
            "waterCount": water,
            "activityCount": activity,
            "breakCompletedCount": breaks,
        }));
    }

    let acceptance = InterventionRepo::stats_in_window(&state.db, &window)
        .ok()
        .and_then(|stats| stats.acceptance_rate());

    serde_json::json!({
        "days": day_values,
        "workMinutes": total_work,
        "waterCount": total_water,
        "activityCount": total_activity,
        "breakCompletedCount": total_breaks,
        "acceptanceRate": acceptance,
    })
}

#[derive(Debug, Default, PartialEq, Eq)]
struct WorkSummary {
    total_minutes: u32,
    longest_minutes: u32,
}

/// 累加当天每段工作，并把跨午夜的工作段裁到当天边界。
fn compute_work_summary(
    state: &AppState,
    today: &tacet_storage::DateWindow,
    now: Timestamp,
) -> WorkSummary {
    use tacet_core::model::BehaviorKind;
    use tacet_storage::repo::EventRepo;

    let events = match EventRepo::in_window(&state.db, today) {
        Ok(events) => events,
        Err(_) => return WorkSummary::default(),
    };
    let carried = matches!(
        EventRepo::last_work_boundary_before(&state.db, today.start),
        Ok(Some(BehaviorKind::WorkStarted))
    );
    work_summary_from_events(
        &events,
        today,
        now,
        carried,
        state.clock.state() == tacet_core::state::WorkState::Working,
        state.idle_threshold_ms,
    )
}

fn work_summary_from_events(
    events: &[tacet_storage::repo::EventRow],
    today: &tacet_storage::DateWindow,
    now: Timestamp,
    carried: bool,
    working_now: bool,
    merge_gap_ms: i64,
) -> WorkSummary {
    use tacet_core::model::BehaviorKind;

    let end = now.min(today.end);
    let mut total_ms: i64 = 0;
    let mut longest_ms: i64 = 0;
    // 当前连续块：一口气里的多段工作之和（段间空档不计入）。
    // 块结束（隔太久 / 显式休息 / 事件耗尽）时才把整块计入 longest。
    let mut block_ms: i64 = 0;
    let mut started_at = carried.then_some(today.start);
    // 最近一次工作收尾的时刻：判断下一段是否接在上一块后面用
    let mut last_stop: Option<Timestamp> = None;
    // 最近一条工作边界事件的时刻：识别历史遗留的连续 started 用
    let mut prev_boundary_at: Option<Timestamp> = None;

    for event in events {
        match event.kind {
            BehaviorKind::WorkStarted => {
                if started_at.is_some() {
                    // 正常流程里两条 started 之间必有 paused；连续出现
                    // 只会来自旧版本的「退出不收尾」。靠得近（≤ 阈值）
                    // 是重启衔接，保留更早的起点 —— 那几分钟空档用户
                    // 几乎肯定在干活，把整段丢掉才是大错；隔得远说明
                    // 中间是漫长的离开，用新起点（旧起点按丢弃处理）。
                    let replace = match prev_boundary_at {
                        // 只有跨午夜带来的起点、不是真实事件 → 用新起点
                        None => true,
                        Some(prev) => {
                            event.occurred_at.millis_since(prev).max(0) > merge_gap_ms
                        }
                    };
                    if replace {
                        started_at = Some(event.occurred_at);
                    }
                } else {
                    // 新的一段：看它是否接在上一块后面（≤ 阈值 = 同一口气，
                    // 块继续累计；否则上一块到此为止）
                    let continues = match last_stop {
                        Some(stop) => {
                            event.occurred_at.millis_since(stop).max(0) <= merge_gap_ms
                        }
                        None => false,
                    };
                    if !continues {
                        longest_ms = longest_ms.max(block_ms);
                        block_ms = 0;
                    }
                    started_at = Some(event.occurred_at);
                }
            }
            BehaviorKind::WorkPaused | BehaviorKind::BreakStarted => {
                if let Some(start) = started_at.take() {
                    let span = event
                        .occurred_at
                        .min(end)
                        .millis_since(start.max(today.start))
                        .max(0);
                    total_ms += span;
                    block_ms += span;
                }
                if event.kind == BehaviorKind::BreakStarted {
                    // 显式休息是用户主动打断：块到此为止，休息后的
                    // 工作不再与之前的段相接
                    longest_ms = longest_ms.max(block_ms);
                    block_ms = 0;
                    last_stop = None;
                } else {
                    last_stop = Some(event.occurred_at);
                }
            }
            _ => {}
        }
        if matches!(
            event.kind,
            BehaviorKind::WorkStarted
                | BehaviorKind::WorkPaused
                | BehaviorKind::BreakStarted
        ) {
            prev_boundary_at = Some(event.occurred_at);
        }
    }

    // 只有当前状态仍在工作，才把未闭合的一段计到此刻；崩溃遗留的开始事件不续算。
    if working_now {
        if let Some(start) = started_at {
            let span = end.millis_since(start.max(today.start)).max(0);
            total_ms += span;
            block_ms += span;
        }
    }
    longest_ms = longest_ms.max(block_ms);

    WorkSummary {
        total_minutes: (total_ms / tacet_core::time::MINUTE) as u32,
        longest_minutes: (longest_ms / tacet_core::time::MINUTE) as u32,
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use tacet_core::model::BehaviorKind;
    use tacet_platform::MockPlatform;
    use tacet_storage::repo::EventRow;
    use tacet_storage::{DateWindow, LocalOffset};

    fn state() -> AppState {
        AppState::in_memory(Box::new(MockPlatform::new())).expect("建立状态")
    }

    /// 测试用的「同一口气」界线：5 分钟（与默认空闲阈值一致）。
    const MERGE_GAP_MS: i64 = 5 * 60_000;

    fn event(kind: BehaviorKind, at: Timestamp) -> EventRow {
        EventRow {
            id: 0,
            kind,
            payload: "{}".to_string(),
            occurred_at: at,
            created_at: at,
        }
    }

    #[test]
    fn 今日累计工作会累加多段而最长连续只取一段() {
        let today = DateWindow::day_of(Timestamp::from_millis(0), LocalOffset::utc());
        let at = |minutes: i64| today.start.saturating_add_millis(minutes * 60_000);
        let events = vec![
            event(BehaviorKind::WorkStarted, at(10)),
            event(BehaviorKind::WorkPaused, at(30)),
            event(BehaviorKind::WorkStarted, at(40)),
            event(BehaviorKind::WorkPaused, at(70)),
        ];

        assert_eq!(
            work_summary_from_events(&events, &today, at(80), false, false, MERGE_GAP_MS),
            WorkSummary {
                total_minutes: 50,
                longest_minutes: 30,
            }
        );
    }

    #[test]
    fn 跨午夜工作段从当天零点计入并包含当前进行中的时间() {
        let today = DateWindow::day_of(Timestamp::from_millis(0), LocalOffset::utc());
        let at = |minutes: i64| today.start.saturating_add_millis(minutes * 60_000);
        let events = vec![
            event(BehaviorKind::WorkPaused, at(90)),
            event(BehaviorKind::WorkStarted, at(120)),
        ];

        assert_eq!(
            work_summary_from_events(&events, &today, at(140), true, true, MERGE_GAP_MS),
            WorkSummary {
                total_minutes: 110,
                longest_minutes: 90,
            }
        );
        assert_eq!(
            work_summary_from_events(&events, &today, at(140), true, false, MERGE_GAP_MS),
            WorkSummary {
                total_minutes: 90,
                longest_minutes: 90,
            },
            "应用重启后留下的未闭合记录不应一直算到现在"
        );
    }

    /// 重启衔接的两段（收尾到重新开始 ≤ 空闲阈值）在统计里是一口气：
    /// 累计和最长连续都把两段加起来，重启的空档不计入 ——
    /// 否则「今天」页会与面板上接回来的计时对不上。
    #[test]
    fn 重启衔接的两段合并成一块且空档不计入() {
        let today = DateWindow::day_of(Timestamp::from_millis(0), LocalOffset::utc());
        let at = |minutes: i64| today.start.saturating_add_millis(minutes * 60_000);
        // 工作 30 分钟 → 重启（空档 5 分钟，恰好压着界线）→ 再工作 35 分钟
        let events = vec![
            event(BehaviorKind::WorkStarted, at(10)),
            event(BehaviorKind::WorkPaused, at(40)),
            event(BehaviorKind::WorkStarted, at(45)),
            event(BehaviorKind::WorkPaused, at(80)),
        ];

        assert_eq!(
            work_summary_from_events(&events, &today, at(80), false, false, MERGE_GAP_MS),
            WorkSummary {
                total_minutes: 65,
                longest_minutes: 65,
            }
        );
    }

    /// 空档超过阈值就不是同一口气了：各算各的段，空档两头都不沾。
    #[test]
    fn 隔太久的两段不合并() {
        let today = DateWindow::day_of(Timestamp::from_millis(0), LocalOffset::utc());
        let at = |minutes: i64| today.start.saturating_add_millis(minutes * 60_000);
        let events = vec![
            event(BehaviorKind::WorkStarted, at(10)),
            event(BehaviorKind::WorkPaused, at(40)),
            event(BehaviorKind::WorkStarted, at(120)),
            event(BehaviorKind::WorkPaused, at(150)),
        ];

        assert_eq!(
            work_summary_from_events(&events, &today, at(150), false, false, MERGE_GAP_MS),
            WorkSummary {
                total_minutes: 60,
                longest_minutes: 30,
            }
        );
    }

    /// 显式休息是用户主动打断连续：休息前后即使贴得很近也不算同一口气。
    #[test]
    fn 休息把最长连续切成两块() {
        let today = DateWindow::day_of(Timestamp::from_millis(0), LocalOffset::utc());
        let at = |minutes: i64| today.start.saturating_add_millis(minutes * 60_000);
        let events = vec![
            event(BehaviorKind::WorkStarted, at(10)),
            event(BehaviorKind::WorkPaused, at(40)),
            event(BehaviorKind::BreakStarted, at(40)),
            event(BehaviorKind::WorkStarted, at(45)),
            event(BehaviorKind::WorkPaused, at(80)),
        ];

        assert_eq!(
            work_summary_from_events(&events, &today, at(80), false, false, MERGE_GAP_MS),
            WorkSummary {
                total_minutes: 65,
                longest_minutes: 35,
            }
        );
    }

    /// 旧版本退出不收尾，库里会留下连续两条 `work.started`。
    /// 靠得近的是重启衔接：保留更早的起点，把那段真实工作时间找回来，
    /// 而不是像 0.2.0 之前那样被第二条整个覆盖掉。
    #[test]
    fn 历史遗留的连续开始事件靠得近时保留早起点() {
        let today = DateWindow::day_of(Timestamp::from_millis(0), LocalOffset::utc());
        let at = |minutes: i64| today.start.saturating_add_millis(minutes * 60_000);
        // 工作 1 小时 → 应用重启（间隔 5 分钟）→ 旧版本不收尾又记了一条 started
        let events = vec![
            event(BehaviorKind::WorkStarted, at(10)),
            event(BehaviorKind::WorkStarted, at(15)),
            event(BehaviorKind::WorkPaused, at(80)),
        ];

        assert_eq!(
            work_summary_from_events(&events, &today, at(80), false, false, MERGE_GAP_MS),
            WorkSummary {
                total_minutes: 70,
                longest_minutes: 70,
            }
        );
    }

    /// 连续两条 `started` 隔得远：中间是漫长的离开，悬空的旧起点
    /// 只能丢弃（时长无从得知），从新起点算起 —— 与旧行为一致。
    #[test]
    fn 历史遗留的连续开始事件隔得远时用新起点() {
        let today = DateWindow::day_of(Timestamp::from_millis(0), LocalOffset::utc());
        let at = |minutes: i64| today.start.saturating_add_millis(minutes * 60_000);
        let events = vec![
            event(BehaviorKind::WorkStarted, at(10)),
            event(BehaviorKind::WorkStarted, at(300)),
            event(BehaviorKind::WorkPaused, at(320)),
        ];

        assert_eq!(
            work_summary_from_events(&events, &today, at(320), false, false, MERGE_GAP_MS),
            WorkSummary {
                total_minutes: 20,
                longest_minutes: 20,
            }
        );
    }

    /// 周统计的关键契约：事件按本地自然日分桶，各天之和等于汇总；
    /// 今天那格的未闭合工作段在时钟不在工作态时不续算（与「今日」口径一致）。
    #[test]
    fn 周统计按天分桶且汇总等于各天之和() {
        use tacet_storage::repo::EventRepo;

        let state = state();
        let now = Timestamp::now();
        let offset = state.offset;
        let today = DateWindow::day_of(now, offset);
        let yesterday = DateWindow::from_day_index(today.day_index() - 1, offset);
        let six_days_ago = DateWindow::from_day_index(today.day_index() - 6, offset);

        // 昨天：一段完整的工作（本地 9:00–9:30）和两次喝水
        let at = |day: &DateWindow, minutes: i64| {
            day.local_midnight().saturating_add_millis(minutes * 60_000)
        };
        EventRepo::append(
            &state.db,
            BehaviorKind::WorkStarted,
            "{}",
            at(&yesterday, 9 * 60),
        )
        .expect("写入");
        EventRepo::append(
            &state.db,
            BehaviorKind::WorkPaused,
            "{}",
            at(&yesterday, 9 * 60 + 30),
        )
        .expect("写入");
        EventRepo::append(
            &state.db,
            BehaviorKind::WaterLogged,
            "{}",
            at(&yesterday, 10 * 60),
        )
        .expect("写入");
        EventRepo::append(
            &state.db,
            BehaviorKind::WaterLogged,
            "{}",
            at(&yesterday, 11 * 60),
        )
        .expect("写入");

        // 6 天前：一次完成的休息
        EventRepo::append(
            &state.db,
            BehaviorKind::BreakCompleted,
            "{}",
            at(&six_days_ago, 60),
        )
        .expect("写入");

        // 今天：一段还没闭合的工作（时钟不在工作态 = 重启后遗留，不续算）
        EventRepo::append(&state.db, BehaviorKind::WorkStarted, "{}", at(&today, 60))
            .expect("写入");

        let week = build_week_summary(&state, now);
        let days = week["days"].as_array().expect("days 数组");

        assert_eq!(days.len(), 7);
        assert_eq!(days[6]["isToday"].as_bool(), Some(true));
        assert_eq!(
            days[0]["date"].as_str(),
            Some(six_days_ago.format_date().as_str())
        );

        assert_eq!(days[5]["workMinutes"].as_u64(), Some(30));
        assert_eq!(days[5]["waterCount"].as_u64(), Some(2));
        assert_eq!(days[5]["longestStreakMinutes"].as_u64(), Some(30));
        assert_eq!(days[0]["breakCompletedCount"].as_u64(), Some(1));
        assert_eq!(
            days[6]["workMinutes"].as_u64(),
            Some(0),
            "未闭合且不在工作态的段不应续算"
        );

        assert_eq!(week["workMinutes"].as_u64(), Some(30));
        assert_eq!(week["waterCount"].as_u64(), Some(2));
        assert_eq!(week["breakCompletedCount"].as_u64(), Some(1));
    }

    /// 回归测试：快照里的 `breakTotalSeconds` 必须真的送到界面上。
    ///
    /// ## 为什么需要这条
    ///
    /// 快照是用 `serde_json::json!` 手工拼的，键名是个**字符串字面量** ——
    /// 拼错了编译器一声不吭。而前端拿不到这个字段时的表现是「退化成
    /// 没有它」，正好退回到那个「环先慢后快」的旧行为上：
    /// 应用照常能跑、测试照常全绿、界面照常显示，只有环在悄悄地抖。
    ///
    /// 这类「静默退化」是最难发现的一种故障，所以键名必须由测试钉住。
    #[test]
    fn 快照里带着休息总时长() {
        let mut state = state();
        let now = Timestamp::now();

        // 不在休息中：字段存在，但是 null（界面据此知道没有休息在进行）。
        // 「字段存在」和「值为 null」要分开断言 —— 只断言 null 的话，
        // 键名拼错（整个键消失）也会让测试通过。
        let idle = build_snapshot(&state).expect("快照");
        assert!(
            idle.get("breakTotalSeconds").is_some(),
            "快照里必须有 breakTotalSeconds 这个键；\
             键名写错会让前端永远拿不到值，进度环退化成「先慢后快」"
        );
        assert_eq!(
            idle.get("breakTotalSeconds"),
            Some(&serde_json::Value::Null),
            "没在休息时 breakTotalSeconds 应当是 null"
        );

        state.start_break(now).expect("开始休息");

        let snapshot = build_snapshot(&state).expect("快照");
        assert_eq!(
            snapshot.get("breakTotalSeconds").and_then(|v| v.as_u64()),
            Some(300),
            "休息中 breakTotalSeconds 必须是这次休息的总时长（默认 300 秒）"
        );
    }

    /// 回归测试：休息进行中，快照里的**总时长与剩余是两个不同的数**。
    ///
    /// ## 这条测试守住的是什么
    ///
    /// 那个「环先慢后快」的 bug，根子上是前端把剩余秒数当成了分母 ——
    /// 而它能这么写，是因为当时快照里**只有**剩余秒数这一个数，
    /// 前端手上根本没有别的东西可用。
    ///
    /// 现在快照同时给出这两个数，它们必须满足：
    ///
    /// - **总时长恒等于设置值**（300）—— 它是进度环的分母，不能随时间变
    /// - **剩余随着时间减少** —— 它是分子，本来就该在走
    ///
    /// 所以这条测试要让时间真的过去一段。`build_snapshot` 内部用的是
    /// 真实时钟（它要算「距上次喝水多久」这类相对时间），没法用模拟时间
    /// 直接推 —— 于是换一个方向：把休息的**起点**设在 3 分钟前，
    /// 效果和「已经休息了 3 分钟」完全一样。
    #[test]
    fn 休息进行中总时长不变而剩余在减少() {
        use tacet_core::time::MINUTE;

        let mut state = state();
        let now = Timestamp::now();

        // 这次休息在 3 分钟前就开始了
        state
            .start_break(now.saturating_sub_millis(3 * MINUTE))
            .expect("开始休息");

        let snapshot = build_snapshot(&state).expect("快照");

        assert_eq!(
            snapshot.get("breakTotalSeconds").and_then(|v| v.as_u64()),
            Some(300),
            "已经休息了 3 分钟，总时长必须还是 300 —— \
             它一旦跟着剩余一起缩水，进度环就会每隔几秒倒退重画"
        );

        let remaining = snapshot
            .get("breakRemainingSeconds")
            .and_then(|v| v.as_u64())
            .expect("剩余时间");
        assert!(
            (117..=120).contains(&remaining),
            "休息了 3 分钟，剩余应当在 120 秒附近，实际 {remaining}"
        );

        // 两个数确实不一样了 —— 这正是旧代码出错的地方：
        // 那时前端手上只有 remaining，拿它当分母，比例被反复拉回 0。
        assert_ne!(
            snapshot.get("breakTotalSeconds"),
            snapshot.get("breakRemainingSeconds"),
            "总时长和剩余必须是两个不同的数，否则分母又会跟着分子一起变"
        );
    }
}
