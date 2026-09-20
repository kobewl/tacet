/**
 * 前端入口。
 *
 * ## 一个入口，四种窗口
 *
 * Tacet 有四种形态的界面，它们共用同一份前端产物，靠 URL 查询参数区分：
 *
 * | 参数 | 窗口 | 大小 | 说明 |
 * | --- | --- | --- | --- |
 * | （无） | 菜单栏主面板 | 376×520 | 点菜单栏图标弹出 |
 * | `?view=break` | 全屏休息流程 | 全屏 | Intent 记录 → 休息 → 结束 |
 * | `?view=settings` | 设置页 | 520×640 | 独立窗口 |
 *
 * 这样做的好处是**只有一份构建产物、一份依赖、一份样式**。
 * 代价是入口处多了一个分支 —— 相比维护三套 Vite 配置，这个代价很划算。
 */

import { StrictMode } from "react";
import { createRoot } from "react-dom/client";

import { App } from "./App";
import "./styles/global.css";

const container = document.getElementById("root");

if (!container) {
  // 这种情况只可能发生在 index.html 被改坏时。直接报错比白屏好排查。
  throw new Error("找不到 #root 容器，index.html 可能被改动了");
}

createRoot(container).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
