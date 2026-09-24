/**
 * 休息界面是否已经和后端脱节。
 *
 * 后端 tick 到点后会清掉 breakTotalSeconds，但前端倒计时在休眠后
 * 可能冻住，界面继续停在「休息中 05:00」。这种情况应当收掉，
 * 而不是继续显示一个已经结束的倒计时。
 *
 * 刚点「开始休息」的那一帧快照可能还没到 —— 那种情况由调用方
 * 用短延迟排除，这里只判断「当前这一帧像不像残骸」。
 */
export function isOrphanRestingUi(input: {
  stage: "ask" | "intent" | "resting" | "done";
  workState: string | undefined;
  breakTotalSeconds: number | null | undefined;
  remaining: number | null;
}): boolean {
  if (input.stage !== "resting") return false;
  if (input.workState === "breaking") return false;
  if (input.breakTotalSeconds != null) return false;
  if (input.remaining != null && input.remaining > 0) return false;
  return true;
}
