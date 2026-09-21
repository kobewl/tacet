//! IPC 命令 —— 界面能对后端做的全部事情。
//!
//! ## 这一层的职责边界
//!
//! 每个命令都是「薄」的：加锁 → 调一个领域操作 → 拿回快照。
//! **没有任何业务判断**在这里 —— 那是 core / health 的事。
//!
//! ## 返回值约定
//!
//! 绝大多数命令返回最新的完整快照（`AppSnapshot`）。这样做的好处是
//! 界面**不需要自己推断状态变化**：点完「+1 杯水」，拿回来的就是
//! 更新后的四类需求、更新时间、更新后的今日统计。
//!
//! 代价是每次操作都要多序列化一点数据 —— 在一次本地 IPC 调用的量级上
//! 完全可以忽略，换来的是「界面永远不可能显示过期状态」这个保证。

use tauri::{AppHandle, Manager, State};

use crate::scheduler;
use crate::state::AppState;
use crate::windows::{self, SharedState};

/// 命令的统一错误类型（会被序列化后传给前端）。
#[derive(Debug, serde::Serialize)]
pub struct CommandError {
    /// 给用户看的简短说明。
    pub message: String,
}

impl From<String> for CommandError {
    fn from(message: String) -> Self {
        Self { message }
    }
}

impl From<tacet_core::CoreError> for CommandError {
    fn from(err: tacet_core::CoreError) -> Self {
        Self {
            message: err.to_string(),
        }
    }
}

impl From<crate::state::StateError> for CommandError {
    fn from(err: crate::state::StateError) -> Self {
        Self {
            message: err.to_string(),
        }
    }
}

impl From<tacet_storage::StorageError> for CommandError {
    fn from(err: tacet_storage::StorageError) -> Self {
        Self {
            message: err.to_string(),
        }
    }
}

impl From<tauri::Error> for CommandError {
    fn from(err: tauri::Error) -> Self {
        Self {
            message: err.to_string(),
        }
    }
}

pub type CmdResult<T> = Result<T, CommandError>;

/// 取当前完整状态快照。
#[tauri::command]
pub fn get_snapshot(state: State<'_, SharedState>) -> CmdResult<serde_json::Value> {
    let guard = AppState::lock(&state);
    scheduler::build_snapshot(&guard).ok_or_else(|| CommandError::from("读取状态失败".to_string()))
}

/// 取今日统计。
#[tauri::command]
pub fn get_today_summary(state: State<'_, SharedState>) -> CmdResult<serde_json::Value> {
    let guard = AppState::lock(&state);

    let now = tacet_core::Timestamp::now();
    let today = tacet_storage::DateWindow::day_of(now, guard.offset);

    use tacet_core::model::BehaviorKind;
    use tacet_storage::repo::{EventRepo, InterventionRepo};

    let count =
        |kind: BehaviorKind| EventRepo::count_in_window(&guard.db, kind, &today).unwrap_or(0);

    let stats = InterventionRepo::stats_in_window(&guard.db, &today).unwrap_or_default();

    Ok(serde_json::json!({
        "workMinutes": guard.continuous_work_minutes(),
        "longestStreakMinutes": guard.continuous_work_minutes(),
        "waterCount": count(BehaviorKind::WaterLogged),
        "activityCount": count(BehaviorKind::ActivityLogged),
        "breakCompletedCount": count(BehaviorKind::BreakCompleted),
        "breakSkippedCount": count(BehaviorKind::BreakSkipped),
        "breakSnoozedCount": count(BehaviorKind::BreakSnoozed),
        "acceptanceRate": stats.acceptance_rate(),
    }))
}

/// 取用户偏好。
#[tauri::command]
pub fn get_preferences(state: State<'_, SharedState>) -> CmdResult<serde_json::Value> {
    let guard = AppState::lock(&state);
    let prefs = guard.preferences()?;

    Ok(serde_json::json!({
        "reminders": {
            "rest": rule_json(&prefs.reminders.rest),
            "hydration": rule_json(&prefs.reminders.hydration),
            "movement": rule_json(&prefs.reminders.movement),
            "eyeRest": rule_json(&prefs.reminders.eye_rest),
        },
        "doNotDisturb": prefs.do_not_disturb,
        "idleThresholdMinutes": prefs.idle_threshold_minutes,
        "breakDurationMinutes": prefs.break_duration_minutes,
        "snoozeOptionsMinutes": prefs.snooze_options_minutes,
    }))
}

