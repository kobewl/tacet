/**
 * 崩溃兜底 —— 保证界面出问题时用户仍然走得掉。
 *
 * ## 为什么这个组件是必须的，而不是「好习惯」
 *
 * 休息界面住在一个**全屏、无边框、常在最前**的窗口里。如果组件树
 * 在渲染时抛错，React 的行为是**卸载整棵树** —— 窗口变成一片空白，
 * 而用户面对的是：
 *
 * - 没有按钮（树没了，按钮也没了）
 * - 按 Esc 没用（监听器挂在被卸载的组件上）
 * - 点任何地方都没反应
 *
 * 除了强退应用，他没有别的出路。这直接违背「永不困住用户」这条硬约束
 * —— 而且是在最不该出事的地方：一个专门用来让人休息、本该让人放松的界面。
 *
 * 所以这里要保证：**即使里面全崩了，外面仍然有一个能用的出口**。
 *
 * ## 它做什么
 *
 * 1. 显示一句说明（明确告诉用户「出错了」，而不是让他对着一片空白猜）
 * 2. 提供一个「关闭」按钮
 * 3. 自己监听 Esc —— 因为里面那些监听器已经随崩溃消失了
 *
 * ## 为什么写成一个类组件
 *
 * React 目前只有类组件能实现 `componentDidCatch` 这种错误边界，
 * 没有函数组件版本的等价物。这是框架的现状，不是风格选择。
 */

import { Component, type ErrorInfo, type ReactNode } from "react";

import * as api from "../api";

interface Props {
  children: ReactNode;
}

interface State {
  failed: boolean;
}

export class BreakErrorBoundary extends Component<Props, State> {
  override state: State = { failed: false };

  static getDerivedStateFromError(): State {
    return { failed: true };
  }

  override componentDidCatch(error: Error, info: ErrorInfo): void {
    // 记到控制台。真实应用里的日志由 Rust 侧写 —— 前端拿不到那个通道，
    // 但用户看到界面上的说明对我们来说更重要。
    console.error("[Tacet] 休息界面渲染失败：", error, info.componentStack);
  }

  /**
   * 兜底逃生通道。
   *
   * 挂在 `window` 上而不是某段 DOM 上：崩溃后页面上已经没有可信的
   * 组件了，唯一还能工作的就是浏览器本身的键盘事件。
   */
  private onKeyDown = (event: KeyboardEvent): void => {
    if (event.key === "Escape") {
      event.preventDefault();
      void api.closeCurrentWindow();
    }
  };

  override componentDidMount(): void {
    window.addEventListener("keydown", this.onKeyDown);
  }

  override componentWillUnmount(): void {
    window.removeEventListener("keydown", this.onKeyDown);
  }

  override render(): ReactNode {
    if (!this.state.failed) {
      return this.props.children;
    }

    return (
      <div className="break-stage">
        <div className="break-content">
          <div className="crash-fallback">
            <div className="kicker">出了点问题</div>
            <p className="crash-text">
              休息界面没能正常显示。你可以先关掉它，不影响已经记录的数据。
            </p>
            <button
              className="btn btn-primary"
              onClick={() => void api.closeCurrentWindow()}
            >
              关闭
            </button>
            <p className="crash-hint sub">也可以按 Esc</p>
          </div>
        </div>
      </div>
    );
  }
}
