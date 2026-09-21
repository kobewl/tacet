/**
 * 与 Rust 侧保持同步的 Hook。
 *
 * ## 数据流（单向，永远只有一个方向）
 *
 * ```text
 *   Rust（业务真值）
 *      │  get_snapshot() 拉一次
 *      │  listen("tacet:snapshot") 持续推送
 *      ▼
 *   React 状态（只读副本）
 *      │  用户点击
 *      ▼
 *   invoke 命令 → Rust 改状态 → 推新快照 → 回到第一步
 * ```
 *
 * 关键点：**前端从不「乐观更新」**。点了「喝了一杯水」之后，
 * 界面不会自己先加一，而是等 Rust 回传新的快照。这样做的代价是
 * 有一瞬间的延迟（实际在本地 IPC 上感觉不到），换来的是
 * **界面上显示的数字永远是真实落库的数字** ——
 * 对于一个记录健康行为的工具，这比「点起来更跟手」重要得多。
 */

import { useCallback, useEffect, useRef, useState } from "react";

import * as api from "../api";
import type { AppSnapshot, TacetEvent } from "../types";

/** 快照的来源，用于决定要不要显示加载态。 */
type Source = "loading" | "live" | "error";

export interface UseTacetResult {
  snapshot: AppSnapshot | null;
  source: Source;
  error: string | null;
  /** 重新拉一次快照。 */
  refresh: () => Promise<void>;
}

/**
 * 持续获取应用快照。
 *
 * @param intervalMs 兜底轮询间隔。即使事件通道正常，也每分钟拉一次 ——
 *   防止某种情况下事件丢了之后界面永远停在旧数据（「显示的数字不再更新」
 *   是最容易被忽略、也最容易让人失去信任的 bug）。
 */
export function useTacet(intervalMs = 60_000): UseTacetResult {
  const [snapshot, setSnapshot] = useState<AppSnapshot | null>(null);
  const [source, setSource] = useState<Source>("loading");
  const [error, setError] = useState<string | null>(null);

  // 用 ref 记住组件是否还挂载着：异步回调回来时组件可能已经被卸载了，
  // 这时候 setState 会触发 React 的警告。
  const mounted = useRef(true);

  const refresh = useCallback(async () => {
    try {
      const next = await api.getSnapshot();
      if (!mounted.current) return;
      setSnapshot(next);
      setSource("live");
      setError(null);
    } catch (err) {
      if (!mounted.current) return;
      setSource("error");
      setError(api.errorMessage(err));
    }
  }, []);

  useEffect(() => {
    mounted.current = true;

    // 先拉一次
    void refresh();

    // 订阅后台推送
    let unlisten: (() => void) | undefined;
    void api
      .listen<TacetEvent>("tacet:event", (event) => {
        if (!mounted.current) return;
        if (event.type === "snapshot") {
          setSnapshot(event.snapshot);
          setSource("live");
          setError(null);
        }
      })
      .then((fn) => {
        unlisten = fn;
      });

    // 兜底轮询
    const timer = window.setInterval(() => {
      void refresh();
    }, intervalMs);

    return () => {
      mounted.current = false;
      unlisten?.();
      window.clearInterval(timer);
    };
  }, [refresh, intervalMs]);

  return { snapshot, source, error, refresh };
}

/**
 * 执行一个会改变状态的命令，并把返回的快照写回来。
 *
 * 用法：
 * ```tsx
 * const run = useCommand();
 * <button onClick={() => run(api.logWater)}>+1 杯水</button>
 * ```
 *
 * 所有写操作都走这里，好处是「调用 → 更新界面 → 出错处理」三件事
 * 只写一次，组件里不必重复。
 */
export function useCommand(): {
  run: <T>(action: () => Promise<T>) => Promise<T | null>;
  busy: boolean;
  error: string | null;
} {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const run = useCallback(async <T,>(action: () => Promise<T>): Promise<T | null> => {
    setBusy(true);
    setError(null);
    try {
      return await action();
    } catch (err) {
      setError(api.errorMessage(err));
      return null;
    } finally {
      setBusy(false);
    }
  }, []);

  return { run, busy, error };
}

/**
 * 一个每秒走一格的计时器，用于倒计时与「距提醒还有多久」。
 *
 * @param active 为 false 时不启动定时器（省电，也避免无意义的渲染）
 */
export function useTicker(active = true): number {
  const [tick, setTick] = useState(0);

  useEffect(() => {
    if (!active) return;

    const timer = window.setInterval(() => {
      setTick((value) => value + 1);
    }, 1000);

    return () => window.clearInterval(timer);
  }, [active]);

  return tick;
}

/** 本地倒计时（从给定秒数开始，每秒减一）。 */
export function useCountdown(initialSeconds: number | null): number | null {
  const [remaining, setRemaining] = useState<number | null>(initialSeconds);

  useEffect(() => {
    setRemaining(initialSeconds);
  }, [initialSeconds]);

  useEffect(() => {
    if (remaining === null || remaining <= 0) return;

    const timer = window.setTimeout(() => {
      setRemaining((value) => (value === null ? null : Math.max(0, value - 1)));
    }, 1000);

    return () => window.clearTimeout(timer);
  }, [remaining]);

  return remaining;
}