fn rule_json(rule: &tacet_core::model::ReminderRule) -> serde_json::Value {
    serde_json::json!({
        "enabled": rule.enabled,
        "intervalMinutes": rule.interval_minutes,
    })
}

/// 保存用户偏好。
#[tauri::command]
pub fn save_preferences(
    state: State<'_, SharedState>,
    preferences: serde_json::Value,
) -> CmdResult<()> {
    let mut prefs = {
        let guard = AppState::lock(&state);
        guard.preferences()?
    };

    // 逐字段读取，任何一项缺失都保留原值。
    //
    // 为什么不用 serde 直接反序列化整个对象：那样前端漏传一个字段
    // 就会导致整个保存失败，用户会看到「保存失败」而不知道少了什么。
    // 逐字段读取 + 保留原值则总是能保存成功。
    if let Some(reminders) = preferences.get("reminders") {
        read_rule(reminders.get("rest"), &mut prefs.reminders.rest);
        read_rule(reminders.get("hydration"), &mut prefs.reminders.hydration);
        read_rule(reminders.get("movement"), &mut prefs.reminders.movement);
        read_rule(reminders.get("eyeRest"), &mut prefs.reminders.eye_rest);
    }

    if let Some(value) = preferences.get("doNotDisturb").and_then(|v| v.as_bool()) {
        prefs.do_not_disturb = value;
    }
    if let Some(value) = preferences
        .get("idleThresholdMinutes")
        .and_then(|v| v.as_u64())
    {
        prefs.idle_threshold_minutes = (value as u32).max(1);
    }
    if let Some(value) = preferences
        .get("breakDurationMinutes")
        .and_then(|v| v.as_u64())
    {
        prefs.break_duration_minutes = (value as u32).max(1);
    }
    if let Some(values) = preferences
        .get("snoozeOptionsMinutes")
        .and_then(|v| v.as_array())
    {
        let options: Vec<u32> = values
            .iter()
            .filter_map(|value| value.as_u64().map(|n| n.min(60) as u32))
            .filter(|n| *n > 0)
            .collect();

        if !options.is_empty() {
            prefs.snooze_options_minutes = options;
        }
    }

    let guard = AppState::lock(&state);
    // 统一走构造函数让范围夹取生效
    prefs.reminders.rest = tacet_core::model::ReminderRule::new(
        prefs.reminders.rest.enabled,
        prefs.reminders.rest.interval_minutes,
    );
    prefs.reminders.hydration = tacet_core::model::ReminderRule::new(
        prefs.reminders.hydration.enabled,
        prefs.reminders.hydration.interval_minutes,
    );
    prefs.reminders.movement = tacet_core::model::ReminderRule::new(
        prefs.reminders.movement.enabled,
        prefs.reminders.movement.interval_minutes,
    );
    prefs.reminders.eye_rest = tacet_core::model::ReminderRule::new(
        prefs.reminders.eye_rest.enabled,
        prefs.reminders.eye_rest.interval_minutes,
    );

    tacet_storage::repo::SettingsRepo::save_preferences(&guard.db, &prefs)?;
    Ok(())
}

/// 从 JSON 里读一条提醒规则并写进偏好。
fn read_rule(value: Option<&serde_json::Value>, target: &mut tacet_core::model::ReminderRule) {
    let Some(value) = value else {
        return;
    };

    if let Some(enabled) = value.get("enabled").and_then(|v| v.as_bool()) {
        target.enabled = enabled;
    }
    if let Some(minutes) = value.get("intervalMinutes").and_then(|v| v.as_u64()) {
        target.interval_minutes = minutes.min(u32::MAX as u64) as u32;
    }
}

/// 记录一次喝水。
#[tauri::command]
pub fn log_water(app: AppHandle, state: State<'_, SharedState>) -> CmdResult<serde_json::Value> {
    log_behavior(&app, &state, tacet_core::model::BehaviorKind::WaterLogged)
}

