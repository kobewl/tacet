import { describe, expect, it } from "vitest";

import { isOrphanRestingUi } from "./restingState";

describe("isOrphanRestingUi", () => {
  it("后端已结束、前端还停在休息中时判定为残骸", () => {
    expect(
      isOrphanRestingUi({
        stage: "resting",
        workState: "away",
        breakTotalSeconds: null,
        remaining: null,
      }),
    ).toBe(true);
  });

  it("正在休息时不是残骸", () => {
    expect(
      isOrphanRestingUi({
        stage: "resting",
        workState: "breaking",
        breakTotalSeconds: 300,
        remaining: 280,
      }),
    ).toBe(false);
  });

  it("本地倒计时还在走时先不收（等快照对齐）", () => {
    expect(
      isOrphanRestingUi({
        stage: "resting",
        workState: "working",
        breakTotalSeconds: null,
        remaining: 300,
      }),
    ).toBe(false);
  });

  it("询问阶段不是残骸", () => {
    expect(
      isOrphanRestingUi({
        stage: "ask",
        workState: "working",
        breakTotalSeconds: null,
        remaining: null,
      }),
    ).toBe(false);
  });
});
