-- ============================================================================
-- Tacet schema S1（v0.1 交付）
--
-- 依据：数据模型规划 §3.1
--
-- 通用约定（数据模型 §2）：
--   · 表名小写复数蛇形
--   · 时间一律存 UTC 毫秒 INTEGER
--   · 主键用自增整数（v0.1 简单优先）
--
-- 隐私约定（数据模型 §7）：
--   · 这里没有任何一列用于存窗口标题、网页地址、剪贴板或音频
--   · events.payload 只放结构化摘要
-- ============================================================================

-- schema_version：迁移记录。每次迁移插入一行，保留完整历史。
CREATE TABLE IF NOT EXISTS schema_version (
    version    INTEGER PRIMARY KEY,
    applied_at INTEGER NOT NULL
);

-- events：一切可统计行为的原始记录（ADR-009：不为每种行为单独建表）
CREATE TABLE IF NOT EXISTS events (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    kind        TEXT    NOT NULL,
    payload     TEXT    NOT NULL DEFAULT '{}',   -- JSON
    occurred_at INTEGER NOT NULL,                -- 事件发生时间（UTC ms）
    created_at  INTEGER NOT NULL                 -- 入库时间（UTC ms）
);

CREATE INDEX IF NOT EXISTS idx_events_kind_time ON events (kind, occurred_at);
CREATE INDEX IF NOT EXISTS idx_events_time      ON events (occurred_at);

-- interventions：每次提醒的发出与响应（接受率的分子/分母来源）
CREATE TABLE IF NOT EXISTS interventions (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    kind           TEXT    NOT NULL,             -- rest / hydration / movement / eye_rest / fused
    level          INTEGER NOT NULL,             -- 干预等级 0~5
    reason         TEXT    NOT NULL DEFAULT '[]',-- 触发因子清单（JSON 数组）
    fired_at       INTEGER NOT NULL,
    resolved_at    INTEGER,                      -- 用户响应时间
    outcome        TEXT,                         -- completed / snoozed / skipped / ignored
    snooze_minutes INTEGER                       -- 延后时长
);

CREATE INDEX IF NOT EXISTS idx_interv_fired   ON interventions (fired_at);
CREATE INDEX IF NOT EXISTS idx_interv_outcome ON interventions (outcome);

-- intents：「下一步要做什么」（P1 级隐私，默认不进入任何 AI 摘要）
CREATE TABLE IF NOT EXISTS intents (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    text        TEXT    NOT NULL,
    created_at  INTEGER NOT NULL,
    restored_at INTEGER                          -- 在休息结束界面展示过的时间
);

CREATE INDEX IF NOT EXISTS idx_intents_created ON intents (created_at);

-- settings：键值对配置（ADR-009：新增设置项不需要迁移表结构）
CREATE TABLE IF NOT EXISTS settings (
    key        TEXT PRIMARY KEY,
    value      TEXT NOT NULL,                    -- 统一 JSON 编码，便于类型演进
    updated_at INTEGER NOT NULL
);
