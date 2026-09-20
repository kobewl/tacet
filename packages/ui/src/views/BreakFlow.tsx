/**
 * 全屏休息流程 —— 产品的核心闭环（PRD 旅程 A）。
 *
 * 四个阶段串成一个状态机：
 *
 * ```text
 *   ask ──「现在休息」──► intent ──提交/跳过──► resting ──倒计时结束──► done
 *    │                      │                     │                    │
 *    │                      │                     │                    └─ 展示 Intent
 *    ├─「略过」─────────────┴──────────────────────┴──────────────────────┘
 *    │                                                    回到工作
 *    └─「3 分钟后」──► 关闭窗口（Rust 侧记 snooze）
 * ```
 *
 * ## 为什么 Intent 在「现在休息」之后问，而不是在提醒卡片上
 *
 * 提醒卡片上的每一秒都在消耗用户的耐心 —— 用户此刻还没决定要休息，
 * 让他先输入一句话是冒犯的。等他点了「现在休息」，说明已经接受这个决定，
 * 这时候问「接下来准备做什么」才是自然的、有价值的。
 *
 * ## 为什么结束后要把 Intent 原样还给用户
 *
 * 打断最贵的代价不是那五分钟，而是回来以后想不起刚才在干嘛（用户故事 US-2）。
 * 所以结束页把 Intent 放在最显眼的位置，且**不催促**
 * —— 按钮文案是「继续」，不是「开始工作」。
 *
 * ## 关于那个呼吸环
 *
 * 内圈做 10 秒一个周期的缩放（4 秒吸 / 6 秒呼，即 6 次/分），幅度很小。
 * 依据是慢呼吸（<10 次/分）能引起自主神经系统的变化、促进放松
 * （Zaccaro 等 2018 年的系统综述）。
 *
 * 需要说明：**这个证据基础不算强**（只有少数综述，没有大规模 RCT），
 * 所以它是设计上的合理选择，不是「临床证实」。
 * 尺度也刻意压得很小 —— 够被余光感知、引导呼吸，但不足以把人留在
 * 屏幕前盯着它看。这与「让用户离开屏幕」的目标是一致的。
 */

import { useCallback, useEffect, useRef, useState } from "react";

import * as api from "../api";
import { useCountdown, useTacet } from "../hooks/useTacet";
import { pickQuote } from "../quotes";
import {
  formatClock,
  reasonText,
  type IntentRecord,
  type TacetEvent,
} from "../types";
import "./BreakFlow.css";

/** 休息流程的阶段。 */
type Stage = "ask" | "intent" | "resting" | "done";

/**
 * 环的半径与周长（用于倒计时进度环）。
 *
 * 116 这个值不是随手取的：它对应直径 260px 的环，加上里面的倒计时数字
 * （68px）之后，与设计原型里「340px 环 / 88px 数字」的比例接近。
 *
 * 之前是 78（环只有 184px），在 1440×900 的屏幕上整组内容缩在中间一小块，
 * 显得单薄。但也不能真按原型的 340px 来 —— 那加上清单卡片和那句话之后
 * 总高度接近 700px，在 1280×800 的笔记本上会顶到边缘。
 * 260px 是两头都顾得上的取值。
 */
const RING_RADIUS = 116;
const RING_CIRCUMFERENCE = 2 * Math.PI * RING_RADIUS;

/**
 * 休息时那句话的出现时机。
 *
 * 10 秒——这个数字是给「行动清单」留的阅读时间：用户刚进入休息，
 * 先看到的是「离开屏幕，看看远处」这类具体动作，鸡汤是之后的事。
 * 先办事、再闲聊，是一个懂分寸的同事的节奏。
 */
const QUOTE_DELAY_MS = 10_000;

/**
 * 之后的换句间隔：一分钟一次。
 *
 * 比这个更勤就变成「内容流」了 —— 那会把人留在屏幕前等着看下一句，
 * 与这一屏的目的相反。
 */
const QUOTE_ROTATE_MS = 60_000;