/// 记录一次活动。
#[tauri::command]
pub fn log_activity(app: AppHandle, state: State<'_, SharedState>) -> CmdResult<serde_json::Value> {
    log_behavior(
        &app,
        &state,
        tacet_core::model::BehaviorKind::ActivityLogged,
    )
}

/// 记录一次远眺。
#[tauri::command]
pub fn log_eye_rest(app: AppHandle, state: State<'_, SharedState>) -> CmdResult<serde_json::Value> {
    log_behavior(&app, &state, tacet_core::model::BehaviorKind::EyeRestLogged)
}

fn log_behavior(
    app: &AppHandle,
    state: &State<'_, SharedState>,
    kind: tacet_core::model::BehaviorKind,
) -> CmdResult<serde_json::Value> {
    let now = tacet_core::Timestamp::now();

    let snapshot = {
        let mut guard = AppState::lock(state);
        guard.log_behavior(kind, now)?;
        scheduler::build_snapshot(&guard)
    };

    // 广播给其它窗口（比如面板上按了「+1 杯水」，设置窗口的今日统计也该刷新）
    if let Some(snapshot) = snapshot.clone() {
        windows::broadcast(
            app,
            serde_json::json!({ "type": "snapshot", "snapshot": snapshot }),
        );
    }

    snapshot.ok_or_else(|| CommandError::from("记录之后读取状态失败".to_string()))
}

/// 用户点了「现在休息」—— 标记休息开始，并把休息界面打开。
///
/// ## 一个真实踩过的坑：只改状态不显示窗口
///
/// 早期版本这里**只更新了内部状态**，没有调用 `show_break_window`。
/// 结果是：用户点「现在休息」，什么都不会发生 ——
/// 状态机确实切换到了 Breaking，但屏幕上没有任何变化，
/// 用户唯一的感受就是「这个按钮是坏的」。
///
/// 对比 `preview_reminder`（它一开始就调了 `show_break_window`），
/// 差别一目了然。教训：**命令层的职责不只是改数据，还要把结果的
/// 可见部分呈现出来**；只做前一半，用户就会认为功能不存在。
///
/// ## 两件顺手做的事
///
/// 1. **收起主面板**：用户已经决定休息了，面板继续挂着只是碍事
/// 2. **窗口打开失败要记日志**：这是「按钮没反应」类问题的直接证据，
///    不写下来就又要靠猜
///
/// ## 两件事，两个时机（多显示器）
///
/// 1. **主屏的休息界面**：如果它还没显示（用户可能是从主面板点的
///    「现在休息」，而不是从全屏提醒点进来的），现在补上。
/// 2. **其它屏幕的幕布**：到这里才盖。用户刚刚明确表达了「我要休息」
///    这个意愿，把其它屏幕也挡上是履行承诺；早一步（提醒刚弹出、
///    用户还没做决定时）盖就是绑架。
///    这条界线见 `windows::show_break_window` 的注释。
#[tauri::command]
pub fn start_break(app: AppHandle, state: State<'_, SharedState>) -> CmdResult<serde_json::Value> {
    let now = tacet_core::Timestamp::now();

    let snapshot = {
        let mut guard = AppState::lock(&state);
        guard.start_break(now)?;
        scheduler::build_snapshot(&guard)
    };

    // 收起面板：让位给休息界面。
    if let Some(panel) = app.get_webview_window("panel") {
        let _ = panel.hide();
    }

    // 打开休息界面 —— 这才是用户点这个按钮想看到的东西。
    //
    // 成败都记一笔。只记失败是不够的：用户报「点了没反应」时，
    // 日志里什么都没有会有两种解释 —— 请求没到后端，或者到了但成功了。
    // 两种情况的排查方向完全不同，所以成功也要留下痕迹。
    //
    // 其它屏幕的蒙层由 `show_break_window` 内部一并处理（它知道
    // 「主界面先显示、蒙层后铺」这个顺序，理由见那里的文档）。
    // 这里不再单独调用 —— 同一个动作有两个入口，改一处就会漏另一处。
    match windows::show_break_window(&app) {
        Ok(()) => crate::logging::info("用户开始休息，休息界面已打开"),
        Err(err) => crate::logging::error(&format!("打不开休息窗口：{err}")),
    }

    if let Some(snapshot) = snapshot.clone() {
        windows::broadcast(
            &app,
            serde_json::json!({ "type": "snapshot", "snapshot": snapshot }),
        );
    }

    snapshot.ok_or_else(|| CommandError::from("无法开始休息".to_string()))
}

