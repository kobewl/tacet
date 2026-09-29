/**
 * 首次启动引导（v0.2.2）。
 *
 * ## 为什么要有它
 *
 * Tacet 会在到点时铺满整块屏幕。一个从没听说过这件事的人，
 * 第一次被整屏盖住时的反应是「这是什么东西」，而不是「好，歇一下」。
 * 所以在第一次打扰之前先打个招呼：会发生什么、数据去哪、怎么让它安静。
 *
 * ## 分寸
 *
 * - 最多三页，每页一件事；任何一页都能跳过，红叉也算跳过
 * - 不要权限、不要填表、不要设目标 —— 那些都是任务，而引导不该布置任务
 * - 文案只说事实，不说「你应该」
 */
import { useState } from "react";
import * as api from "../api";
import "./Welcome.css";

interface Page {
  kicker: string;
  title: string;
  lines: string[];
}

const PAGES: Page[] = [
  {
    kicker: "你好",
    title: "Tacet 平时安静待在菜单栏",
    lines: [
      "连续工作一阵后，它会铺满屏幕，请你歇几分钟。喝水、活动、远眺到点了也会提醒。",
      "每次都可以「稍后」或「跳过」，不需要理由。",
    ],
  },
  {
    kicker: "数据",
    title: "记录只留在这台 Mac 上",
    lines: [
      "没有账号，不上传。不读取屏幕内容，不截屏，不录音。",
      "想带走的话，设置里可以导出成 CSV。",
    ],
  },
  {
    kicker: "安静",
    title: "想让它闭嘴时",
    lines: [
      "点菜单栏图标：「勿扰」停掉提醒，「暂停计时」连计时一起停。",
      "提醒间隔在设置里调。",
    ],
  },
];

export function Welcome() {
  const [index, setIndex] = useState(0);
  const [closing, setClosing] = useState(false);

  // index 只在 0..PAGES.length-1 之间走，兜底只为满足类型检查
  const page = PAGES[index] ?? PAGES[0]!;
  const isLast = index === PAGES.length - 1;

  /** 看完和跳过是同一件事：记下来、关窗，以后不再出现。 */
  const finish = () => {
    if (closing) return;
    setClosing(true);
    void api.completeOnboarding().catch(() => {
      // 记不下来也要让用户走：留在这一页等他再点一次，
      // 比把人困在引导里强。
      setClosing(false);
    });
  };

  return (
    <div className="panel welcome-shell">
      {/* Overlay 标题栏把原生拖动条盖住了，这一条兼作拖动把手（同设置页）。 */}
      <div className="welcome-drag" data-tauri-drag-region="deep" />

      <main className="welcome-body" aria-live="polite">
        <div className="kicker">{page.kicker}</div>
        <h1 className="welcome-title">{page.title}</h1>
        {page.lines.map((line) => (
          <p className="welcome-line" key={line}>
            {line}
          </p>
        ))}
      </main>

      <footer className="welcome-footer">
        <div
          className="welcome-dots"
          aria-label={`第 ${index + 1} 页，共 ${PAGES.length} 页`}
        >
          {PAGES.map((item, dot) => (
            <span
              key={item.kicker}
              className={`welcome-dot${dot === index ? " is-current" : ""}`}
              aria-hidden
            />
          ))}
        </div>

        <div className="welcome-actions">
          {!isLast ? (
            <button className="btn btn-ghost" onClick={finish} disabled={closing}>
              跳过
            </button>
          ) : null}
          <button
            className="btn btn-primary"
            // 换页后焦点仍留在主按钮上，回车可以一路按到底
            autoFocus
            disabled={closing}
            onClick={() => (isLast ? finish() : setIndex(index + 1))}
          >
            {isLast ? "知道了" : "下一步"}
          </button>
        </div>
      </footer>
    </div>
  );
}