export function BreakFlow() {
  const { snapshot, refresh } = useTacet();
  const [stage, setStage] = useState<Stage>("ask");
  const [intentText, setIntentText] = useState("");
  const [restoredIntent, setRestoredIntent] = useState<IntentRecord | null>(null);

  // 整个休息流程只允许提交一次 Intent
  const submitted = useRef(false);

  // 本地倒计时，从快照给的剩余秒数起步
  const remaining = useCountdown(snapshot?.breakRemainingSeconds ?? null);

  // 倒计时走完 -> 切到结束页
  useEffect(() => {
    if (stage === "resting" && remaining === 0) {
      void (async () => {
        const intent = await api.endBreak();
        setRestoredIntent(intent);
        setStage("done");
      })();
    }
  }, [stage, remaining]);
  const handleStartBreak = useCallback(async () => {
    await api.startBreak();
    setStage("intent");
    void refresh();
  }, [refresh]);

  const handleSubmitIntent = useCallback(async () => {
    if (submitted.current) return;
    submitted.current = true;

    // 允许空文本 —— 跳过不填是被允许的答案，不是错误。
    const trimmed = intentText.trim();
    if (trimmed.length > 0) {
      await api.captureIntent(trimmed);
    }

    await api.startBreak();
    setStage("resting");
    void refresh();
  }, [intentText, refresh]);

  const handleSkip = useCallback(async () => {
    await api.skipBreak();
    await api.closeCurrentWindow();
  }, []);

  const handleSnooze = useCallback(async (minutes: number) => {
    await api.snoozeBreak(minutes);
    await api.closeCurrentWindow();
  }, []);

  const handleFinish = useCallback(async () => {
    await api.closeCurrentWindow();
  }, []);

  /**
   * 「用户想把这个界面收起来」—— Esc 和副屏幕布共用这一条逻辑。
   *
   * ## 为什么抽出来
   *
   * 两个触发源（键盘 Esc、副屏幕布上的点击）表达的是**同一个意图**，
   * 而「这个意图在当前阶段意味着什么」只该有一份答案。如果各写一份，
   * 迟早会分叉 —— 用户会发现同一件事在两个地方有不同结果。
   *
   * ## 各阶段的含义
   *
   * - `ask`（还没决定休息）→ 只关闭，**不记 skip**。
   *   「关掉提示」和「主动跳过这次休息」是两个意思，记成 skip
   *   会让接受率统计被低估。
   * - `intent`（已开始休息，在填待办）→ 跳过填写，直接进入休息。
   * - `resting`（休息中）→ 提前结束。
   * - `done` → 直接关窗。
   */
  const dismiss = useCallback(() => {
    void (async () => {
      switch (stage) {
        case "ask":
          await api.closeCurrentWindow();
          break;

        case "intent":
          await api.startBreak();
          setStage("resting");
          void refresh();
          break;

        case "resting": {
          const intent = await api.endBreak();
          setRestoredIntent(intent);
          setStage("done");
          break;
        }

        case "done":
          await api.closeCurrentWindow();
          break;
      }
    })();
  }, [stage, refresh]);

  /**
   * Esc 逃生通道 —— 「永不困住用户」的最后一道保险。
   *
   * ## 为什么这个必须有
   *
   * 这个窗口是无边框的、常在最前的，而且会盖住一整块屏幕。
   * 如果用户想让它消失却发现关不掉，那种感觉和「中病毒」没有区别。
   *
   * 界面上一直有「略过」「提前结束」这些按钮，正常情况下点一下就行。
   * 但还有两种情况会让人卡住：
   *
   * 1. **多显示器**：鼠标在另一块屏上，或者用户此刻在用触控板/键盘
   * 2. **窗口抢焦点后的输入法状态异常**：点击没反应（少见但真实存在）
   *
   * Esc 是「关掉当前这个东西」的通用直觉，给它一个确定的出口，
   * 成本只有几行代码，换来的是「任何情况下都关得掉」这个保证。
   */
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape") return;
      event.preventDefault();
      dismiss();
    };

    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [dismiss]);

  /**
   * 副屏幕布上的动作，以及「窗口被重新打开」—— 两条事件。
   *
   * ## `dismiss`
   *
   * 幕布不知道当前在哪个阶段，所以它只发这个意图过来，由上面那个
   * `dismiss` 统一处理。这样「用户想收起来」这件事在全应用只有一份答案。
   *
   * ## `breakShown`
   *
   * 窗口被隐藏时网页**不会被卸载**，所以这个组件还停在上一次离开时的
   * 阶段。上一次休息正常结束后停在 `done`（「欢迎回来」），
   * 下一次提醒弹出来就会错误地显示它 —— 用户看到的是一个「没关掉的旧窗口」。
   *
   * 所以每次窗口被显示出来，都按真实状态重置一次阶段。
   * 判断依据是快照里的 `state`，而不是事件自己带参数：
   * 状态真值只有一份（在 Rust 侧），前端不该有第二份。
   */
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    void api
      .listen<TacetEvent>("tacet:event", (event) => {
        if (event.type === "dismiss") {
          dismiss();
          return;
        }

        if (event.type === "breakShown") {
          void (async () => {
            const next = await api.getSnapshot();
            // `breaking` 说明用户已经同意休息了，该填待办；
            // 否则这是一次全新的提醒，从询问开始。
            setStage(next.state === "breaking" ? "intent" : "ask");
            setIntentText("");
            setRestoredIntent(null);
            // 新一轮休息，允许重新提交一次 Intent
            submitted.current = false;
          })();
        }
      })
      .then((fn) => {
        unlisten = fn;
      });

    return () => unlisten?.();
  }, [dismiss]);

  if (!snapshot) {
    return <div className="break-stage break-loading sub">正在准备…</div>;
  }

  const snoozeOptions = [1, 3, 5];

  /**
   * 进度环的分母：这次休息**计划的总时长**，来自 Rust 侧的快照。
   *
   * ## 这里曾经算错过（用户报「刚开始慢，然后快」）
   *
   * 早期写的是：
   *
   * ```ts
   *   const totalSeconds = snapshot.breakRemainingSeconds ?? 300;
   *   const elapsedRatio = 1 - remaining / Math.max(1, totalSeconds);
   * ```
   *
   * 看着像是「剩余 / 总共」，但 `remaining` 和 `totalSeconds` 其实是
   * **同一个数**（只不过一个在本地每秒减，一个等着快照刷新）。
   * 于是分母跟着分子一起缩小，比例被反复拉回 0：
   *
   * ```text
   *   第  0 秒  比例 0                     环从 0 开始
   *   第  9 秒  比例 9/300  = 3.0%          慢慢爬
   *   第 10 秒  新快照到，分母变成 290
   *            比例 = 1 - 290/290 = 0      整段倒退，1 秒内退完
   *   第 19 秒  比例 9/290  = 3.1%          重新爬
   * ```
   *
   * **每 10 秒重复一次**（调度器的 tick 间隔），而每一次窗口里
   * 环要扫过的比例是 `10 / 剩余秒数` —— 这个数随时间**加速**：
   *
   * | 时间窗口      | 每 10 秒扫过 |
   * |---------------|--------------|
   * | 第   0~ 10 秒 |  3%          |
   * | 第 200~210 秒 |  9%          |
   * | 第 270~280 秒 | 30%          |
   * | 第 290~300 秒 | 90%          |
   *
   * 这就是「刚开始慢，然后快」的全部来源：不是动画曲线的问题，
   * 而是分母本身在缩水，导致越接近结束、环冲得越猛。
   * 再加上 CSS 上挂的是 1 秒过渡，倒退那一下也会被画成一次飞快的滑动。
   *
   * ## 修法
   *
   * 分母换成 `breakTotalSeconds`：它在 `start_break` 那一刻确定，
   * 整段休息里一个数都不变（Rust 侧有测试钉住这一点，见
   * `休息总时长在整段休息里恒定不变`），于是每 10 秒扫过的比例
   * 恒定是 3%，环平稳地线性走到 100%。
   *
   * `?? remaining ?? 300` 是兜底 —— 理论上快照一定带这个字段，
   * 但如果哪天旧版本前端配上了新版本后端（或相反），
   * 也不能让环除以 0 或直接报错。宁可退化成旧行为，也不要白屏。
   */
  const totalSeconds =
    snapshot.breakTotalSeconds ?? snapshot.breakRemainingSeconds ?? 300;

  /**
   * 已经过去多少（0~1），进度环按它画弧。
   *
   * 末尾的钳制是给「比例」这个量本身定的规矩：它是个比例，就不该跑出
   * [0, 1] —— 超出去的话 SVG 会照着 `strokeDashoffset` 画出一段**反向**
   * 的弧，看起来像环在往回长。
   *
   * 正常路径下永远不会越界（剩余时间本来就落在 [0, 总时长] 里），
   * 所以这是道保险，不是常规分支。留着它的理由和 `Math.max(1, ...)`
   * 一样：万一哪天时间来源出了问题，环应该「画得不准」，而不是
   * 「画成另一个东西」。
   */
  const elapsedRatio =
    remaining === null
      ? 0
      : Math.min(1, Math.max(0, 1 - remaining / Math.max(1, totalSeconds)));

  return (
    <div className="break-stage">
      {/* 呼吸引导环：背景层，始终在，但很淡 */}
      {stage === "resting" ? <div className="break-breathe" aria-hidden /> : null}

      <div className="break-content">
        {stage === "ask" ? (
          <AskStage
            snapshot={snapshot}
            onStart={() => void handleStartBreak()}
            onSnooze={(minutes) => void handleSnooze(minutes)}
            onSkip={() => void handleSkip()}
            snoozeOptions={snoozeOptions}
          />
        ) : null}

        {stage === "intent" ? (
          <IntentStage
            value={intentText}
            onChange={setIntentText}
            onSubmit={() => void handleSubmitIntent()}
          />
        ) : null}

        {stage === "resting" ? (
          <RestingStage
            remaining={remaining ?? totalSeconds}
            totalSeconds={totalSeconds}
            progress={elapsedRatio}
          />
        ) : null}

        {stage === "done" ? (
          <DoneStage
            intent={restoredIntent}
            onFinish={() => void handleFinish()}
          />
        ) : null}
      </div>

      {/* 休息中的「结束休息」出口 —— 用户永远有出口（交互原则 3） */}
      {stage === "resting" ? (
        <button
          className="btn btn-ghost break-exit"
          onClick={() => {
            void (async () => {
              const intent = await api.endBreak();
              setRestoredIntent(intent);
              setStage("done");
            })();
          }}
        >
          提前结束
        </button>
      ) : null}

      {/* 角落的 Esc 提示 —— 用最轻的方式告诉用户「有键盘出口」。
          它不抢注意力（很小、很淡、在角落），但需要的时候一定找得到。 */}
      <span className="break-esc-hint" aria-hidden>
        Esc 关闭
      </span>
    </div>
  );
}

