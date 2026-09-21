# Tacet

> 乐谱记号 **tacet** —— 此处休止。
> *A wellness agent that knows when to speak up — and when to stay quiet.*

**Tacet** 是一个理解你工作状态的个人健康 Agent。它会学习你的工作习惯，在**合适的时间**、用**合适的方式**提醒你休息、喝水和活动，而不是机械地打断你。

> ✅ 当前状态：**V0.1 已完成可运行的 macOS 应用**（Foundation 基础闭环）
>
> `Tacet.app` 能构建、能启动，核心链路端到端跑通：状态机 → 需求评分 →
> 时机判断 → 决策 → 干预 → 落库。366 个测试全绿，clippy 零警告。

---

## 为什么叫 Tacet

在乐谱上，当一件乐器停止演奏，谱面会印一个记号：**tacet**。它不是「退出」，更不是「不重要」——它的意思是：

> **此刻，我知道该我安静了。**

这正是 Tacet 想学会的全部：99% 的时间它安静地待在菜单栏，只在真正值得的时候才开口。

- 发音：/ˈtæsɪt/（近「塔-西特」，重音在前）
- 中文小名：休止

---

## 它解决的问题

| 现有工具 | 毛病 |
| --- | --- |
| 番茄钟 | 只认识时钟，不认识你——你正在开会、正在心流，它照样弹窗 |
| 喝水 / 站立提醒 | 只认识间隔，不认识场合——提醒发多了，用户直接关掉它，从此再也不健康 |

Tacet 的解药：把「**打扰的成本**」提升为一等公民，与「**健康的需求**」放在同一张天平上称。

```
健康需求 × 可打扰度  →  用哪一级方式干预
（该管你吗）  （现在方便吗）      （Level 0 ~ 5）
```

---

## 五层干预体系

| Level | 名称 | 形式 | 适用场景 |
| --- | --- | --- | --- |
| 0 | Silent | 完全不打扰 | 刚休息过 / 用户勿扰 / 健康需求低 |
| 1 | Ambient | 菜单栏计数（💧 1h 26m） | 只需环境级提示 |
| 2 | Notification | 系统通知 | 开会中、写代码中，不宜明显打断 |
| 3 | Floating Card | 不阻塞操作的悬浮卡片 | 需要行动，但不宜强打断 |
| 4 | Full Screen | 全屏休息提醒：提醒时覆盖当前屏，用户确认休息后再覆盖所有屏幕 | 健康需求高 + 当前适合打断 |
| 5 | Escalated | 反复跳过 / 延后后的升级提醒 | 持续忽视（**永不强制锁屏**） |

---

## 核心能力

- **休息管理** —— 连续工作计时、完整休息 / 跳过 / 延后记录
- **休息引导** —— 全屏提醒里的呼吸节奏（4 秒吸 / 6 秒呼）、三件具体小事（远眺 / 喝水 / 活动）、一句适时的鼓励（本地精选句子池，不联网）
- **多显示器** —— 提醒时只覆盖当前屏；用户点了「现在休息」之后，其它屏幕也会被温柔地挡住，休息没法被「换块屏继续干活」绕过去
- **喝水** —— 次数、间隔、每日目标
- **站立与活动** —— 久坐检测、活动建议
- **护眼** —— 看屏时长提醒
- **Reminder Fusion** —— 多个提醒自动合并成一次干预，避免 Reminder Fatigue
- **Intent（下一步要做什么）** —— 休息前记录意图，休息结束后原样奉还，降低上下文丢失成本
- **Context Engine** —— 当前 App、Idle、全屏检测、会议概率（Meeting Probability）
- **User Model** —— 学习接受率、跳过 / 延后模式、不同时段与不同 App 的行为

---

## 设计原则

1. **不机械** —— 不是「时间到了 → 提醒」，而是「现在适合提醒吗？」
2. **不惩罚** —— Skip / Snooze / Ignore 是数据，不是失败；用 Health Debt 调整，而不是惩罚
3. **不过度采集** —— 只需知道「VS Code 活跃」，就不读取代码内容
4. **可解释** —— 每次提醒都能回答「为什么现在提醒我？」
5. **Local First** —— 数据默认保存在本地：无账号、无服务器、无云同步
6. **AI 不是核心规则引擎** —— 实时决策由规则 + 状态机 + 评分完成；LLM 只负责解释、总结、对话与规划
7. **AI 完全可选** —— **不配置任何 AI 服务时，产品依然完整可用**；无 Key、无网络都不影响核心功能，AI 只是增强

---

## 技术栈

| 层 | 技术 |
| --- | --- |
| Desktop | Tauri 2 |
| Core | Rust（Reminder / Context / Health / Policy / UserModel / Agent 引擎） |
| UI | React + TypeScript |
| Storage | SQLite（完全本地） |

平台目标：macOS（Apple Silicon 优先）→ Windows 10 / 11。架构从第一天起按跨平台设计。

---

## 路线图

