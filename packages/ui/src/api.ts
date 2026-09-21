/**
 * 与 Rust 侧通信的唯一入口。
 *
 * ## 为什么所有 IPC 调用都收在这一个文件里
 *
 * 1. **类型安全**：命令名和返回值类型在这里一次性对上，组件里不再出现裸字符串
 * 2. **可替换**：开发时没有 Tauri 运行时，这里可以整体切到假数据（见下）
 * 3. **可审计**：想知道前端能对后端做什么，读这一个文件就够了
 *
 * ## 业务真值在 Rust 侧（架构文档 §11 A-4）
 *
 * 前端**不持有任何业务状态**：不自己算「距上次喝水多久」，不自己判断
 * 「该不该提醒」。它只做两件事 —— 把 Rust 给的状态画出来，把用户的操作传回去。
 *
 * 一个具体体现：所有日期边界（「今天」从几点开始）都由 Rust 算好再传过来。
 * 数据模型 §8 把「UI 层禁止自行计算日期边界」写成了工程约束，
 * 因为时区口径混用会造成统计错位。
 */

import type {
  AppSnapshot,
  Decision,
  IntentRecord,
  TodaySummary,
  UpdateInfo,
  UserPreferences,
} from "./types";

/** 是否运行在 Tauri 环境里（浏览器里打开时为 false）。 */
export const isTauri = (): boolean =>
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

/**
 * 把 IPC 抛出的错误变成一句能给用户看的话。
 *
 * ## 为什么不能只判断 `instanceof Error`
 *
 * Rust 命令返回 `Err(...)` 时，Tauri **不会**构造一个 JS 的 `Error` ——
 * 它把错误对象**序列化成普通对象**再 reject（形状是 `{ message: "..." }`，
 * 对应 Rust 侧的 `CommandError`）。
 *
 * 于是 `err instanceof Error` 是 false，`String(err)` 得到 `"[object Object]"`。
 * 这不是假设：设置页的「检查更新」真实显示过这七个字，而**真正的原因
 * （仓库还没发过版本）被它盖住了** —— 报错信息把一个本来可解释的状态
 * 变成了一个谜。
 *
 * 所以错误展示统一走这里，不要在组件里自己写 `instanceof` 判断。
 */
export function errorMessage(err: unknown): string {
  if (err instanceof Error) return err.message;
  if (typeof err === "string") return err;

  // Tauri 的 IPC 错误：Rust 侧结构体被序列化后的形状
  if (typeof err === "object" && err !== null && "message" in err) {
    // `in` 已经把类型收窄成 `{ message: unknown }`，不需要再断言一次
    if (typeof err.message === "string") return err.message;
  }

  // 兜底：至少给出一段可搜索的文本，而不是一句 [object Object]
  try {
    const json = JSON.stringify(err);
    if (json && json !== "{}") return json;
  } catch {
    // 循环引用之类序列化失败的情况，落到最后一行
  }

  return String(err);
}

/** 动态导入 Tauri API —— 这样在浏览器里也能跑（走假数据）。 */
async function invoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  if (!isTauri()) {
    return mockInvoke<T>(command, args);
  }

  const { invoke: tauriInvoke } = await import("@tauri-apps/api/core");
  return tauriInvoke<T>(command, args);
}

/** 监听 Rust 侧推来的事件。返回取消监听的函数。 */
export async function listen<T>(
  event: string,
  handler: (payload: T) => void,
): Promise<() => void> {
  if (!isTauri()) {
    return () => {};
  }

  const { listen: tauriListen } = await import("@tauri-apps/api/event");
  const unlisten = await tauriListen<T>(event, (e) => handler(e.payload));
  return unlisten;
}

// ============================================================ 命令

/** 取当前完整状态快照。UI 的所有渲染都基于它。 */
export const getSnapshot = () => invoke<AppSnapshot>("get_snapshot");

/** 取今日统计。 */
export const getTodaySummary = () => invoke<TodaySummary>("get_today_summary");

/** 取用户偏好。 */
export const getPreferences = () => invoke<UserPreferences>("get_preferences");

