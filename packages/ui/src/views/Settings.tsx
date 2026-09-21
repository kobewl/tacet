/**
 * 设置页（对应原型 08 屏）。
 *
 * ## 两条产品原则在界面上的体现
 *
 * **一、AI 入口静默存在，无红点、无弹窗（原则 7 / ADR-011）**
 *
 * v0.1 里根本没有 AI 功能，所以这一屏连 AI 入口都不出现 ——
 * 一个不存在的能力不该在界面上占位置。等 v0.4 真正实现时，
 * 它会作为「AI 助手（可选）」安静地出现在最后，不配置也不妨碍任何事。
 *
 * **二、能力缺失不是错误（渐进增强原则 6）**
 *
 * 平台能力那一节把「哪些能力在当前系统上不可用」如实列出来，
 * 但**不做任何警告样式**。用户看到的信息是「这台机器不支持会议检测」，
 * 而不是「你的系统有问题」。
 */

import { useEffect, useState } from "react";

import * as api from "../api";
import { useCommand, useTacet } from "../hooks/useTacet";
import type { NeedKind, UpdateInfo, UserPreferences } from "../types";
import { NEED_META } from "../types";
import "./Settings.css";

/** 提醒间隔的可选范围（与 Rust 侧 ReminderRule 的夹取范围一致）。 */
const MIN_INTERVAL = 5;
const MAX_INTERVAL = 240;

/**
 * 常用档位 —— 滑杆旁边的快捷按钮。
 *
 * ## 为什么滑杆和档位要同时存在
 *
 * 只有滑杆：想设成正好 45 分钟要靠拖，很难拖准，体验糟。
 * 只有档位：想设成 37 分钟根本做不到，用户只能将就。
 *
 * 两者并存就都没有这个问题：多数人会直接点档位（一次点击搞定），
 * 少数有具体想法的人可以拖滑杆精确调整。
 *
 * 档位的选取依据：覆盖「常见的工作节奏刻度」——
 * 半小时、三刻钟、一小时、一个半小时、两小时。
 */
const QUICK_STEPS = [20, 30, 45, 60, 90, 120];

/** 分钟数转成人话（「1 小时 30 分」比「90 分钟」好懂）。 */
function describeInterval(minutes: number): string {
  if (minutes < 60) return `${minutes} 分钟`;

  const hours = Math.floor(minutes / 60);
  const rest = minutes % 60;

  if (rest === 0) return `${hours} 小时`;
  return `${hours} 小时 ${rest} 分`;
}

interface ReminderRowProps {
  kind: NeedKind;
  rule: UserPreferences["reminders"][NeedKind];
  onChange: (next: UserPreferences["reminders"][NeedKind]) => void;
}

/** 一行提醒配置：图标 + 名称 + 开关 + 间隔。 */
function ReminderRow({ kind, rule, onChange }: ReminderRowProps) {
  const meta = NEED_META[kind];

  return (
    <div className="setting-row setting-row-reminder">
      <div className="reminder-head">
        <div className={`tile tile-${meta.category}`}>{meta.icon}</div>

        <div className="setting-row-body">
          <div className="setting-row-name">{meta.label}</div>
          <div className="sub">
            {rule.enabled
              ? `每 ${describeInterval(rule.intervalMinutes)}提醒一次`
              : "已关闭这类提醒"}
          </div>
        </div>

        {/* 开关：用 checkbox 实现，保留键盘可操作性与无障碍语义 */}
        <label className="switch">
          <input
            type="checkbox"
            checked={rule.enabled}
            onChange={(event) =>
              onChange({ ...rule, enabled: event.target.checked })
            }
            aria-label={`${meta.label}提醒`}
          />
          <span className="switch-track" aria-hidden>
            <span className="switch-thumb" />
          </span>
        </label>
      </div>

      {rule.enabled ? (
        <div className="reminder-interval">
          <div className="reminder-interval-row">
            {/* 滑杆：自由调节。`step={5}` 是为了让值落在好读的数字上，
                也避免拖出 37 分钟这种「精确但没意义」的值。 */}
            <input
              type="range"
              className="interval-slider"
              min={MIN_INTERVAL}
              max={MAX_INTERVAL}
              step={5}
              value={rule.intervalMinutes}
              onChange={(event) =>
                onChange({
                  ...rule,
                  intervalMinutes: Number(event.target.value),
                })
              }
              aria-label={`${meta.label}提醒间隔`}
              aria-valuetext={describeInterval(rule.intervalMinutes)}
            />
            <span className="interval-value">
              {describeInterval(rule.intervalMinutes)}
            </span>
          </div>

          <div className="interval-presets">
            {QUICK_STEPS.map((minutes) => (
              <button
                key={minutes}
                type="button"
                className={
                  rule.intervalMinutes === minutes
                    ? "interval-preset is-active"
                    : "interval-preset"
                }
                onClick={() =>
                  onChange({ ...rule, intervalMinutes: minutes })
                }
              >
                {describeInterval(minutes)}
              </button>
            ))}
          </div>
        </div>
      ) : null}
    </div>
  );
}

