/**
 * 幕布 —— 盖住「其它屏幕」的那一层。
 *
 * ## 它为什么存在
 *
 * 只盖住一块屏的全屏休息是假的：用户把鼠标移到另一块屏就能继续干活，
 * 休息被绕过去了。产品承诺的是真的让人停下来，那就得把每一块屏都算进去。
 *
 * ## 为什么副屏不显示一套完整的休息界面
 *
 * 两个理由，都很实际：
 *
 * 1. **状态会打架**：四个窗口各自维护自己的阶段（询问 / 填写待办 /
 *    休息中 / 已结束）。用户在副屏点了「跳过」，主屏还停在询问态 ——
 *    界面互相打脸，而且后端会收到两次相互矛盾的操作。
 * 2. **注意力被摊薄**：用户不知道该看哪块屏，视线在屏幕之间跳。
 *    而这一屏的全部目的是让他**离开**屏幕。
 *
 * 所以副屏只做两件事：**挡住视线**，以及**告诉他还要多久**。
 * 所有操作集中在主屏那一处。
 *
 * ## 但它不是一堵墙
 *
 * 「永不困住用户」是产品的硬约束。所以幕布上**单击**就能把整个休息流程
 * 收起来 —— 效果和用户在主窗口按 Esc 完全一样。
 *
 * 具体做什么由主窗口按当前阶段决定（详见 `commands::dismiss_break`）：
 * 询问阶段是普通关闭，休息中则是提前结束。幕布不知道也不该知道这些区别。
 *
 * ## 文案为什么跟着状态变
 *
 * 幕布会读快照里的 `state`：
 *
 * - `breaking`（正在休息）→ 显示倒计时，出口写成「提前结束」
 * - 其它（提醒刚弹出、用户还没决定）→ 显示「先歇一会儿」，
 *   并告诉他休息界面在哪块屏上
 *
 * 不这么做的话，提醒刚弹出来就写「提前结束」是在替用户做决定 ——
 * 而他此刻甚至还没同意要休息。反过来说，休息中却不显示倒计时，
 * 用户在副屏上就完全不知道还剩多久，只能凭感觉猜。
 *
 * ## 关于 Esc：为什么这里没有键盘监听
 *
 * 直觉上这里应该也监听 Esc，但**监听不到** —— macOS 上幕布窗口是用
 * `focusable(false)` 创建的（原因见 `windows.rs`：它绝不能抢走
 * 用户正在打字的焦点）。一个永远不会成为 key window 的窗口
 * 收不到任何键盘事件。
 *
 * 不过这不妨碍 Esc 有效：键盘焦点仍在主窗口上，用户按 Esc 由
 * **主窗口**的处理器接管，走的还是同一条 `dismiss` 逻辑。
 * 所以界面上那句「按 Esc 提前结束」是准确的。
 *
 * 我没有在这里留一个永远不触发的监听器 —— 那会误导后来的人以为
 * 「幕布自己处理了 Esc」，进而可能把主窗口那个真正的处理器删掉。
 */

import { useCallback } from "react";

import * as api from "../api";
import { useCountdown, useTacet } from "../hooks/useTacet";
import { formatClock } from "../types";
import "./BreakVeil.css";

export function BreakVeil() {
  const { snapshot } = useTacet();

  const breaking = snapshot?.state === "breaking";
  const remaining = useCountdown(
    breaking ? (snapshot?.breakRemainingSeconds ?? null) : null,
  );

  const dismiss = useCallback(() => {
    void api.dismissBreak();
  }, []);

  return (
    <div
      className="break-veil"
      role="button"
      tabIndex={0}
      onClick={dismiss}
      onKeyDown={(event) => {
        // 幕布本身可以聚焦（要能被键盘用户点到），回车等于单击。
        if (event.key === "Enter" || event.key === " ") {
          event.preventDefault();
          dismiss();
        }
      }}
    >
      <div className="veil-content">
        <div className="kicker veil-kicker">
          {breaking ? "休息中" : "休息提醒"}
        </div>

        {breaking && remaining !== null ? (
          <div className="veil-count numeric">{formatClock(remaining)}</div>
        ) : (
          <div className="veil-title">先歇一会儿</div>
        )}

        <p className="veil-hint">
          {breaking
            ? "点击任意处，或按 Esc 提前结束"
            : "休息界面在另一块屏幕上"}
        </p>
      </div>

      {/* 和主窗口一致的角落提示 —— 键盘出口必须可发现，否则等于没有 */}
      <span className="veil-esc-hint" aria-hidden>
        Esc
      </span>
    </div>
  );
}
