/**
 * 菜单栏下拉主面板。
 *
 * 这是用户日常看到最多的界面（对应原型 05 屏），设计目标只有一个：
 * **四类健康状态一眼可见，常用操作一步可达**。
 *
 * ## 几个刻意的设计决定
 *
 * 1. **「现在休息」是唯一的主按钮**，其余操作都是次要的。休息是产品的
 *    核心闭环，不该被「+1 杯水」这类高频小操作抢走视觉重心。
 * 2. **卡片只在「快到点」时才有一条细提示条**。时刻显示进度条会让面板
 *    看起来像一个仪表盘，那是「系统在监控你」的感觉。
 * 3. **底部操作区保持文字级**（设置 / 暂停 / 退出）——低频入口，
 *    不该占用注意力。
 */

import { useCallback, useEffect } from "react";

import * as api from "../api";
import { useCommand, useTacet } from "../hooks/useTacet";
import { NEED_META, formatDuration, type NeedKind } from "../types";
import "./Panel.css";

interface NeedCardProps {
  kind: NeedKind;
  /** 需求强度 0~1。 */
  score: number;
  /** 用户设定的提醒间隔（分钟）。 */
  intervalMinutes: number;
  /** 距离上次满足过了多久；没有记录时为 null。 */
  minutesAgo: number | null;
  /** 这一类提醒是否被用户关掉了。 */
  enabled: boolean;
  /** 点击卡片的动作（记录 / 直接开始）。 */
  onAction?: () => void;
  /** 卡片上显示的动作文案。 */
  actionLabel?: string;
}

/** 一张健康状态卡。 */
function NeedCard({
  kind,
  score,
  intervalMinutes,
  minutesAgo,
  enabled,
  onAction,
  actionLabel,
}: NeedCardProps) {
  const meta = NEED_META[kind];

  // 「距提醒还有多久」比「需求 72%」对用户有用得多 ——
  // 百分比需要换算，而「18 分钟」是立刻能理解的。
  const remainingMinutes =
    minutesAgo === null
      ? intervalMinutes
      : Math.max(0, intervalMinutes - minutesAgo);

  const isDone = minutesAgo !== null && score < 0.2;
  const isDue = score >= 0.75;

  let status: string;
  if (!enabled) {
    status = "已关闭";
  } else if (minutesAgo === null) {
    status = `${intervalMinutes} 分钟后提醒`;
  } else if (isDone) {
    status = `${formatDuration(minutesAgo)}前`;
  } else if (isDue) {
    status = "该提醒了";
  } else {
    status = `距提醒 ${remainingMinutes} 分钟`;
  }

  const className = [
    "card",
    "need-card",
    isDone && enabled ? "need-card-done" : "",
    isDue && enabled ? "need-card-due" : "",
    enabled && onAction ? "card-interactive" : "",
  ]
    .filter(Boolean)
    .join(" ");

  return (
    <div
      className={className}
      onClick={enabled ? onAction : undefined}
      role={enabled && onAction ? "button" : undefined}
      tabIndex={enabled && onAction ? 0 : undefined}
      title={enabled && actionLabel ? actionLabel : undefined}
      onKeyDown={(event) => {
        if (!enabled || !onAction) return;
        if (event.key === "Enter" || event.key === " ") {
          event.preventDefault();
          onAction();
        }
      }}
    >
      <div className={`tile tile-${meta.category}`}>{meta.icon}</div>

      <div className="need-card-body">
        <div className="need-card-name">
          {meta.label}
          {enabled && onAction ? (
            <span className="need-card-action">{actionLabel}</span>
          ) : null}
        </div>
        <div className="need-card-status numeric">{status}</div>
      </div>
    </div>
  );
}

