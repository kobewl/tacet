/**
 * Rust ↔ UI 的类型契约。
 *
 * 这里的每个类型都对应 `crates/` 里的一个 Rust 结构体（用 `#[serde(rename_all
 * = "camelCase")]` 序列化）。改动任何一边都必须同步另一边 ——
 * 这是前后端之间唯一的真相通道。
 *
 * 命名约定：Rust 侧是 snake_case，到了前端统一 camelCase。
 */

/** 工作状态。对应 `tacet_core::state::WorkState`。 */
export type WorkState = "idle" | "working" | "breaking" | "away";

/** 四类健康需求。对应 `tacet_core::model::NeedKind`。 */
export type NeedKind = "rest" | "hydration" | "movement" | "eyeRest";

/** 干预等级 0~5。对应 `tacet_core::model::InterventionLevel`。 */
export type InterventionLevel = 0 | 1 | 2 | 3 | 4 | 5;

/** 用户对一次提醒的回应。对应 `tacet_core::model::InterventionOutcome`。 */
export type InterventionOutcome = "completed" | "snoozed" | "skipped" | "ignored";

/**
 * 一条决策依据。
 *
 * 与 Rust 侧 `Reason` 的 `#[serde(tag = "reason")]` 对应。
 * 之所以用可辨识联合而不是纯字符串：v0.3 的学习模块需要按类型统计
 * 「用户最常在哪种理由下跳过提醒」，字符串是没法可靠统计的。
 */
export type Reason =
  | { reason: "continuous_work"; minutes: number }
  | { reason: "since_last_break"; minutes: number }
  | { reason: "since_last_hydration"; minutes: number }
  | { reason: "since_last_movement"; minutes: number }
  | { reason: "screen_time"; minutes: number }
  | { reason: "app_fullscreen"; app: string }
  | { reason: "user_away" }
  | { reason: "do_not_disturb" }
  | { reason: "need_below_threshold"; kind: NeedKind; percent: number }
  | { reason: "rate_limited"; kind: NeedKind; minutes_ago: number }
  | { reason: "context_unavailable" };

/** 一次决策的结果。对应 `tacet_core::policy::InterventionDecision`。 */
export interface Decision {
  kind: NeedKind;
  level: InterventionLevel;
  reasons: Reason[];
  actions: string[];
}

/** 四类需求的当前强度（0~1）。 */
export interface HealthNeeds {
  rest: number;
  hydration: number;
  movement: number;
  eyeRest: number;
}

/** 今日统计。所有日期边界都在 Rust 侧算好（数据模型 §8 的约束）。 */
export interface TodaySummary {
  /** 累计工作时长（分钟）。 */
  workMinutes: number;
  /** 最长连续工作（分钟）。 */
  longestStreakMinutes: number;
  /** 喝水次数。 */
  waterCount: number;
  /** 活动次数。 */
  activityCount: number;
  /** 完成的休息次数。 */
  breakCompletedCount: number;
  /** 跳过的次数。 */
  breakSkippedCount: number;
  /** 延后的次数。 */
  breakSnoozedCount: number;
  /** 接受率（0~1）；没有数据时为 null。 */
  acceptanceRate: number | null;
}

/** 一类提醒的配置。对应 `tacet_core::model::ReminderRule`。 */
export interface ReminderRule {
  enabled: boolean;
  intervalMinutes: number;
}

/** 用户偏好。对应 `tacet_core::model::UserPreferences`。 */
export interface UserPreferences {
  reminders: {
    rest: ReminderRule;
    hydration: ReminderRule;
    movement: ReminderRule;
    eyeRest: ReminderRule;
  };
  doNotDisturb: boolean;
  idleThresholdMinutes: number;
  breakDurationMinutes: number;
  snoozeOptionsMinutes: number[];
}

/** 应用的完整状态快照 —— UI 渲染的唯一数据来源。 */
export interface AppSnapshot {
  /** 工作状态。 */
  state: WorkState;
  /** 这一口气连续工作了多少分钟。 */
  continuousWorkMinutes: number;
  /** 已经多久没输入了（秒）。 */
  idleSeconds: number;
  /** 四类需求的当前强度。 */
  needs: HealthNeeds;
  /** 四类提醒的配置（面板上显示「距提醒还有多久」用）。 */
  reminders: UserPreferences["reminders"];
  /** 距离上次喝水多久（分钟）；没有记录时为 null。 */
  lastWaterMinutesAgo: number | null;
  /** 距离上次活动多久（分钟）；没有记录时为 null。 */
  lastActivityMinutesAgo: number | null;
  /** 距离上次远眺多久（分钟）；没有记录时为 null。 */
  lastEyeRestMinutesAgo: number | null;
  /** 今日统计。 */
  today: TodaySummary;
  /** 勿扰模式是否开启。 */
  doNotDisturb: boolean;
  /** 当前暂停的 Intent（如果有一条尚未恢复的）。 */
  pendingIntent: IntentRecord | null;
  /** 最近一次决策（用于展示「为什么」）。 */
  lastDecision: Decision | null;
  /** 当前正在休息时，剩余多少秒。 */
  breakRemainingSeconds: number | null;
  /** 平台能力报告：哪些功能这台机器上不可用。 */
  capabilities: CapabilityInfo[];
}