/** 保存用户偏好。 */
export const savePreferences = (preferences: UserPreferences) =>
  invoke<void>("save_preferences", { preferences });

/** 记录一次喝水。 */
export const logWater = () => invoke<AppSnapshot>("log_water");

/** 记录一次活动 / 站立。 */
export const logActivity = () => invoke<AppSnapshot>("log_activity");

/** 记录一次远眺。 */
export const logEyeRest = () => invoke<AppSnapshot>("log_eye_rest");

/** 用户点了「现在休息」。会先要求记录 Intent，再进入休息。 */
export const startBreak = () => invoke<AppSnapshot>("start_break");

/** 记录「下一步要做什么」。 */
export const captureIntent = (text: string) =>
  invoke<IntentRecord | null>("capture_intent", { text });

/**
 * 休息结束，读取刚才的 Intent。
 *
 * ## 为什么这里要「洗」一遍返回值
 *
 * 休息结束页要读 `intent.text` 才能把待办还给用户。如果这个字段是
 * `undefined`（假后端形状不对、Rust 侧序列化改了名字、将来的重构
 * 漏了字段），组件会在渲染时抛错 —— 而这个组件跑在一个**全屏、
 * 无边框、常在最前**的窗口里。
 *
 * 崩溃的后果不是「少显示一块内容」：React 会把整棵树卸载掉，
 * 窗口变成一片空白，用户既看不到按钮也按不了 Esc，
 * **除了强退应用没有别的出路**。
 *
 * 所以在这里把形状收口：拿不到合法的文字就当「没有待办」，
 * 让休息结束页走它的空态分支。一个格式问题不该让用户失去屏幕。
 */
export const endBreak = async (): Promise<IntentRecord | null> => {
  const raw = await invoke<IntentRecord | null>("end_break");

  if (raw && typeof raw.text === "string") {
    return raw;
  }
  return null;
};

/** 用户跳过这次休息。 */
export const skipBreak = () => invoke<AppSnapshot>("skip_break");

/** 用户延后这次提醒。 */
export const snoozeBreak = (minutes: number) =>
  invoke<AppSnapshot>("snooze_break", { minutes });

/** 切换勿扰模式。 */
export const setDoNotDisturb = (enabled: boolean) =>
  invoke<AppSnapshot>("set_do_not_disturb", { enabled });

/** 暂停计时（v0.1 里是「我离开一会儿」）。 */
export const pauseTracking = () => invoke<AppSnapshot>("pause_tracking");

/** 恢复计时。 */
export const resumeTracking = () => invoke<AppSnapshot>("resume_tracking");

/** 手动触发一次全屏休息提醒（用于体验，不属于自动提醒路径）。 */
export const previewReminder = () => invoke<Decision>("preview_reminder");

/** 读取当前平台能力状态。 */
export const getCapabilities = () => invoke<AppSnapshot["capabilities"]>("get_capabilities");

/** 打开设置窗口（已开则聚焦到它）。 */
export const openSettingsWindow = () => invoke<void>("open_settings_window");

/** 关闭当前窗口（Overlay 的「跳过」用它）。 */
export const closeCurrentWindow = () => invoke<void>("close_current_window");

/**
 * 幕布（副屏遮挡层）上的用户动作 —— 请主窗口收起休息界面。
 *
 * 幕布是独立窗口，它不知道休息流程当前在哪个阶段，所以不自己处理，
 * 只发这个意图。主窗口收到 `dismiss` 事件后按阶段决定该做什么。
 */
export const dismissBreak = () => invoke<void>("dismiss_break");

/**
 * 读取当前应用版本。
 *
 * 走 Tauri 的 `getVersion()`（读的是打包进二进制的版本号），而不是前端
 * 自己写一个常量。这一条对「更新」这件事是必须的：如果版本号是写死的，
 * 更新完之后界面还会显示旧版本，用户会以为更新失败了。
 *
 * 浏览器预览下没有这个 API，返回一个显眼的占位值 —— 让开发时一眼看出
 * 「这是假数据」，而不是误以为版本号真的叫这个名字。
 */