// ============================================================ 01 询问

interface AskStageProps {
  snapshot: NonNullable<ReturnType<typeof useTacet>["snapshot"]>;
  onStart: () => void;
  onSnooze: (minutes: number) => void;
  onSkip: () => void;
  snoozeOptions: number[];
}

function AskStage({
  snapshot,
  onStart,
  onSnooze,
  onSkip,
  snoozeOptions,
}: AskStageProps) {
  const decision = snapshot.lastDecision;

  // 「为什么现在提醒我」—— 交互原则 5：提醒卡片上永远能找到理由。
  const why = decision?.reasons ?? [];
  // 过滤掉「需求 XX%」这类内部指标，用户看到的应该是具体事实
  const facts = why.filter(
    (reason) =>
      reason.reason !== "need_below_threshold" &&
      reason.reason !== "context_unavailable",
  );
  return (
    <div className="stage-ask">
      <h1 className="ask-title">建议休息一下</h1>

      <div className="ask-facts">
        {facts.length > 0 ? (
          facts.map((reason, index) => (
            <div className="ask-fact" key={`${reason.reason}-${index}`}>
              <span className="ask-fact-dot" aria-hidden />
              <span>{reasonText(reason)}</span>
            </div>
          ))
        ) : (
          <div className="ask-fact">
            <span className="ask-fact-dot" aria-hidden />
            <span>连续工作了一段时间，该歇一会儿了</span>
          </div>
        )}
      </div>

      <div className="ask-actions">
        <button className="btn btn-primary ask-primary" onClick={onStart}>
          现在休息
        </button>

        <div className="ask-secondary">
          {snoozeOptions.map((minutes) => (
            <button
              key={minutes}
              className="btn btn-quiet"
              onClick={() => onSnooze(minutes)}
            >
              {minutes} 分钟后
            </button>
          ))}
        </div>

        {/* 跳过必须始终可见，且不加任何解释性文案（那会变成变相的指责） */}
        <button className="btn btn-ghost ask-skip" onClick={onSkip}>
          这次不用
        </button>
      </div>
    </div>
  );
}

