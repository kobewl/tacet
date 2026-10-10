/**
 * reasonText 的护栏测试。
 *
 * 背景（v0.1.3 修复）：Rust 侧 NeedKind 用 snake_case 序列化（"eye_rest"），
 * 而前端 NEED_META 的键是 camelCase（"eyeRest"）。护眼类的「刚提醒过」理由
 * 渲染时取到 undefined.label 抛 TypeError，触发休息界面的整屏兜底错误页。
 * rest / hydration / movement 三种写法两种口径恰好相同，所以只有护眼会崩。
 */
import { describe, expect, it } from "vitest";
import {
  dndRemainingText,
  formatMinuteOfDay,
  parseMinuteOfDay,
  reasonText,
  snoozeStreakNote,
  waterProgress,
  type NeedKind,
} from "./types";

/** 后端真实吐出的 snake_case 口径，前端类型里并不存在这个值。 */
const backendEyeRest = "eye_rest" as NeedKind;

describe("reasonText 口径护栏", () => {
  it("护眼类限流理由（后端 eye_rest 口径）能正常渲染", () => {
    const text = reasonText({
      reason: "rate_limited",
      kind: backendEyeRest,
      minutes_ago: 5,
    });
    expect(text).toBe("5 分钟前刚提醒过护眼");
  });

  it("护眼类「需求百分比」理由（后端 eye_rest 口径）能正常渲染", () => {
    const text = reasonText({
      reason: "need_below_threshold",
      kind: backendEyeRest,
      percent: 32,
    });
    expect(text).toBe("护眼需求 32%，暂时不需要提醒");
  });

  it("完全未知的需求类型也不崩溃，退化成显示原始值", () => {
    const text = reasonText({
      reason: "rate_limited",
      kind: "focus" as NeedKind,
      minutes_ago: 10,
    });
    expect(text).toBe("10 分钟前刚提醒过focus");
  });

  it("前端自己的 camelCase 口径照常工作", () => {
    const text = reasonText({
      reason: "rate_limited",
      kind: "eyeRest",
      minutes_ago: 5,
    });
    expect(text).toBe("5 分钟前刚提醒过护眼");
  });
});

describe("v0.3 新增的展示辅助", () => {
  it("会议与工作时段的理由有人话", () => {
    expect(reasonText({ reason: "in_meeting" })).toContain("会议");
    expect(reasonText({ reason: "outside_work_hours" })).toContain("工作时段");
  });

  it("时刻与分钟数互相转换", () => {
    expect(formatMinuteOfDay(9 * 60 + 5)).toBe("09:05");
    expect(parseMinuteOfDay("19:30")).toBe(19 * 60 + 30);
    expect(parseMinuteOfDay("25:00")).toBeNull();
    expect(parseMinuteOfDay("abc")).toBeNull();
  });

  it("勿扰剩余时间", () => {
    const now = new Date(2026, 0, 1, 10, 0).getTime();
    expect(dndRemainingText(null, now)).toBe("直到你手动关闭");
    expect(dndRemainingText(now + 25 * 60_000, now)).toBe("还剩 25 分钟");
    // 超过 2 小时改说几点
    expect(dndRemainingText(new Date(2026, 0, 2, 8, 0).getTime(), now)).toBe("到 08:00");
  });

  it("喝水进度封顶且不除零", () => {
    expect(waterProgress(4, 8)).toBe(0.5);
    expect(waterProgress(12, 8)).toBe(1);
    expect(waterProgress(3, 0)).toBe(0);
  });

  it("连续延后的文案不带评判，且永远只是文案", () => {
    expect(snoozeStreakNote(0)).toBeNull();
    expect(snoozeStreakNote(1)).toBeNull();
    for (const streak of [2, 3, 6]) {
      const note = snoozeStreakNote(streak);
      expect(note).not.toBeNull();
      for (const banned of ["又", "应该", "必须", "太久", "还没", "竟然", "居然"]) {
        expect(note).not.toContain(banned);
      }
    }
  });
});