/// 幕布上的用户动作（点击或按 Esc）—— 他想让休息界面收起来。
///
/// ## 为什么幕布不自己处理
///
/// 幕布不知道当前处在哪个阶段（询问 / 填写待办 / 休息中 / 已结束），
/// 而每个阶段「关掉」的含义是不同的：
///
/// - 询问阶段关掉 = 一次普通的关闭（**不记 skip**，他不是在拒绝休息）
/// - 休息中关掉 = 提前结束（记一次完整休息）
///
/// 这些判断只存在于主窗口的状态机里。所以幕布只把「用户想收起来」
/// 这个意图转发过去，由主窗口按当前阶段处理 —— 效果和用户直接在
/// 主窗口按 Esc 完全一致。
///
/// ## 为什么必须记来源窗口
///
/// 这个命令有**两个完全不同的触发源**，而它们对排查的意义相反：
///
/// - `break-veil-*`：用户点了副屏那层幕布。幕布的**整个表面**都是
///   可点击区域（这是「永不困住用户」的代价），所以它也是最容易被
///   误触的一条路径 —— 手肘碰到触控板、在副屏上随手点一下，休息就结束了。
/// - `break`：用户在主屏的休息界面上按了 Esc。
///
/// 用户报过「显示 5 分钟，过了一会就自动结束了」。查库只能看到
/// `break.completed`，而它既可能是「按了 Esc」也可能是「点了幕布」，
/// 甚至可能是「点了提前结束按钮」—— 三条路径的排查方向完全不同。
/// 记下来源窗口，下次这条日志就能直接给出答案。
#[tauri::command]
pub fn dismiss_break(app: AppHandle, window: tauri::WebviewWindow) -> CmdResult<()> {
    crate::logging::info(&format!(
        "收到「收起休息界面」意图：来源窗口 = {}（{}）",
        window.label(),
        if window.label().starts_with("break-veil-") {
            "副屏幕布被点击，这是最容易被误触的路径"
        } else {
            "主屏休息界面（Esc 键或界面上的按钮）"
        }
    ));

    windows::broadcast_dismiss(&app);
    Ok(())
}

/// 记录「下一步要做什么」。
#[tauri::command]
pub fn capture_intent(
    state: State<'_, SharedState>,
    text: String,
) -> CmdResult<Option<serde_json::Value>> {
    let now = tacet_core::Timestamp::now();
    let intent = tacet_core::model::Intent::new(text, now)?;

    let guard = AppState::lock(&state);
    let id = tacet_storage::repo::IntentRepo::save(&guard.db, &intent)?;

    Ok(id.map(|id| {
        serde_json::json!({
            "id": id,
            "text": intent.text,
            "createdAtMs": intent.created_at.as_millis(),
        })
    }))
}

/// 结束休息，返回这次要还给用户的 Intent。
///
/// ## 为什么要记「提前结束」与「耗时」两件事
///
/// 用户报过一个现象：「显示 5 分钟，过了一会就自己结束了」。
/// 这条日志是为了让下一次同类反馈能被直接定位，而不是靠猜：
///
/// - 日志里有「提前结束」→ 是**用户自己**触发的（Esc / 幕布点击 /
///   那个按钮），需要往交互反馈那边查
/// - 日志里没有「提前结束」→ 是 tick 到点自动收的，需要核对
///   `break_ends_at` 是不是被算错了
///
/// 光记一个「休息结束了」不够 —— 结束有两条完全不同的路径，
/// 它们的排查方向正好相反。
#[tauri::command]
pub fn end_break(
    app: AppHandle,
    state: State<'_, SharedState>,
) -> CmdResult<Option<serde_json::Value>> {
    let now = tacet_core::Timestamp::now();

    let (intent, snapshot, early, planned_seconds) = {
        let mut guard = AppState::lock(&state);

        // 结束前先看一眼计划时长与实际过了多久，用来判断这是提前还是到点。
        let planned = guard.break_remaining_seconds(now).unwrap_or(0);
        let early = planned > 0;

        let intent = guard.finish_break(now)?;
        let snapshot = scheduler::build_snapshot(&guard);
        (intent, snapshot, early, planned)
    };

    if early {
        crate::logging::info(&format!(
            "休息提前结束（用户主动），原计划还剩 {planned_seconds} 秒"
        ));
    } else {
        crate::logging::info("休息到点，自动结束");
    }

    // 休息结束 -> 关掉全屏窗口
    windows::hide_break_window(&app);

    if let Some(snapshot) = snapshot {
        windows::broadcast(
            &app,
            serde_json::json!({ "type": "snapshot", "snapshot": snapshot }),
        );
    }

    Ok(intent.map(|intent| {
        serde_json::json!({
            "id": intent.id.unwrap_or(0),
            "text": intent.text,
            "createdAtMs": intent.created_at.as_millis(),
        })
    }))
}