export async function getAppVersion(): Promise<string> {
  if (!isTauri()) return "0.1.0（浏览器预览）";

  const { getVersion } = await import("@tauri-apps/api/app");
  return getVersion();
}

/** 把主面板窗口调整到内容需要的高度。 */
export const resizePanel = (height: number) => invoke<void>("resize_panel", { height });

// ============================================================ 开机自启

/**
 * 读取当前是否已设置为开机自启。
 *
 * ## 为什么每次都问后端
 *
 * 这个状态存在**系统**里（`~/Library/LaunchAgents` 下的登录项），不在本应用的
 * 数据库里。用户可以随时在「系统设置 → 通用 → 登录项」里把它改掉 ——
 * 前端缓存一份就会显示过期状态，那种「我明明关了它怎么还开着」最让人不信任。
 */
export const getAutostartEnabled = () => invoke<boolean>("get_autostart_enabled");

/**
 * 打开或关闭开机自启，**立即生效**。
 *
 * 它改的是系统里的登录项，不是本应用的配置 —— 所以没有「保存」这一步。
 * 失败时调用方应当把开关弹回原值：让控件停在用户点的位置上，
 * 他会以为已经设好了。
 */
export const setAutostartEnabled = (enabled: boolean) =>
  invoke<void>("set_autostart_enabled", { enabled });

// ============================================================ 应用更新

/**
 * 检查有没有新版本。
 *
 * 返回 `null` 表示**已经是最新**（一个正常结果，不是错误）；
 * 抛错表示检查本身失败了（网络不通、发布渠道没就绪）。
 *
 * ## 为什么不用「自动检查更新」
 *
 * 菜单栏应用在后台偷偷联网，是一件需要向用户解释的事。Tacet 的原则是
 * 「不配置任何东西时它也该完全安静地工作」（原则 7），所以 v0.1 里更新是
 * **用户主动触发**的：点了才查。将来要做后台检查时，也应该是一个默认关闭的
 * 开关，而不是默认打开的行为。
 */
export const checkUpdate = () => invoke<UpdateInfo | null>("check_update");

/**
 * 下载并安装更新。成功后应用会重启 —— 也就是说**这个 Promise 可能不会返回**。
 *
 * 调用方不该依赖它之后继续执行；进度通过 `update:progress` 事件推过来。
 */
export const installUpdate = () => invoke<void>("install_update");

/** 监听更新包下载进度（0~100）。返回取消监听的函数。 */
export const onUpdateProgress = (handler: (percent: number) => void) =>
  listen<number>("update:progress", handler);

/**
 * 用系统浏览器打开发布页 —— 自动更新不可用时的兜底路径。
 *
 * 后端只放行本项目 Release 页下的地址（见 Rust 侧 `open_release_page`），
 * 这里传别的 URL 会被拒绝。
 */
export const openReleasePage = (url: string) =>
  invoke<void>("open_release_page", { url });

// ============================================================ 开发用假数据
//
// 在浏览器里 `pnpm dev` 时（没有 Tauri 运行时），所有命令都走这里。
// 它的价值不只是「能预览界面」：它还是**界面契约的活文档** ——
// 想确认某个字段前端到底要什么形态，读这里比读 Rust 更快。

const now = Date.now();

/**
 * 浏览器预览里的开机自启状态。
 *
 * 与 `mockSnapshot` 分开：它是**系统状态**，不属于业务快照 ——
 * 真实实现里它存在系统的登录项里，连数据库都不进。
 */
let mockAutostart = false;

