/**
 * 需求图标 —— 细线条、1.4 描边，照搬原型 `_build.py` 里的那一套。
 *
 * ## 为什么不用 emoji
 *
 * 早先主面板和设置页的图标是 ☕💧🧍👁。emoji 自带颜色和立体感，
 * 在「白色毛玻璃 + 低饱和功能色」的界面里像贴上去的贴纸；
 * 而且各系统版本画法不同，☕ 在这套冷色调里是一块突兀的暖棕。
 * 线条图标跟着 `currentColor` 走，底座（`.tile-*`）给它配什么色它就是什么色。
 *
 * 所有图标都是装饰（旁边永远有文字标签），一律 `aria-hidden`。
 */
import type { NeedKind } from "./types";

export function IconCup() {
  return (
    <svg viewBox="0 0 20 20" fill="none" aria-hidden>
      <path
        d="M3.6 5.4h9.2v6.3a3.3 3.3 0 0 1-3.3 3.3H6.9a3.3 3.3 0 0 1-3.3-3.3V5.4Z"
        stroke="currentColor"
        strokeWidth="1.4"
      />
      <path
        d="M12.8 7h1.3a1.9 1.9 0 0 1 0 3.8h-1.3"
        stroke="currentColor"
        strokeWidth="1.4"
        strokeLinecap="round"
      />
      <path
        d="M6.9 2.8v1.4M9.5 2.5v1.7"
        stroke="currentColor"
        strokeWidth="1.4"
        strokeLinecap="round"
      />
    </svg>
  );
}

export function IconDrop() {
  return (
    <svg viewBox="0 0 20 20" fill="none" aria-hidden>
      <path
        d="M10 2.8c2.7 3.1 4.4 5.5 4.4 7.5a4.4 4.4 0 0 1-8.8 0c0-2 1.7-4.4 4.4-7.5Z"
        stroke="currentColor"
        strokeWidth="1.4"
      />
    </svg>
  );
}

export function IconWalk() {
  return (
    <svg viewBox="0 0 20 20" fill="none" aria-hidden>
      <circle cx="11.2" cy="4" r="1.6" stroke="currentColor" strokeWidth="1.4" />
      <path
        d="M11.6 6.9 9 9.3l1.4 2.4-.9 5M11.6 6.9l2 2.9 2.3.8M10.4 11.7 6.5 12.3"
        stroke="currentColor"
        strokeWidth="1.4"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

export function IconEye() {
  return (
    <svg viewBox="0 0 20 20" fill="none" aria-hidden>
      <path
        d="M1.9 10S4.7 5.2 10 5.2 18.1 10 18.1 10 15.3 14.8 10 14.8 1.9 10 1.9 10Z"
        stroke="currentColor"
        strokeWidth="1.4"
      />
      <circle cx="10" cy="10" r="2.3" stroke="currentColor" strokeWidth="1.4" />
    </svg>
  );
}

/** 品牌标识：三笔交叉的「休止」记号（原型 IC_BRAND）。 */
export function IconBrand() {
  return (
    <svg viewBox="0 0 20 20" fill="none" aria-hidden>
      <path
        d="M10 3.2v13.6M4.4 6.6l11.2 6.8M15.6 6.6 4.4 13.4"
        stroke="currentColor"
        strokeWidth="1.3"
        strokeLinecap="round"
      />
    </svg>
  );
}

const NEED_ICONS: Record<NeedKind, () => JSX.Element> = {
  rest: IconCup,
  hydration: IconDrop,
  movement: IconWalk,
  eyeRest: IconEye,
};

/**
 * 按需求类型取图标。后端口径漂移（`eye_rest`）时先归一化，
 * 仍认不出就不画 —— 旁边的文字标签足够说明，界面降级但不崩
 * （同 `needMeta` 的原则，见 types.ts）。
 */
export function NeedIcon({ kind }: { kind: string }) {
  const camel = kind.replace(/_([a-z])/g, (_, c: string) => c.toUpperCase());
  const Icon = NEED_ICONS[kind as NeedKind] ?? NEED_ICONS[camel as NeedKind];
  return Icon ? <Icon /> : null;
}