/**
 * 「检查更新」一行。
 *
 * ## 为什么更新是用户主动触发的
 *
 * 一个常驻菜单栏的健康工具在后台偷偷联网，是一件需要向用户解释的事。
 * Tacet 的原则是「不配置任何东西时它也该安静地工作」（原则 7），
 * 所以这里没有自动检查：**点了才查**。
 *
 * ## 状态为什么是一个联合而不是几个 boolean
 *
 * 「正在查」「已是最新」「有新版本」「检查失败」这四种状态是互斥的，
 * 用 `loading` / `hasUpdate` / `error` 三个独立变量表达，会出现
 * 「既在加载又有错误」这种不可能却表示得出来的组合。
 * 一个字段穷举所有情况，界面就不可能显示出矛盾的状态。
 */
type UpdateState =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "up-to-date" }
  | { kind: "available"; info: UpdateInfo }
  | { kind: "installing"; percent: number | null }
  | { kind: "error"; message: string };

function UpdateRow() {
  const [state, setState] = useState<UpdateState>({ kind: "idle" });

  // 订阅下载进度。
  //
  // ## 为什么订阅一次就够，不需要「只在安装时订阅」
  //
  // 安装成功后应用就重启了，这个窗口根本活不到下一次检查更新。
  // 所以订阅常驻是安全的，也让这个 effect 不再依赖任何状态 ——
  // 依赖数组为空，不会因为进度更新而反复订阅（那会让进度条一顿一顿的）。
  //
  // 用函数式 setState 读当前状态：只有正在安装时才采纳进度值。
  // 这样即使有空转的事件进来，也不会把一个「已是最新」的界面
  // 突然变成「正在下载」。
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;

    void api
      .onUpdateProgress((percent) => {
        if (cancelled) return;
        setState((prev) =>
          prev.kind === "installing" ? { kind: "installing", percent } : prev,
        );
      })
      .then((fn) => {
        unlisten = fn;
      });

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);

  const check = () => {
    setState({ kind: "checking" });
    void (async () => {
      try {
        const info = await api.checkUpdate();
        setState(info ? { kind: "available", info } : { kind: "up-to-date" });
      } catch (err) {
        setState({
          kind: "error",
          message: err instanceof Error ? err.message : String(err),
        });
      }
    })();
  };

  const install = () => {
    setState({ kind: "installing", percent: null });
    void (async () => {
      try {
        await api.installUpdate();
        // 正常情况下走不到这里：安装成功后应用会重启，这个 Promise 就断了。
        // 真的返回了说明重启没发生 —— 如实告诉用户，别假装成功。
        setState({
          kind: "error",
          message: "更新已安装，但应用没有自动重启。请手动退出并重新打开 Tacet。",
        });
      } catch (err) {
        setState({
          kind: "error",
          message: err instanceof Error ? err.message : String(err),
        });
      }
    })();
  };

  /** 兜底：到浏览器里下载。 */
  const openReleases = (url: string) => {
    void api.openReleasePage(url);
  };

  const busy = state.kind === "checking" || state.kind === "installing";

  return (
    <div className="update-row">
      <div className="setting-row compact">
        <div className="setting-row-body wide">
          <div className="setting-row-name">软件更新</div>
          <div className="sub">{describeUpdateState(state)}</div>
        </div>

        {state.kind === "available" ? (
          <button className="btn btn-primary" onClick={install}>
            立即更新
          </button>
        ) : state.kind === "installing" ? (
          <span className="update-spinner" aria-hidden />
        ) : (
          <button className="btn btn-quiet" onClick={check} disabled={busy}>
            {state.kind === "checking" ? "检查中…" : "检查更新"}
          </button>
        )}
      </div>

      {/* 有新版本时把发布说明摊开 —— 用户要能看清「这次改了什么」再决定装不装 */}
      {state.kind === "available" ? (
        <UpdateNotes info={state.info} onOpenReleases={openReleases} />
      ) : null}

      {/* 出错时给一条**能自己走下去**的路：不能更新不等于不能用，
          用户至少应该能手动去下载。 */}
      {state.kind === "error" ? (
        <div className="update-actions">
          <button
            className="btn btn-ghost"
            onClick={() => openReleases("https://github.com/kobewl/tacet/releases/latest")}
          >
            前往下载页
          </button>
        </div>
      ) : null}
    </div>
  );
}