// 用 `const` 而非 `let`：我们只改它的**字段**，不重新赋值整个对象。
// 这样 TypeScript 能确定这个引用的身份永远不变。
const mockSnapshot: AppSnapshot = {
  state: "working",
  continuousWorkMinutes: 62,
  idleSeconds: 4,
  needs: { rest: 0.72, hydration: 0.95, movement: 0.51, eyeRest: 0.4 },
  reminders: {
    rest: { enabled: true, intervalMinutes: 50 },
    hydration: { enabled: true, intervalMinutes: 45 },
    movement: { enabled: true, intervalMinutes: 60 },
    eyeRest: { enabled: true, intervalMinutes: 40 },
  },
  lastWaterMinutesAgo: 43,
  lastActivityMinutesAgo: 31,
  lastEyeRestMinutesAgo: 16,
  today: {
    workMinutes: 214,
    longestStreakMinutes: 62,
    waterCount: 4,
    activityCount: 3,
    breakCompletedCount: 2,
    breakSkippedCount: 1,
    breakSnoozedCount: 1,
    acceptanceRate: 0.67,
  },
  doNotDisturb: false,
  pendingIntent: null,
  lastDecision: {
    kind: "hydration",
    level: 2,
    reasons: [
      { reason: "since_last_hydration", minutes: 43 },
      { reason: "need_below_threshold", kind: "hydration", percent: 95 },
    ],
    actions: ["喝几口水"],
  },
  breakRemainingSeconds: null,
  breakTotalSeconds: null,
  capabilities: [
    { name: "idle_detection", displayName: "空闲检测", available: true, reason: null },
    { name: "foreground_app", displayName: "前台应用识别", available: true, reason: null },
    { name: "fullscreen_detection", displayName: "全屏检测", available: true, reason: null },
    { name: "screen_enumeration", displayName: "显示器识别", available: true, reason: null },
    {
      name: "notification",
      displayName: "系统通知",
      available: true,
      reason: null,
    },
    { name: "startup_launch", displayName: "开机自启", available: true, reason: null },
    {
      name: "meeting_detection",
      displayName: "会议检测",
      available: false,
      reason: "会议检测属于 v0.2 范围",
    },
  ],
};

/**
 * 从 IPC 参数里安全地取出一个字符串。
 *
 * 为什么不直接写 `String(args?.text ?? "")`：`args` 的值类型是 `unknown`，
 * 直接 `String()` 一个对象会得到 `"[object Object]"` —— 一个看起来像
 * 成功了、实际上完全错误的字符串。类型收窄是这里唯一正确的做法。
 */
function argString(args: Record<string, unknown> | undefined, key: string): string {
  const value = args?.[key];
  return typeof value === "string" ? value : "";
}

/** 从 IPC 参数里安全地取出一个布尔值。 */
function argBool(args: Record<string, unknown> | undefined, key: string): boolean {
  return args?.[key] === true;
}

/**
 * 假后端里的用户偏好 —— **必须与快照分开存**。
 *
 * ## 为什么不能复用快照
 *
 * `get_snapshot` 和 `get_preferences` 返回的是**两种不同的对象**：
 * 快照给界面显示用（四类需求分数、今日统计…），偏好给设置页编辑用
 * （提醒间隔、空闲阈值、休息时长…）。它们的字段几乎没有重叠。
 *
 * 早期这里让 `get_preferences` 也返回快照，结果是设置页的
 * 「多久算离开」和「一次休息多久」永远显示「1 分钟」——
 * 因为快照里根本没有这两个字段，`<select>` 读到 `undefined`
 * 就回退到第一个选项。真实应用里不会出现（Rust 侧返回的是真偏好），
 * 但开发时会被这个假象误导很久。
 */
const mockPreferences: UserPreferences = {
  reminders: {
    rest: { enabled: true, intervalMinutes: 50 },
    hydration: { enabled: true, intervalMinutes: 45 },
    movement: { enabled: true, intervalMinutes: 60 },
    eyeRest: { enabled: true, intervalMinutes: 40 },
  },
  doNotDisturb: false,
  idleThresholdMinutes: 5,
  breakDurationMinutes: 5,
  snoozeOptionsMinutes: [1, 3, 5],
};

