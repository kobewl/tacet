/**
 * 今日统计。
 *
 * ## 定位：朴素地透明
 *
 * v0.1 的交付物是「只有原始数据」；v0.2 补上了「最近 7 天」的小柱状图，
 * 但尺度依然刻意压着：一根柱子看节奏，没有趋势线、没有对比环。
 * 做这一屏的根本原因是：用户需要能**验证**这个工具到底记了什么。
 * 一个只看得到「连续工作 1h32m」却不给任何回看入口的工具，
 * 会让人怀疑它在偷偷记录别的东西 —— 透明本身就是产品原则。
 *
 * ## 数字口径
 *
 * 所有这些数字都由 Rust 侧算好（数据模型 §8 的工程约束：
 * UI 层禁止自行计算日期边界，柱高分母这类纯展示换算除外）。
 * 这里只负责显示。
 *
 * ## 一个文案细节
 *
 * 「接受率」没有数据时显示「暂无数据」而不是「0%」。
 * 后者看起来像在指责用户，前者只是陈述事实（PRD §5 数据展示规范）。
 */

import { useTacet } from "../hooks/useTacet";
import { formatDuration } from "../types";
import "./Today.css";

/** 周几的显示字。下标即 Rust 侧的 weekday：0 = 周一。 */
const WEEKDAY_LABELS = ["一", "二", "三", "四", "五", "六", "日"];

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

  const { today, week, state, continuousWorkMinutes } = snapshot;

  // 柱高的分母：至少 60 分钟 —— 否则「某天只干了 10 分钟」会让柱子看不见，
  // 而「几乎没工作」和「没有数据」是两种不同的信息
  const weekMax = Math.max(60, ...week.days.map((day) => day.workMinutes));

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
              hint="当天各段工作时间之和，离开超阈值才断开"
            />
            <Stat
              label="最长连续"
              value={formatDuration(today.longestStreakMinutes)}
              hint="当天最长的一段工作时间"
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

        {/* 最近 7 天 —— 日期、周几、「今天」的判定都在 Rust 侧算好，
            这里只负责画柱子。滚动 7 天而不是「本周」：周一看「本周」几乎是空的。 */}
        <section className="today-section">
          <div className="kicker today-section-title">最近 7 天</div>
          <div
            className="week-chart"
            role="img"
            aria-label={`最近 7 天累计工作 ${formatDuration(week.workMinutes)}，喝水 ${week.waterCount} 次，完成休息 ${week.breakCompletedCount} 次`}
          >
            {week.days.map((day) => (
              <div
                className={day.isToday ? "week-col week-today" : "week-col"}
                key={day.date}
              >
                <div className="week-bar-track">
                  <div
                    className="week-bar"
                    style={{
                      height: `${Math.round((day.workMinutes / weekMax) * 100)}%`,
                    }}
                  />
                </div>
                <div className="sub week-day-label">
                  {day.isToday ? "今天" : `周${WEEKDAY_LABELS[day.weekday]}`}
                </div>
              </div>
            ))}
          </div>
          <div className="today-detail-row">
            <span className="sub">工作 {formatDuration(week.workMinutes)}</span>
            <span className="today-detail-dot">·</span>
            <span className="sub">喝水 {week.waterCount} 次</span>
            <span className="today-detail-dot">·</span>
            <span className="sub">完成休息 {week.breakCompletedCount} 次</span>
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