export function Panel() {
  const { snapshot, source, error } = useTacet();
  const { run, busy } = useCommand();

  const act = useCallback(
    (action: () => Promise<unknown>) => {
      void run(action);
    },
    [run],
  );

  /**
   * Esc 收起面板。
   *
   * 这是菜单栏应用的通行为：按 Esc 的意思是「我不想看这个了」。
   * 面板在窗口失焦时也会自动隐藏，但用户如果一直没点别处，
   * 就只能再去菜单栏点一次图标 —— Esc 给了一个更快的出口。
   */
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      void api.closeCurrentWindow();
    };

    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  if (source === "loading" && !snapshot) {
    return (
      <div className="panel panel-shell">
        <div className="panel-loading sub">正在读取状态…</div>
      </div>
    );
  }

  if (source === "error" && !snapshot) {
    return (
      <div className="panel panel-shell">
        <div className="panel-loading">
          <div className="kicker">连接不上后台</div>
          <p className="sub" style={{ marginTop: 8 }}>
            {error ?? "未知错误"}
          </p>
          <p className="sub" style={{ marginTop: 4 }}>
            计时数据不会丢失，应用重启后会自动恢复。
          </p>
        </div>
      </div>
    );
  }

  if (!snapshot) return null;

  const { state, continuousWorkMinutes, needs, reminders } = snapshot;

  // 连续工作时长的显示：休息中就不该再显示「连续工作」了。
  const headline =
    state === "breaking"
      ? { label: "休息中", value: "—" }
      : state === "away"
        ? { label: "已暂停", value: "—" }
        : {
            label: "连续工作",
            value: continuousWorkMinutes >= 60
              ? `${Math.floor(continuousWorkMinutes / 60)}h ${continuousWorkMinutes % 60}m`
              : `${continuousWorkMinutes}m`,
          };

  return (
    <div className="panel panel-shell">
      {/* 状态头 */}
      <header className="panel-header">
        <div className="panel-brand">
          <div className="tile tile-rest panel-logo" aria-hidden>
            ♪
          </div>
          <div className="panel-title">Tacet</div>
          {snapshot.doNotDisturb ? (
            <span className="panel-dnd" title="勿扰模式已开启">
              勿扰
            </span>
          ) : null}
        </div>

        <div className="panel-headline">
          <div className="panel-headline-label">{headline.label}</div>
          <div className="panel-headline-value numeric">{headline.value}</div>
        </div>
      </header>

      <hr className="hair" />

      {/* 四类健康状态 */}
      <div className="panel-grid">
        <NeedCard
          kind="rest"
          score={needs.rest}
          intervalMinutes={reminders.rest.intervalMinutes}
          minutesAgo={continuousWorkMinutes > 0 ? continuousWorkMinutes : null}
          enabled={reminders.rest.enabled}
          actionLabel="现在休息"
          onAction={() => act(api.startBreak)}
        />
        <NeedCard
          kind="hydration"
          score={needs.hydration}
          intervalMinutes={reminders.hydration.intervalMinutes}
          minutesAgo={snapshot.lastWaterMinutesAgo}
          enabled={reminders.hydration.enabled}
          actionLabel="+1 杯"
          onAction={() => act(api.logWater)}
        />
        <NeedCard
          kind="movement"
          score={needs.movement}
          intervalMinutes={reminders.movement.intervalMinutes}
          minutesAgo={snapshot.lastActivityMinutesAgo}
          enabled={reminders.movement.enabled}
          actionLabel="打卡"
          onAction={() => act(api.logActivity)}
        />
        <NeedCard
          kind="eyeRest"
          score={needs.eyeRest}
          intervalMinutes={reminders.eyeRest.intervalMinutes}
          minutesAgo={snapshot.lastEyeRestMinutesAgo}
          enabled={reminders.eyeRest.enabled}
          actionLabel="远眺过了"
          onAction={() => act(api.logEyeRest)}
        />
      </div>

      {/* 操作区 */}
      <div className="panel-actions">
        {state === "breaking" ? (
          <button
            className="btn btn-primary panel-main-action"
            onClick={() => act(api.endBreak)}
            disabled={busy}
          >
            结束休息
          </button>
        ) : (
          <button
            className="btn btn-primary panel-main-action"
            onClick={() => act(api.startBreak)}
            disabled={busy}
          >
            现在休息
          </button>
        )}

        <div className="panel-sub-actions">
          <button
            className="btn btn-quiet panel-sub-action"
            onClick={() => act(api.logWater)}
            disabled={busy}
          >
            +1 杯水
          </button>
          <button
            className="btn btn-quiet panel-sub-action"
            onClick={() => act(api.logActivity)}
            disabled={busy}
          >
            活动打卡
          </button>
        </div>
      </div>

      <hr className="hair" />

      {/* 底部（低频入口） */}
      <footer className="panel-footer">
        <button
          className="btn btn-ghost"
          onClick={() => {
            void api.openSettingsWindow();
          }}
        >
          设置
        </button>
        <span className="panel-footer-dot">·</span>

        {state === "away" ? (
          <button
            className="btn btn-ghost"
            onClick={() => act(api.resumeTracking)}
            disabled={busy}
          >
            继续计时
          </button>
        ) : (
          <button
            className="btn btn-ghost"
            onClick={() => act(api.pauseTracking)}
            disabled={busy}
          >
            暂停计时
          </button>
        )}

        <span className="panel-footer-dot">·</span>

        <button
          className="btn btn-ghost"
          onClick={() => {
            void api.setDoNotDisturb(!snapshot.doNotDisturb);
          }}
          disabled={busy}
        >
          {snapshot.doNotDisturb ? "恢复提醒" : "勿扰"}
        </button>
      </footer>
    </div>
  );
}
