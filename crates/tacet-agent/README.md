# tacet-agent（V0.4 预留）

**这个 crate 目前是空的，这是刻意的。**

## 为什么 v0.1 里没有它

产品原则 7 写的是：

> **AI 完全可选** —— 不配置任何 AI 服务时，产品依然完整可用；
> 无 Key、无网络都不影响核心功能，AI 只是增强。

而路线图把 AI Agent 放在 **V0.4**：

| 版本 | 内容 |
| --- | --- |
| V0.1 | 提醒闭环 + 全屏 Overlay + Intent + SQLite（**不含 AI**） |
| V0.2 | Context Awareness：会议检测、Interruptibility、Fusion |
| V0.3 | Learning：习惯学习、分时段策略、接受率 |
| **V0.4** | **AI Agent：日报周报、Agent Chat、决策解释、动态 Planner** |

所以 v0.1 的依赖图里根本不该出现这个 crate。目录先建好，
是为了让「核心链路不依赖 AI」这件事在结构上一眼可见。

## 红线 6 会盯住它

研发规范 §3.3 红线 6：**核心链路不得依赖 tacet-agent**。

CI 里的「AI 可选性」检查会做这件事：

1. 搜出所有引用 `tacet-agent` 的 `Cargo.toml`
2. 把 `crates/tacet-agent` 整个挪走、把那些依赖行删掉
3. `cargo build --workspace --all-targets`
4. **编译通过 = 核心不依赖 AI**

现在这个检查是「天然成立」的（没有任何依赖引用它）。
但等 V0.4 真的开始写 agent 时，这条检查才会开始真正起作用 ——
它会在每一次提交上验证「摘掉 AI 之后产品依然完整」。

## 将来写在这里的东西

- `Planner` —— 把长期目标拆成可执行的提醒计划
- `Explanation` —— 用自然语言解释「为什么刚才提醒我」
- `Review` —— 日报 / 周报的生成
- `Chat` —— 与用户的对话界面

**这四个模块全部是可选的增强**：它们挂掉、没配 Key、或者没有网络时，
`tacet-core` + `tacet-health` 驱动的提醒闭环必须照常工作。