// ============================================================ 03 记录 Intent

interface IntentStageProps {
  value: string;
  onChange: (value: string) => void;
  onSubmit: () => void;
}

function IntentStage({ value, onChange, onSubmit }: IntentStageProps) {
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    // 自动聚焦，省掉用户一次点击
    inputRef.current?.focus();
  }, []);

  const charCount = [...value].length;
  const overLimit = charCount > 100;

  return (
    <div className="stage-intent">
      <div className="kicker intent-kicker">休息之前</div>
      <h2 className="intent-title">接下来准备做什么？</h2>
      <p className="sub intent-hint">
        休息完会原样还给你。不填也可以。
      </p>

      <div className="intent-field">
        <input
          ref={inputRef}
          className="intent-input"
          value={value}
          onChange={(event) => onChange(event.target.value)}
          placeholder="比如：完成 Auth 模块的测试"
          maxLength={120}
          onKeyDown={(event) => {
            if (event.key === "Enter" && !overLimit) {
              event.preventDefault();
              onSubmit();
            }
          }}
        />
        <span className="intent-caret" aria-hidden />
      </div>

      <div className="intent-meta">
        <span className={`sub ${overLimit ? "intent-over" : ""}`}>
          {charCount} / 100
        </span>
        <div className="intent-buttons">
          <button className="btn btn-quiet" onClick={onSubmit}>
            跳过
          </button>
          <button
            className="btn btn-primary"
            onClick={onSubmit}
            disabled={overLimit}
          >
            开始休息
          </button>
        </div>
      </div>
    </div>
  );
}