/** 一条 Intent 记录。 */
export interface IntentRecord {
  id: number;
  text: string;
  createdAtMs: number;
}

/** 平台能力状态。 */
export interface CapabilityInfo {
  name: string;
  displayName: string;
  available: boolean;
  /** 不可用时的原因；可用时为 null。 */
  reason: string | null;
}

/** 后台线程推送给 UI 的事件载荷。 */
export type TacetEvent =
  | { type: "snapshot"; snapshot: AppSnapshot }
  | { type: "intervention"; decision: Decision }
  | { type: "breakEnded" }
  | { type: "breakStarted" }
  /**
   * 用户在副屏的幕布上做了动作（点击或 Esc），希望收起休息界面。
   *
   * 幕布自己不知道当前处在哪个阶段（询问 / 填写 / 休息中 / 已结束），
   * 所以它只发这个意图，由主窗口按阶段处理 —— 效果等同于在主窗口按 Esc。
   */
  | { type: "dismiss" }
  /**
   * 休息窗口被（重新）显示出来了，流程该重置到正确的阶段。
   *
   * 隐藏窗口不会卸载网页，所以组件还停在上一次离开时的阶段：
   * 上一次休息正常结束后是「欢迎回来」，下次提醒就会错误地显示它。
   * 收到这个事件后重新读一次快照，按真实状态决定该显示哪一步。
   */
  | { type: "breakShown" };

/** 需求类型的显示元数据。 */
export const NEED_META: Record<
  NeedKind,
  { label: string; icon: string; category: string }
> = {
  rest: { label: "休息", icon: "☕", category: "rest" },
  hydration: { label: "喝水", icon: "💧", category: "water" },
  movement: { label: "活动", icon: "🧍", category: "move" },
  eyeRest: { label: "护眼", icon: "👁", category: "eye" },
};

/** 干预等级的显示名。 */
export const LEVEL_LABELS: Record<InterventionLevel, string> = {
  0: "静默",
  1: "环境提示",
  2: "系统通知",
  3: "悬浮卡片",
  4: "全屏提醒",
  5: "升级提醒",
};

/** 把一条决策依据渲染成给用户看的一句话。 */
export function reasonText(reason: Reason): string {
  switch (reason.reason) {
    case "continuous_work":
      return `已连续工作 ${reason.minutes} 分钟`;
    case "since_last_break":
      return `距离上次休息 ${reason.minutes} 分钟`;
    case "since_last_hydration":
      return `距离上次喝水 ${reason.minutes} 分钟`;
    case "since_last_movement":
      return `已经坐着 ${reason.minutes} 分钟没起身`;
    case "screen_time":
      return `连续看屏幕 ${reason.minutes} 分钟`;
    case "app_fullscreen":
      return `${reason.app} 正在全屏使用`;
    case "user_away":
      return "你刚刚不在电脑前";
    case "do_not_disturb":
      return "勿扰模式已开启";
    case "need_below_threshold":
      return `${NEED_META[reason.kind].label}需求 ${reason.percent}%，暂时不需要提醒`;
    case "rate_limited":
      return `${reason.minutes_ago} 分钟前刚提醒过${NEED_META[reason.kind].label}`;
    case "context_unavailable":
      return "暂时读不到上下文，按基础规则处理";
  }
}

/** 把分钟数格式化成「1h 26m」这样的人类可读形式。 */
export function formatDuration(minutes: number): string {
  if (minutes < 1) return "不到 1 分钟";
  if (minutes < 60) return `${Math.round(minutes)} 分钟`;

  const hours = Math.floor(minutes / 60);
  const rest = Math.round(minutes % 60);
  if (rest === 0) return `${hours} 小时`;
  return `${hours} 小时 ${rest} 分钟`;
}

/** 把秒数格式化成倒计时用 mm:ss。 */
export function formatClock(totalSeconds: number): string {
  const safe = Math.max(0, Math.floor(totalSeconds));
  const minutes = Math.floor(safe / 60);
  const seconds = safe % 60;
  return `${String(minutes).padStart(2, "0")}:${String(seconds).padStart(2, "0")}`;
}