/**
 * 开发便利：允许用 URL 参数把假快照钉在某一种状态上。
 *
 * ## 为什么需要它
 *
 * 有些界面的样子**完全取决于状态**，而假快照的模块状态在每次
 * 页面导航（`?view=` 变化就是一次导航）时都会重置回初始值。
 * 最典型的例子是副屏幕布（`views/BreakVeil.tsx`）：它要区分
 * 「提醒刚弹出」和「正在休息」两种样子，而这两种样子没法在
 * 一次页面加载里都看到。
 *
 * 于是在开发时可以直接打开 `?view=veil&state=breaking` 检查
 * 「休息中」的样子，不必先跑一遍完整的休息流程。
 *
 * ## 它为什么是安全的
 *
 * 只在**浏览器预览**路径（没有 Tauri 运行时）里生效 —— 真实应用里
 * 状态由 Rust 决定，这个函数根本不会被调用（`isTauri()` 会走另一条路）。
 *
 * 支持的参数：
 * - `state`：`working` / `breaking` / `away` / `idle`
 * - `remaining`：休息剩余秒数（配合 `state=breaking` 看倒计时）
 */
function applyDevOverrides(): void {
  if (typeof window === "undefined") return;

  const params = new URLSearchParams(window.location.search);

  const state = params.get("state");
  if (
    state === "working" ||
    state === "breaking" ||
    state === "away" ||
    state === "idle"
  ) {
    mockSnapshot.state = state;
    mockSnapshot.breakRemainingSeconds =
      state === "breaking" ? Number(params.get("remaining") ?? 300) : null;
    // 总时长必须跟着一起给：界面用 `remaining=240&state=breaking` 这种
    // 链接预览休息页时，分母也得有个值，否则进度环会除以 0。
    mockSnapshot.breakTotalSeconds =
      state === "breaking" ? Number(params.get("total") ?? 300) : null;
  }
}

/**
 * 假后端 —— 会**真的随操作变化**。
 *
 * ## 为什么必须可变
 *
 * 最初这里永远返回同一份数据。结果是：在浏览器里点「+1 杯水」，
 * 界面毫无反应 —— 开发时根本没法确认交互到底有没有接通。
 * 一个不会响应的假后端，比没有假后端更糟：它让人误以为代码有问题。
 *
 * ## 关于「业务真值在 Rust 侧」（研发规范 §3.2）
 *
 * 这条规则针对的是**产品代码**。这里是只在浏览器预览时启用的测试替身，
 * 它的职责恰恰是**模拟** Rust 侧的状态变化。真正的状态依然只存在于
 * `tacet-desktop` 里，前端拿到的永远是快照。
 */