// ============================================================ 02 休息中

interface RestingStageProps {
  remaining: number;
  totalSeconds: number;
  progress: number;
}

/** 休息清单里的一项。 */
interface RestingTip {
  Icon: () => JSX.Element;
  /** 图标底座的配色类（与主面板的 tile-* 同一套）。 */
  tone: string;
  label: string;
  sub: string;
}

/**
 * 休息时可以做的三件具体事。
 *
 * ## 为什么是「具体动作」而不是「要放松哦」
 *
 * 休息最大的敌人不是不想休息，而是**不知道该干什么**——于是刷一下手机，
 * 五分钟过去，眼睛更累。给出可执行的小动作，休息才真的发生。
 *
 * ## 为什么是这三件
 *
 * 对应这一屏要修复的三件事：眼睛（睫状肌）、水分、久坐的肌肉。
 * 三件都是「离开屏幕就能做」的，不需要任何准备。
 */
const RESTING_TIPS: RestingTip[] = [
  {
    Icon: IconEye,
    tone: "tile-eye",
    label: "看向 6 米外，保持 20 秒",
    sub: "让睫状肌松一下",
  },
  {
    Icon: IconDrop,
    tone: "tile-hydration",
    label: "喝几口水",
    sub: "顺手补一次水",
  },
  {
    Icon: IconWalk,
    tone: "tile-movement",
    label: "站起来活动 2~3 分钟",
    sub: "走动或拉伸都可以",
  },
];

/**
 * 休息时那句话 —— 延迟出现，之后每分钟换一句。
 *
 * 用「空字符串」表示还没到出现的时机。调用方必须**始终**渲染这个容器
 * （哪怕内容是空的），否则句子出现的一刻整个画面会往上一跳。
 */
