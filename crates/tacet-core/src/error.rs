//! 核心层错误类型。
//!
//! 研发规范 §3.1 约定：**库 crate 用 `thiserror` 定义具名错误**，应用层才用 `anyhow` 兜底。
//! 好处是调用方能对错误做模式匹配（而不是只能打印字符串），
//! 也能逼着我们把「可能失败的情况」在类型里说清楚。

use thiserror::Error;

/// 核心层可能出现的错误。
///
/// 注意这里派生的是 `PartialEq` 而不是 `Eq`：`InvalidNeedScore` 里带着 `f64`，
/// 而浮点数不满足全序关系（`NaN != NaN`），所以整个枚举只能是偏序比较。
/// 这不是缺陷 —— 错误类型本来就只需要能比较、能打印。
#[derive(Debug, Error, PartialEq)]
pub enum CoreError {
    /// Intent 文本超过长度上限（PRD §3.4：单行文本 ≤ 100 字符）。
    #[error("Intent 文本过长：{actual} 个字符，上限 {max} 个字符")]
    IntentTooLong { actual: usize, max: usize },

    /// 干预等级不在 0~5 范围内（通常来自数据库里的脏数据）。
    #[error("非法的干预等级：{0}（合法范围 0~5）")]
    InvalidInterventionLevel(i64),

    /// 需求强度不是 0.0~1.0 之间的有限数。
    #[error("非法的需求强度：{0}（必须是 0.0~1.0 之间的有限数）")]
    InvalidNeedScore(f64),

    /// 时间戳倒挂：结束时间早于开始时间。
    #[error("时间顺序颠倒：结束 {end} 早于开始 {start}")]
    TimeOrderInverted { start: i64, end: i64 },
}