async function mockInvoke<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  // 模拟一点点调用延迟，让加载态在开发时也能被看到
  await new Promise((resolve) => setTimeout(resolve, 40));

  applyDevOverrides();

  const snapshot = () => mockSnapshot as unknown as T;

  switch (command) {
    // ── 只读命令：直接返回当前快照 ──
    case "get_snapshot":
    case "get_today_summary":
    case "get_capabilities":
      return snapshot();

    // 偏好走独立的一份数据，见 mockPreferences 的说明
    case "get_preferences":
      return structuredClone(mockPreferences) as unknown as T;

    // ── 记录类行为：更新分数与计数 ──
    //
    // 需求分数归零 + 「多久之前」归零，这两件事必须同时发生 ——
    // 只改一个会让界面出现「刚喝完水，但仍提示 43 分钟没喝水」的矛盾。
    case "log_water":
      mockSnapshot.needs.hydration = 0;
      mockSnapshot.lastWaterMinutesAgo = 0;
      mockSnapshot.today.waterCount += 1;
      return snapshot();

    case "log_activity":
      mockSnapshot.needs.movement = 0;
      mockSnapshot.lastActivityMinutesAgo = 0;
      mockSnapshot.today.activityCount += 1;
      return snapshot();

    case "log_eye_rest":
      mockSnapshot.needs.eyeRest = 0;
      mockSnapshot.lastEyeRestMinutesAgo = 0;
      return snapshot();

    // ── 休息流程 ──
    case "start_break":
      mockSnapshot.state = "breaking";
      mockSnapshot.breakRemainingSeconds = 300;
      mockSnapshot.breakTotalSeconds = 300;
      return snapshot();

    case "end_break": {
      // 这个命令返回的是**待办本身**，不是快照 —— 休息结束页要把那句话
      // 原样还给用户。返回值形状必须和 Rust 侧一致（`Option<IntentRecord>`），
      // 否则界面会在读取 `intent.text` 时崩掉。
      const restored = mockSnapshot.pendingIntent;

      mockSnapshot.state = "working";
      mockSnapshot.breakRemainingSeconds = null;
      mockSnapshot.breakTotalSeconds = null;
      mockSnapshot.today.breakCompletedCount += 1;
      mockSnapshot.needs.rest = 0;
      // 休息结束后，这条 Intent 就算「已经还给你了」
      mockSnapshot.pendingIntent = null;

      return restored as unknown as T;
    }

    case "skip_break":
      mockSnapshot.state = "working";
      mockSnapshot.breakRemainingSeconds = null;
      mockSnapshot.breakTotalSeconds = null;
      mockSnapshot.today.breakSkippedCount += 1;
      return snapshot();

    case "snooze_break":
      mockSnapshot.state = "working";
      mockSnapshot.breakRemainingSeconds = null;
      mockSnapshot.breakTotalSeconds = null;
      mockSnapshot.today.breakSnoozedCount += 1;
      return snapshot();

    // ── 暂停 / 恢复 ──
    case "pause_tracking":
      mockSnapshot.state = "away";
      return snapshot();

    case "resume_tracking":
      mockSnapshot.state = "working";
      return snapshot();

    // ── 勿扰 ──
    case "set_do_not_disturb":
      mockSnapshot.doNotDisturb = argBool(args, "enabled");
      return snapshot();

    // ── Intent ──
    case "capture_intent": {
      const text = argString(args, "text");
      const intent = { id: 1, text, createdAtMs: now };
      mockSnapshot.pendingIntent = intent;
      return intent as unknown as T;
    }

    case "preview_reminder":
      return mockSnapshot.lastDecision as unknown as T;

    // ── 保存偏好 ──
    //
    // 这里真的写进 mockPreferences，而不是「假装成功」。
    // 如果只是返回 undefined，开发时改完设置再进来会看到旧值，
    // 会让人以为保存逻辑坏了 —— 而实际可能是好的。
    case "save_preferences": {
      const incoming = args?.preferences;
      if (incoming && typeof incoming === "object") {
        Object.assign(mockPreferences, incoming);

        // 勿扰状态在快照里也有一份（面板上要显示），同步过去
        if (typeof mockPreferences.doNotDisturb === "boolean") {
          mockSnapshot.doNotDisturb = mockPreferences.doNotDisturb;
        }
      }
      return undefined as unknown as T;
    }

    // ── 无返回值的命令 ──
    //
    // `dismiss_break`：幕布上的动作。浏览器预览里没有副屏也没有幕布窗口，
    // 收到这个调用说明有人手动触发了 —— 忽略即可。
    case "open_settings_window":
    case "close_current_window":
    case "resize_panel":
    case "dismiss_break":
      return undefined as unknown as T;

    // 浏览器预览里打开 Release 页：直接开新标签页，正是用户期待的
    case "open_release_page": {
      const url = argString(args, "url");
      if (url) window.open(url, "_blank", "noopener");
      return undefined as unknown as T;
    }

    // ── 应用更新 ──
    //
    // 浏览器预览里没有更新器（`isTauri()` 为 false 时走的正是这里），
    // 所以只能给一个**明确的**答复，而不是假装成功：
    // 假装成功会让开发时以为「检查更新」这条路径通了，实际上从没连通。
    case "check_update":
      throw new Error("浏览器预览模式下没有更新器，请在打包后的应用里检查更新");

    case "install_update":
      throw new Error("浏览器预览模式下没有更新器，无法安装更新");

    // ── 开机自启 ──
    //
    // 这里**可以**给出有意义的假数据：自启状态是一个偏布尔值，
    // 在浏览器里点一下开关能看到界面反馈（这就是它存在的意义）。
    // 真正的写入由 Rust 侧负责，浏览器里只改这份内存状态。
    case "get_autostart_enabled":
      return mockAutostart as unknown as T;

    case "set_autostart_enabled":
      mockAutostart = argBool(args, "enabled");
      return undefined as unknown as T;

    default:
      throw new Error(`未实现的假命令：${command}`);
  }
}