/// 用户跳过这次休息。
///
/// ## 为什么这里必须记日志
///
/// 「这次休息怎么结束的」有三条路径，排查方向完全不同：
///
/// 1. **到点自动结束**（tick 分支，日志「休息到点，自动结束」）
/// 2. **用户主动提前结束**（`end_break`，日志「休息提前结束」）
/// 3. **用户跳过**（就是这里）
///
/// 用户报过「显示 5 分钟，但过了一会就自己结束了」。要判断那到底是
/// 哪一条路径，唯一的依据就是日志 —— 而这条路径原本**一个字都不写**，
/// 于是数据库里凭空多出一条 `break.skipped`，却无从知道是谁触发的。
///
/// 有了这行日志，「自己结束了」就能立刻定位：
/// 日志里有「用户跳过」→ 是界面收到了点击（可能是误触）；
/// 日志里什么都没有 → 那才真的是代码里的自动路径出了问题。
#[tauri::command]
pub fn skip_break(
    app: AppHandle,
    state: State<'_, SharedState>,
    window: tauri::WebviewWindow,
) -> CmdResult<serde_json::Value> {
    let now = tacet_core::Timestamp::now();
    let caller = window.label().to_string();

    let snapshot = {
        let mut guard = AppState::lock(&state);
        let was_breaking = guard.work_state() == tacet_core::state::WorkState::Breaking;
        guard.skip_break(now)?;

        crate::logging::info(&format!(
            "skip_break 被调用：来源窗口 = {caller}，{}",
            if was_breaking {
                "当时正在休息中"
            } else {
                "当时还在提醒阶段"
            }
        ));

        scheduler::build_snapshot(&guard)
    };

    windows::hide_break_window(&app);

    if let Some(snapshot) = snapshot.clone() {
        windows::broadcast(
            &app,
            serde_json::json!({ "type": "snapshot", "snapshot": snapshot }),
        );
    }

    snapshot.ok_or_else(|| CommandError::from("无法跳过休息".to_string()))
}

/// 用户延后这次提醒。
#[tauri::command]
pub fn snooze_break(
    app: AppHandle,
    state: State<'_, SharedState>,
    minutes: u32,
) -> CmdResult<serde_json::Value> {
    let now = tacet_core::Timestamp::now();
    // 延后时长夹取到合理区间：防止前端传一个 10000 分钟把提醒彻底关掉
    let minutes = minutes.clamp(1, 60);

    let snapshot = {
        let mut guard = AppState::lock(&state);
        guard.snooze(minutes, now)?;
        scheduler::build_snapshot(&guard)
    };

    windows::hide_break_window(&app);

    if let Some(snapshot) = snapshot.clone() {
        windows::broadcast(
            &app,
            serde_json::json!({ "type": "snapshot", "snapshot": snapshot }),
        );
    }

    snapshot.ok_or_else(|| CommandError::from("无法延后提醒".to_string()))
}