- **V0.1 MVP** —— 提醒闭环 + 全屏 Overlay + Intent + SQLite。重点不是 AI，而是把基础闭环做正确
- **V0.2 Context Awareness** —— 会议检测、Interruptibility、Reminder Fusion、Health Debt、Floating Reminder、最佳提醒时机
- **V0.3 Learning** —— 用户习惯学习、分时段 / 分 App 策略、接受率与跳过模式
- **V0.4 AI Agent** —— AI 日报周报、Agent Chat、解释决策、动态 Planner
- **V1.0** —— macOS + Windows 双平台，完成 Sense → Context → Health → Plan → Intervention → Memory → Learning → AI 完整闭环

---

## 项目结构

```
tacet/
├── apps/desktop/          # Tauri 桌面应用（壳 + 平台接入）
│   └── icons/             # 应用图标 + 菜单栏图标（含生成脚本）
├── crates/                # Rust 核心（与操作系统解耦）
│   ├── tacet-core/        # 领域模型、工作状态机、事件总线、决策引擎
│   ├── tacet-context/     # Context Engine：用户当前在做什么
│   ├── tacet-health/      # Health Engine：四类需求评分 + 时机窗口
│   ├── tacet-storage/     # SQLite 持久化 + 迁移框架
│   ├── tacet-platform/    # 平台抽象层（7 大能力 trait + Mock 替身）
│   ├── platform-macos/    # 上述 trait 的 macOS 实现
│   └── tacet-agent/       # AI（**V0.4 才动工，当前刻意留空**）
├── packages/ui/           # React 界面（面板 / 休息流 / 设置 / 今日）
├── docs/
│   ├── brand/             # 品牌资产 + 设计依据（含调研结论）
│   └── design/            # 设计原型
└── .github/               # CI
```

### 依赖方向（单向，不可逆）

```
tacet-core  ←  tacet-health  ←  apps/desktop  →  packages/ui
    ↑             ↑                  ↑
    └── tacet-context ───────────────┘
    └── tacet-platform ← platform-macos
    └── tacet-storage
```

`tacet-core` 是整个依赖图的**根**，它不依赖任何其它 crate，
尤其不依赖操作系统 API —— 这保证了核心逻辑可以在 CI 的
Linux runner 上完整测试（详见 `crates/tacet-platform/src/mocks.rs`）。

## 开发环境

### 前置

```bash
# Rust（1.97+）
rustup toolchain install stable

# Node 22 + pnpm 10
corepack enable
pnpm install

# 启用提交前钩子（每个 clone 都要做一次，它是本地配置、不会随代码分发）
git config core.hooksPath .githooks
```

第三条容易被跳过，但它不是可选项：它是一道**在敏感信息进入 git 历史之前**
把它拦下来的闸门。CI 那些检查跑在推送之后，而密钥一旦进了历史就无法真正撤回
（详见下面「不要把密钥提交进来」）。

### 常用命令

```bash
# 单元测试（366 个，另有 6 个文档测试）
cargo test --workspace

# 格式与静态检查
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings

# 前端类型检查与 lint
pnpm --filter @tacet/ui typecheck
pnpm --filter @tacet/ui lint

# 在浏览器里预览界面（用假数据，不需要启动 Tauri）
pnpm --filter @tacet/ui dev

# 构建 macOS 应用
cd apps/desktop && pnpm exec tauri build --bundles app
```

### 数据与隐私

数据库位于 `~/Library/Application Support/Tacet/tacet.db`。

**全部数据留在本机**：无账号、无服务器、无云同步、无遥测。
应用**不申请**辅助功能权限、屏幕录制权限或麦克风权限
（见 `apps/desktop/entitlements.plist`）。

### 不要把密钥提交进来

这个仓库是公开的，而 v0.4 会接入 LLM，届时会有真实的 API Key。

**密钥一旦进了 commit 历史，就没有真正干净的回退方式。** 删掉文件只是让
`git log -p` 里少一行；对象仍在本地历史、GitHub 的存储、以及任何已经
clone / fork 过的人的硬盘上。唯一可靠的补救是**作废并轮换那个密钥**。
所以这件事只有预防有意义，三道防线都别绕过：

| 防线 | 位置 | 拦住什么 |
| --- | --- | --- |
| 提交前钩子 | `.githooks/pre-commit` | 密钥进入**本地历史**之前 |
| CI 检查 | `.github/workflows/ci.yml` 的「密钥不入库」 | 推送时再核一遍 |
| 忽略规则 | `.gitignore` | `.env` / `*.pem` / `*.p12` / `*.db` 等 |

凭据的正确存放位置：

- **本地开发**：项目根目录的 `.env`（已被忽略）或系统钥匙串
- **CI / 发布**：GitHub Secrets，用 `${{ secrets.XXX }}` 引用

**顺带一句**：`tacet.db` 也不能提交。它存着 Intent 文本 —— 那是用户
「下一步打算做什么」的原始记录，属于 P1 级隐私，比密钥更常被顺手拷贝进仓库。

## 文档

- **产品与项目文档中心**（路线图 / 功能清单 / PRD / 架构 / 数据模型 / 研发规范 / 发布流程 / 决策记录）：
  `~/Documents/Project Documentation/Tacet-文档`
- **工程文档**（随代码走，如模块设计、IPC 契约、技术验证结论）：本仓库 [`docs/`](docs/README.md)

## License

待定
