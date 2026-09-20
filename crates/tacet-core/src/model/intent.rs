//! Intent —— 「我下一步要做什么」。
//!
//! 这是 Tacet 里最小、也最被低估的一个功能（功能清单 F4，用户故事 US-2）。
//!
//! 打断最贵的代价从来不是那五分钟，而是**回来以后想不起来刚才在干嘛**。
//! 一个念头被打断，重新捡起来的成本可能远超休息本身节省的精力。
//! 所以休息之前问一句「接下来准备做什么」，休息完原样奉还 ——
//! 把人从「上下文重建」里解放出来。
//!
//! ## 隐私
//!
//! Intent 文本属于 **P1 级隐私**（架构文档 §9.1）：
//! 本地存储，**默认不进入任何 AI 摘要**，除非用户在隐私面板里显式打开。
//! 这也是为什么它单独建表而不是塞进通用事件表 —— 将来要按数据级别做清理和导出。

use serde::{Deserialize, Serialize};

use crate::{CoreError, Timestamp};

/// Intent 文本的长度上限（PRD §3.4：单行文本 ≤ 100 字符）。
///
/// 数的是**字符数**而不是字节数：一个中文字在 UTF-8 里占 3 个字节，
/// 用 `len()` 判断的话，用户写到第 34 个字就会被拒绝 —— 这是个很常见的坑。
pub const MAX_INTENT_CHARS: usize = 100;

/// 一条 Intent 记录。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Intent {
    /// 数据库主键；尚未落库时为 `None`。
    pub id: Option<i64>,
    /// 用户输入的内容（已去除首尾空白）。
    pub text: String,
    /// 记录时间。
    pub created_at: Timestamp,
    /// 在休息结束界面上被展示出来的时间；`None` 表示还没恢复过。
    pub restored_at: Option<Timestamp>,
}

impl Intent {
    /// 创建一条 Intent。
    ///
    /// **空文本是合法的**：PRD 说这个输入框「可跳过不填」。
    /// 这种情况下调用方不应该落库，但也不该报错 ——
    /// 逼迫用户填点什么，是另一种形式的打扰。
    pub fn new(text: impl Into<String>, at: Timestamp) -> Result<Self, CoreError> {
        let text = text.into().trim().to_string();
        let char_count = text.chars().count();

        if char_count > MAX_INTENT_CHARS {
            return Err(CoreError::IntentTooLong {
                actual: char_count,
                max: MAX_INTENT_CHARS,
            });
        }

        Ok(Self {
            id: None,
            text,
            created_at: at,
            restored_at: None,
        })
    }

    /// 从数据库读回来的记录（带主键与恢复时间）。
    pub fn from_stored(
        id: i64,
        text: impl Into<String>,
        created_at: Timestamp,
        restored_at: Option<Timestamp>,
    ) -> Self {
        Self {
            id: Some(id),
            text: text.into(),
            created_at,
            restored_at,
        }
    }

    /// 标记「已在休息结束界面展示过」。
    ///
    /// 用 `Option` 而不是布尔：`None` 和「某时刻」的差别在统计上有用 ——
    /// 可以算出「有多少 Intent 用户压根没看」。
    pub fn mark_restored(&mut self, at: Timestamp) {
        self.restored_at = Some(at);
    }

    /// 是否已经被恢复展示过。
    pub const fn is_restored(&self) -> bool {
        self.restored_at.is_some()
    }

    /// 是否是空内容（用户选择了跳过）。
    pub fn is_blank(&self) -> bool {
        self.text.is_empty()
    }

    /// 字符数（不是字节数）。
    pub fn char_count(&self) -> usize {
        self.text.chars().count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at() -> Timestamp {
        Timestamp::from_millis(1_700_000_000_000)
    }

    #[test]
    fn 记录并去除首尾空白() {
        let intent = Intent::new("  完成 Auth 模块测试  ", at()).expect("创建应成功");
        assert_eq!(intent.text, "完成 Auth 模块测试");
        assert!(intent.id.is_none());
        assert!(!intent.is_restored());
    }

    #[test]
    fn 空文本是合法的() {
        // 用户跳过不填 —— 这是被允许的答案，不是错误。
        let intent = Intent::new("   ", at()).expect("空文本不应报错");
        assert!(intent.is_blank());
    }

    #[test]
    fn 中文字符数按字符计而不是字节() {
        // 100 个中文字 = 300 字节。如果用字节数判断，这条会被误判为超长。
        let text = "测".repeat(MAX_INTENT_CHARS);
        assert_eq!(
            text.len(),
            MAX_INTENT_CHARS * 3,
            "前置条件：确实是 3 字节字符"
        );

        let intent = Intent::new(text, at()).expect("恰好 100 字应当被接受");
        assert_eq!(intent.char_count(), MAX_INTENT_CHARS);
    }

    #[test]
    fn 超出上限时报错并说明实际长度() {
        let text = "字".repeat(MAX_INTENT_CHARS + 1);
        let err = Intent::new(text, at()).expect_err("101 字应当被拒绝");

        assert_eq!(
            err,
            CoreError::IntentTooLong {
                actual: MAX_INTENT_CHARS + 1,
                max: MAX_INTENT_CHARS,
            }
        );
    }

    #[test]
    fn 恢复时间只记录一次() {
        let mut intent = Intent::new("写周报", at()).expect("创建应成功");
        assert!(!intent.is_restored());

        let later = at().saturating_add_millis(300_000);
        intent.mark_restored(later);
        assert!(intent.is_restored());
        assert_eq!(intent.restored_at, Some(later));
    }

    #[test]
    fn 从数据库还原保留全部字段() {
        let stored = Intent::from_stored(7, "倒杯水再回来", at(), None);
        assert_eq!(stored.id, Some(7));
        assert_eq!(stored.text, "倒杯水再回来");
        assert!(!stored.is_restored());
    }
}