/// 切换勿扰模式。
#[tauri::command]
pub fn set_do_not_disturb(
    app: AppHandle,
    state: State<'_, SharedState>,
    enabled: bool,
) -> CmdResult<serde_json::Value> {
    let snapshot = {
        let guard = AppState::lock(&state);
        let mut prefs = guard.preferences()?;
        prefs.do_not_disturb = enabled;
        tacet_storage::repo::SettingsRepo::save_preferences(&guard.db, &prefs)?;
        scheduler::build_snapshot(&guard)
    };

    // 开了勿扰就把正在显示的全屏提醒收起来 ——
    // 用户刚刚说了「别打扰我」，这时候还挂着一个全屏窗口是自相矛盾的。
    if enabled {
        windows::hide_break_window(&app);
    }

    if let Some(snapshot) = snapshot.clone() {
        windows::broadcast(
            &app,
            serde_json::json!({ "type": "snapshot", "snapshot": snapshot }),
        );
    }

    snapshot.ok_or_else(|| CommandError::from("无法切换勿扰模式".to_string()))
}

/// 暂停计时。
#[tauri::command]
pub fn pause_tracking(
    app: AppHandle,
    state: State<'_, SharedState>,
) -> CmdResult<serde_json::Value> {
    set_paused(&app, &state, true)
}

/// 恢复计时。
#[tauri::command]
pub fn resume_tracking(
    app: AppHandle,
    state: State<'_, SharedState>,
) -> CmdResult<serde_json::Value> {
    set_paused(&app, &state, false)
}

fn set_paused(
    app: &AppHandle,
    state: &State<'_, SharedState>,
    paused: bool,
) -> CmdResult<serde_json::Value> {
    let now = tacet_core::Timestamp::now();

    let snapshot = {
        let mut guard = AppState::lock(state);
        guard.set_paused(paused, now)?;
        scheduler::build_snapshot(&guard)
    };

    if let Some(snapshot) = snapshot.clone() {
        windows::broadcast(
            app,
            serde_json::json!({ "type": "snapshot", "snapshot": snapshot }),
        );
    }

    snapshot.ok_or_else(|| CommandError::from("无法切换计时状态".to_string()))
}

/// 手动预览一次整屏提醒。
///
/// ## 它存在的理由
///
/// 「我设了 45 分钟，怎么知道到点会是什么样？」—— 等 45 分钟太久了。
/// 这个命令让用户（和演示）立刻看到提醒长什么样。
///
/// ## 不写库、不影响统计
///
/// 它不该污染「接受率」这类真实数据，所以**不落 `interventions` 表**、
/// 不碰 `last_interruption_at`。只把这一次的决策放进内存里的
/// `last_decision`，让界面能按它渲染文案。
///
/// ## 为什么要能指定需求类型
///
/// 四类提醒在这一屏上的文案和主按钮都不同（休息是「现在休息」，
/// 喝水是「喝了」）。只预览休息的话，另外三类就等于没人验证过 ——
/// 而这正是之前那个「喝水提醒从来没送达过」能藏这么久的原因之一。
#[tauri::command]
pub fn preview_reminder(
    app: AppHandle,
    state: State<'_, SharedState>,
    kind: Option<String>,
) -> CmdResult<serde_json::Value> {
    use tacet_core::model::{InterventionLevel, NeedKind, Reason};
    use tacet_core::policy::InterventionDecision;

    let need_kind = match kind.as_deref() {
        Some("hydration") => NeedKind::Hydration,
        Some("movement") => NeedKind::Movement,
        Some("eye_rest") => NeedKind::EyeRest,
        // 默认休息：它是产品的主场景
        _ => NeedKind::Rest,
    };

    // 每个需求类型配一句真实会出现的理由文案（照抄 decide() 的产出格式）
    let reasons = match need_kind {
        NeedKind::Rest => vec![
            Reason::ContinuousWork { minutes: 50 },
            Reason::SinceLastBreak { minutes: 80 },
        ],
        NeedKind::Hydration => vec![Reason::SinceLastHydration { minutes: 45 }],
        NeedKind::Movement => vec![Reason::SinceLastMovement { minutes: 60 }],
        NeedKind::EyeRest => vec![Reason::ScreenTime { minutes: 45 }],
        NeedKind::Fused => Vec::new(),
    };

    let decision = InterventionDecision {
        kind: need_kind,
        level: InterventionLevel::FullScreen,
        reasons,
        actions: tacet_core::policy::suggested_actions(need_kind),
        fused: Vec::new(),
    };

    // 让界面按这次预览的决策渲染文案（内存字段，下一次真实 tick 会覆盖它）
    {
        let mut guard = AppState::lock(&state);
        guard.last_decision = Some(decision.clone());
    }

    windows::show_break_window(&app)?;

    // 广播新快照，让刚打开的窗口立刻拿到这次预览的决策 ——
    // 否则它会先渲染成上一次的真实决策，然后在下一次 tick 时才跳变。
    let snapshot = {
        let guard = AppState::lock(&state);
        scheduler::build_snapshot(&guard)
    };
    if let Some(snapshot) = snapshot {
        windows::broadcast(
            &app,
            serde_json::json!({ "type": "snapshot", "snapshot": snapshot }),
        );
    }

    Ok(serde_json::json!({
        "kind": match need_kind {
            NeedKind::Rest => "rest",
            NeedKind::Hydration => "hydration",
            NeedKind::Movement => "movement",
            NeedKind::EyeRest => "eyeRest",
            NeedKind::Fused => "rest",
        },
        "level": 4,
        "reasons": decision.reasons,
        "actions": decision.actions,
    }))
}