/** 把状态翻译成一句给用户看的话。 */
function describeUpdateState(state: UpdateState): string {
  switch (state.kind) {
    case "idle":
      return "检查有没有新版本。只有点这个按钮时才会联网。";
    case "checking":
      return "正在查询…";
    case "up-to-date":
      return "已经是最新版本";
    case "available":
      return `有新版本 ${state.info.version}（当前 ${state.info.currentVersion}）`;
    case "installing":
      return state.percent === null
        ? "正在下载更新…"
        : `正在下载更新… ${state.percent}%`;
    case "error":
      return state.message;
  }
}

/** 新版本的发布说明。「前往下载」是装不了时的退路。 */
function UpdateNotes({
  info,
  onOpenReleases,
}: {
  info: UpdateInfo;
  onOpenReleases: (url: string) => void;
}) {
  return (
    <div className="update-notes">
      {info.notes ? (
        // 发布说明是纯文本（来自 Release body）：不解析 Markdown，
        // 也不用 innerHTML —— 那两者都会把外部内容变成可执行的东西。
        <pre className="update-notes-body">{info.notes}</pre>
      ) : (
        <div className="sub">这个版本没有附发布说明。</div>
      )}

      <button
        className="btn btn-ghost update-notes-link"
        onClick={() => onOpenReleases(info.releaseUrl)}
      >
        查看发布页
      </button>
    </div>
  );
}

