/**
 * 今日统计。
 *
 * ## v0.1 的定位：只有原始数据，没有花哨的可视化
 *
 * 路线图把「统计页面（Dashboard / 周视图 / 趋势）」明确划到 v0.3，
 * v0.1 的交付物是「只有原始数据」。所以这一屏刻意做得很朴素：
 * 几个数字，没有图表。
 *
 * 为什么还是做了这一屏而不是完全不做：用户需要能**验证**这个工具到底
 * 记了什么。一个只看得到「连续工作 1h32m」却不给任何回看入口的工具，
 * 会让人怀疑它在偷偷记录别的东西 —— 透明本身就是产品原则。
 *
 * ## 数字口径
 *
 * 所有这些数字都由 Rust 侧算好（数据模型 §8 的工程约束：
 * UI 层禁止自行计算日期边界）。这里只负责显示。
 *
 * ## 一个文案细节
 *
 * 「接受率」没有数据时显示「暂无数据」而不是「0%」。
 * 后者看起来像在指责用户，前者只是陈述事实（PRD §5 数据展示规范）。
 */

import { useTacet } from "../hooks/useTacet";
import { formatDuration } from "../types";
import "./Today.css";

interface StatProps {
  label: string;
  value: string;
  hint?: string;
}

/** 一个统计数字。 */
function Stat({ label, value, hint }: StatProps) {
  return (
    <div className="stat">
      <div className="stat-label">{label}</div>
      <div className="stat-value numeric">{value}</div>
      {hint ? <div className="stat-hint sub">{hint}</div> : null}
    </div>
  );
}

export function Today() {
  const { snapshot } = useTacet();

  if (!snapshot) {
    return (
      <div className="panel today-shell">
        <div className="today-loading sub">正在读取今天的记录…</div>
      </div>
    );
  }

  const { today, state, continuousWorkMinutes } = snapshot;

  // 接受率：没有数据时给「暂无数据」，而不是 0%
  const acceptance =
    today.acceptanceRate === null
      ? "暂无数据"
      : `${Math.round(today.acceptanceRate * 100)}%`;

  const remindingCount =
    today.breakCompletedCount + today.breakSkippedCount + today.breakSnoozedCount;

  return (
    <div className="panel today-shell">
      {/* 同设置页：这一条兼作窗口的拖动把手（Overlay 标题栏把原生那条盖住了）。 */}
      <header className="today-header" data-tauri-drag-region="deep">
        <div className="kicker">今天</div>
        <h1 className="today-title">
          {state === "breaking"
            ? "正在休息"
            : state === "away"
              ? "暂时离开了"
              : continuousWorkMinutes > 0
                ? `已经连续工作 ${formatDuration(continuousWorkMinutes)}`
                : "还在开始阶段"}
        </h1>
      </header>

      <div className="today-scroll">
        <section className="today-section">
          <div className="kicker today-section-title">节奏</div>
          <div className="today-grid">
            <Stat
              label="累计工作"
              value={formatDuration(today.workMinutes)}
            />
            <Stat
              label="最长连续"
              value={formatDuration(today.longestStreakMinutes)}
            />
          </div>
        </section>

        <section className="today-section">
          <div className="kicker today-section-title">照顾自己</div>
          <div className="today-grid">
            <Stat label="喝水" value={`${today.waterCount} 次`} />
            <Stat label="起身活动" value={`${today.activityCount} 次`} />
          </div>
        </section>

        <section className="today-section">
          <div className="kicker today-section-title">提醒与回应</div>
          <div className="today-grid">
            <Stat label="完成休息" value={`${today.breakCompletedCount} 次`} />
            <Stat
              label="接受率"
              value={acceptance}
              hint={
                remindingCount > 0
                  ? `今天提醒过 ${remindingCount} 次`
                  : undefined
              }
            />
          </div>
        </section>

        {/* 跳过和延后单独列，且文案中性。
            跳过是一个正常的答案，不是需要被纠正的行为。 */}
        <section className="today-section">
          <div className="today-detail-row">
            <span className="sub">跳过 {today.breakSkippedCount} 次</span>
            <span className="today-detail-dot">·</span>
            <span className="sub">延后 {today.breakSnoozedCount} 次</span>
          </div>
          <p className="sub today-note">
            这些记录只用于决定什么时候该少说两句，不会变成任何形式的「评分」。
          </p>
        </section>
      </div>
    </div>
  );
}