/// 读取平台能力报告。
#[tauri::command]
pub fn get_capabilities(state: State<'_, SharedState>) -> CmdResult<Vec<serde_json::Value>> {
    let guard = AppState::lock(&state);
    let report = guard.platform.capabilities();

    Ok(tacet_platform::Capability::ALL
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
        .collect())
}

/// 打开设置窗口。
///
/// ## 为什么成功也要记日志
///
/// 前端调这两个命令时用的是 `void api.openSettingsWindow()` —— 返回值被丢掉了。
/// 这意味着窗口打不开时，用户在界面上看不到任何反应，前端也留不下痕迹。
///
/// 这和当初「现在休息按钮没反应」是同一类问题：**一个没有反馈的操作，
/// 在用户看来就是不存在**。所以这里成败都记一笔 ——
/// 日志里有「设置窗口已打开」，就说明请求到了后端；什么都没有，
/// 就是 IPC 或界面层的问题。两种情况排查方向完全不同，不能靠猜。
#[tauri::command]
pub fn open_settings_window(app: AppHandle) -> CmdResult<()> {
    match windows::open_settings(&app) {
        Ok(()) => {
            crate::logging::info("设置窗口已打开");
            Ok(())
        }
        Err(err) => {
            crate::logging::error(&format!("打不开设置窗口：{err}"));
            Err(err.into())
        }
    }
}

/// 打开今日记录窗口（同样成败都记日志，理由见上面 `open_settings_window`）。
#[tauri::command]
pub fn open_today_window(app: AppHandle) -> CmdResult<()> {
    match windows::open_today(&app) {
        Ok(()) => {
            crate::logging::info("今日记录窗口已打开");
            Ok(())
        }
        Err(err) => {
            crate::logging::error(&format!("打不开今日记录窗口：{err}"));
            Err(err.into())
        }
    }
}

/// 关闭当前窗口（Overlay 的按钮用它）。
///
/// ## 关休息窗口时必须连幕布一起收
///
/// 幕布是盖在**其它屏幕**上的遮挡层。如果只关掉主窗口、把幕布留在那儿，
/// 用户会看到副屏（甚至另外两块屏）一直糊着白雾，而主屏什么都没有 ——
/// 那比不休息更糟，因为他不知道该点哪里才能让屏幕恢复正常。
///
/// 「永不困住用户」在这里的具体含义就是：**主窗口和幕布必须同生共死**。
/// 所以在这一个命令里把两者一起关掉，而不是指望每个调用方都记得。
#[tauri::command]
pub fn close_current_window(app: AppHandle, window: tauri::Window) -> CmdResult<()> {
    if window.label() == "break" {
        windows::hide_break_window(&app);
    } else {
        window.hide()?;
    }
    Ok(())
}

/// 调整面板窗口高度（内容变化时用）。
#[tauri::command]
pub fn resize_panel(window: tauri::Window, height: f64) -> CmdResult<()> {
    // 夹取一个合理范围，防止前端传一个离谱的值把窗口撑爆
    let height = height.clamp(200.0, 900.0);
    let _ = window.set_size(tauri::LogicalSize::new(376.0, height));
    Ok(())
}