export function Settings() {
  const { snapshot } = useTacet();
  const { run, busy } = useCommand();
  const [prefs, setPrefs] = useState<UserPreferences | null>(null);
  /** 进来时的原始偏好，用来判断「有没有改过」。 */
  const [baseline, setBaseline] = useState<UserPreferences | null>(null);
  const [savedAt, setSavedAt] = useState<number | null>(null);
  /** 应用版本，从二进制里读（更新后这里会跟着变）。 */
  const [appVersion, setAppVersion] = useState("…");

  // 首屏读一次偏好
  useEffect(() => {
    void (async () => {
      const loaded = await api.getPreferences();
      setPrefs(loaded);
      setBaseline(loaded);
    })();
    // 只在挂载时读一次：之后的改动都走本地 state + 显式保存
  }, []);

  // 版本号独立读一次。失败不阻塞设置页 —— 显示占位符即可，
  // 一个读不到的版本号不该让整页功能不可用。
  useEffect(() => {
    void api.getAppVersion().then(setAppVersion).catch(() => setAppVersion("未知"));
  }, []);

  if (!prefs) {
    return (
      <div className="panel settings-shell">
        <div className="settings-loading sub">正在读取设置…</div>
      </div>
    );
  }

  // 只有值真的变了才算「有未保存改动」。
  // 用 JSON 比较是够的：这个对象很小、结构固定，且顺序稳定
  // （字段顺序由 UserPreferences 的定义决定，不会因为对象展开而乱）。
  const dirty = baseline !== null && JSON.stringify(prefs) !== JSON.stringify(baseline);

  const update = <K extends keyof UserPreferences>(
    key: K,
    value: UserPreferences[K],
  ) => {
    setPrefs({ ...prefs, [key]: value });
    setSavedAt(null);
  };

  const updateReminder = (
    kind: NeedKind,
    next: UserPreferences["reminders"][NeedKind],
  ) => {
    setPrefs({ ...prefs, reminders: { ...prefs.reminders, [kind]: next } });
    setSavedAt(null);
  };

  const save = () => {
    void run(async () => {
      await api.savePreferences(prefs);
      setBaseline(prefs);
      setSavedAt(Date.now());
    });
  };

  const capabilities = snapshot?.capabilities ?? [];

  return (
    <div className="panel settings-shell">
      <header className="settings-header">
        <div>
          <div className="kicker">设置</div>
          <h1 className="settings-title">提醒节奏</h1>
        </div>
        {snapshot?.doNotDisturb ? (
          <span className="settings-dnd">勿扰中</span>
        ) : null}
      </header>

      <div className="settings-scroll">
        {/* ---------------------------------------------- 四类提醒 */}
        <section className="settings-section">
          <div className="kicker settings-section-title">提醒</div>
          <div className="settings-card">
            {(["rest", "hydration", "movement", "eyeRest"] as const).map(
              (kind) => (
                <ReminderRow
                  key={kind}
                  kind={kind}
                  rule={prefs.reminders[kind]}
                  onChange={(next) => updateReminder(kind, next)}
                />
              ),
            )}
          </div>
          <p className="sub settings-note">
            间隔范围 {MIN_INTERVAL}~{MAX_INTERVAL} 分钟。任何时候都可以跳过一次提醒，
            不会影响统计。
          </p>
        </section>

        {/* ---------------------------------------------- 节奏 */}
        <section className="settings-section">
          <div className="kicker settings-section-title">节奏</div>
          <div className="settings-card">
            <div className="setting-row">
              <div className="setting-row-body wide">
                <div className="setting-row-name">多久算离开</div>
                <div className="sub">
                  连续这么久没有操作，就暂停工作计时
                </div>
              </div>
              <select
                className="setting-select"
                value={prefs.idleThresholdMinutes}
                onChange={(event) =>
                  update("idleThresholdMinutes", Number(event.target.value))
                }
              >
                {[1, 2, 3, 5, 8, 10, 15].map((minutes) => (
                  <option key={minutes} value={minutes}>
                    {minutes} 分钟
                  </option>
                ))}
              </select>
            </div>

            <div className="setting-row">
              <div className="setting-row-body wide">
                <div className="setting-row-name">一次休息多久</div>
                <div className="sub">全屏休息的倒计时长度</div>
              </div>
              <select
                className="setting-select"
                value={prefs.breakDurationMinutes}
                onChange={(event) =>
                  update("breakDurationMinutes", Number(event.target.value))
                }
              >
                {[1, 3, 5, 8, 10, 15].map((minutes) => (
                  <option key={minutes} value={minutes}>
                    {minutes} 分钟
                  </option>
                ))}
              </select>
            </div>
          </div>
        </section>

        {/* ---------------------------------------------- 平台能力 */}
        <section className="settings-section">
          <div className="kicker settings-section-title">这台电脑上的能力</div>
          <div className="settings-card">
            {capabilities.map((capability) => (
              <div className="setting-row compact" key={capability.name}>
                <div className="setting-row-body wide">
                  <div className="setting-row-name">
                    {capability.displayName}
                  </div>
                  {capability.reason ? (
                    <div className="sub">{capability.reason}</div>
                  ) : null}
                </div>
                {/* 可用与否只用文字表达，不用绿色/红色 ——
                    能力缺失不是「错误」，不该用错误色报警 */}
                <span
                  className={
                    capability.available
                      ? "capability-on"
                      : "capability-off"
                  }
                >
                  {capability.available ? "可用" : "不支持"}
                </span>
              </div>
            ))}
          </div>
          <p className="sub settings-note">
            Tacet 不读取窗口标题、不截屏、不录音。选中的这几项能力都只需要公开 API，
            不需要辅助功能或屏幕录制权限。
          </p>
        </section>

        {/* ---------------------------------------------- 关于 */}
        <section className="settings-section">
          <div className="kicker settings-section-title">关于</div>
          <div className="settings-card">
            <div className="setting-row compact">
              <div className="setting-row-body wide">
                <div className="setting-row-name">Tacet</div>
                <div className="sub">
                  版本 {appVersion} · 数据全部保存在本机，没有账号，没有云端
                </div>
              </div>
            </div>

            <UpdateRow />
          </div>

          {/* 未签名这件事必须**明说**，而不是等用户第一次打开时被
              Gatekeeper 拦下来才自己猜。写在这里比写在 README 里有用 ——
              用户装的版本里只有这一个地方能看到。 */}
          <p className="sub settings-note">
            个人测试包，未做 Apple 代码签名。首次打开需要在「系统设置 → 隐私与安全性」中允许；
            更新包经过签名校验，能确认它来自本项目的发布。
          </p>
        </section>
      </div>

      {/* 保存条：只在有未保存改动时出现，避免一个常驻的按钮制造「要不要点它」的疑虑 */}
      {dirty ? (
        <footer className="settings-footer">
          <span className="sub settings-footer-hint">有未保存的改动</span>
          <button className="btn btn-primary" onClick={save} disabled={busy}>
            保存
          </button>
        </footer>
      ) : savedAt ? (
        <footer className="settings-footer settings-footer-quiet">
          <span className="sub">设置已保存</span>
        </footer>
      ) : null}
    </div>
  );
}
