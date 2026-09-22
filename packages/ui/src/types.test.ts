/**
 * reasonText 的护栏测试。
 *
 * 背景（v0.1.3 修复）：Rust 侧 NeedKind 用 snake_case 序列化（"eye_rest"），
 * 而前端 NEED_META 的键是 camelCase（"eyeRest"）。护眼类的「刚提醒过」理由
 * 渲染时取到 undefined.label 抛 TypeError，触发休息界面的整屏兜底错误页。
 * rest / hydration / movement 三种写法两种口径恰好相同，所以只有护眼会崩。
 */
import { describe, expect, it } from "vitest";
import { reasonText, type NeedKind } from "./types";

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
