/**
 * 应用入口 —— 按窗口类型分发到不同的界面。
 *
 * 见 `main.tsx` 的说明：一个入口、四种窗口，靠 URL 查询参数区分。
 */

import { BreakErrorBoundary } from "./views/BreakErrorBoundary";
import { BreakFlow } from "./views/BreakFlow";
import { BreakVeil } from "./views/BreakVeil";
import { Panel } from "./views/Panel";
import { Settings } from "./views/Settings";
import { Today } from "./views/Today";

/** 从 URL 里读出窗口类型。 */
function readView(): string {
  if (typeof window === "undefined") return "panel";

  const params = new URLSearchParams(window.location.search);
  return params.get("view") ?? "panel";
}

export function App() {
  const view = readView();

  switch (view) {
    case "break":
      // 包一层错误边界：这个窗口是全屏无边框的，一旦渲染崩掉，
      // 用户会面对一片没有出口的空白。见 BreakErrorBoundary 的说明。
      return (
        <BreakErrorBoundary>
          <BreakFlow />
        </BreakErrorBoundary>
      );
    case "veil":
      // 副屏上的幕布：只挡视线 + 报时间，不含任何操作。
      // 见 views/BreakVeil.tsx 里的说明。
      return <BreakVeil />;
    case "settings":
      return <Settings />;
    case "today":
      return <Today />;
    case "panel":
      return <Panel />;
    default:
      // 认不出的参数：给出明确提示而不是白屏。
      // 白屏是调试成本最高的一种失败。
      return (
        <div className="panel" style={{ padding: 24 }}>
          <div className="kicker">Tacet</div>
          <p className="sub" style={{ marginTop: 8 }}>
            未知的窗口类型：{view}
          </p>
        </div>
      );
  }
}
