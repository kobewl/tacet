//! 存储层错误。

use thiserror::Error;

/// 存储层可能出现的错误。
#[derive(Debug, Error)]
pub enum StorageError {
    /// 数据库操作失败。
    #[error("数据库错误：{0}")]
    Sqlite(#[from] rusqlite::Error),

    /// JSON 编码或解码失败（`payload` / `reasons` 这类字段）。
    #[error("JSON 编解码失败：{0}")]
    Json(#[from] serde_json::Error),

    /// 数据本身有问题（例如数据库里存了无法识别的枚举值）。
    #[error("数据格式不正确：{0}")]
    Data(String),

    /// 核心层的校验失败（如 Intent 过长）。
    #[error(transparent)]
    Core(#[from] tacet_core::CoreError),

    /// 数据库 schema 版本比当前程序支持的还新。
    ///
    /// 这是**降级保护**（数据模型 §4.1）：用户可能装了新版又退回旧版，
    /// 这时旧版程序绝不能去写一个它不认识的库 —— 那会把新数据搞坏。
    /// 正确做法是提示用户升级程序，然后什么都不做。
    #[error("数据库版本 {found} 高于本程序支持的 {supported}，请升级 Tacet 后再使用")]
    SchemaTooNew { found: i64, supported: i64 },

    /// 找不到数据库文件所在的目录（用户目录不可写等）。
    #[error("无法定位数据目录：{0}")]
    DataDirUnavailable(String),
}

/// 存储层的统一返回类型。
pub type Result<T> = std::result::Result<T, StorageError>;

impl StorageError {
    /// 这个错误是否意味着「应该停止使用数据库并提示用户」。
    pub fn requires_user_action(&self) -> bool {
        matches!(self, StorageError::SchemaTooNew { .. })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 版本过高需要用户处理() {
        let err = StorageError::SchemaTooNew {
            found: 5,
            supported: 1,
        };

        assert!(err.requires_user_action());
        assert!(err.to_string().contains("升级 Tacet"));
    }

    #[test]
    fn 普通错误不需要打断用户() {
        let err = StorageError::Data("payload 不是合法 JSON".to_string());
        assert!(!err.requires_user_action());
    }
}