function useRestingQuote(): string {
  const [quote, setQuote] = useState("");

  useEffect(() => {
    // 先让行动清单被读完，句子是之后的事。
    const appear = window.setTimeout(() => setQuote(pickQuote()), QUOTE_DELAY_MS);
    const rotate = window.setInterval(
      () => setQuote(pickQuote()),
      QUOTE_ROTATE_MS,
    );

    return () => {
      window.clearTimeout(appear);
      window.clearInterval(rotate);
    };
  }, []);

  return quote;
}

function RestingStage({ remaining, totalSeconds, progress }: RestingStageProps) {
  // 呼吸节奏提示：4 秒吸 / 6 秒呼 = 6 次/分（慢呼吸的生理依据）
  const cycle = 10;
  const phase = remaining % cycle;
  const breathingIn = phase < 4;

  const offset = RING_CIRCUMFERENCE * (1 - progress);
  const quote = useRestingQuote();

  return (
    <div className="stage-resting">
      <div className="kicker resting-kicker">休息中</div>

      <div className="resting-ring-wrap">
        <svg
          className="resting-ring"
          width={(RING_RADIUS + 14) * 2}
          height={(RING_RADIUS + 14) * 2}
          viewBox={`0 0 ${(RING_RADIUS + 14) * 2} ${(RING_RADIUS + 14) * 2}`}
          aria-hidden
        >
          <circle
            className="resting-ring-track"
            cx={RING_RADIUS + 14}
            cy={RING_RADIUS + 14}
            r={RING_RADIUS}
          />
          <circle
            className="resting-ring-progress"
            cx={RING_RADIUS + 14}
            cy={RING_RADIUS + 14}
            r={RING_RADIUS}
            strokeDasharray={RING_CIRCUMFERENCE}
            strokeDashoffset={offset}
          />
        </svg>

        <div className="resting-center">
          <div className="resting-count numeric">{formatClock(remaining)}</div>
          <div className="resting-total sub">
            共 {Math.round(totalSeconds / 60)} 分钟
          </div>
        </div>
      </div>

      <div className="resting-breathe-hint">
        {breathingIn ? "吸气 4 秒" : "呼气 6 秒"}
      </div>

      {/* 行动清单：这一屏唯一「有信息量」的东西，也是唯一要用户动手的 */}
      <div className="resting-card">
        {RESTING_TIPS.map(({ Icon, tone, label, sub }, index) => (
          <div key={label}>
            {index > 0 ? <div className="resting-hair" aria-hidden /> : null}
            <div className="resting-row">
              <span className={`tile ${tone}`}>
                <Icon />
              </span>
              <div className="resting-row-text">
                <div className="resting-row-label">{label}</div>
                <div className="resting-row-sub">{sub}</div>
              </div>
            </div>
          </div>
        ))}
      </div>

      {/* 休息时的一句话：可看可不看，字号与对比度都压到最低。
          容器始终占位，避免句子出现时画面往上跳。 */}
      <div className="resting-quote-slot">
        {quote ? (
          <p className="resting-quote" key={quote}>
            {quote}
          </p>
        ) : null}
      </div>
    </div>
  );
}

// ------------------------------------------------------------ 清单图标

/**
 * 清单里的小图标。
 *
 * 用 SVG 而不是 emoji：emoji 的彩色和形状由系统决定，在这个
 * 「低信息量」的界面里既吵又不可控；线性图标则能跟着 tile 的
 * 配色走，和其他界面保持一致。
 */
function IconEye() {
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

function IconDrop() {
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

function IconWalk() {
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

// ============================================================ 04 休息结束

interface DoneStageProps {
  intent: IntentRecord | null;
  onFinish: () => void;
}

function DoneStage({ intent, onFinish }: DoneStageProps) {
  return (
    <div className="stage-done">
      <div className="kicker done-kicker">欢迎回来</div>

      {intent && intent.text.length > 0 ? (
        <>
          <p className="done-lead">休息前你准备继续：</p>
          <div className="done-intent selectable">{intent.text}</div>
        </>
      ) : (
        <p className="done-lead done-lead-solo">
          休息好了。刚才没有记录待办，可以慢慢想一下从哪儿接上。
        </p>
      )}

      <button className="btn btn-primary done-button" onClick={onFinish}>
        继续
      </button>
    </div>
  );
}
