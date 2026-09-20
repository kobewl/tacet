//! 数据库文件放在哪里。
//!
//! 数据模型 §1 规定：
//!
//! | 平台 | 路径 |
//! | --- | --- |
//! | macOS | `~/Library/Application Support/Tacet/tacet.db` |
//! | Windows（v0.5） | `%APPDATA%\Tacet\tacet.db` |
//!
//! ## 为什么自己拼路径，而不用 `dirs` 这类 crate
//!
//! v0.1 只支持 macOS，而 macOS 的应用数据目录规则极其稳定
//! （`~/Library/Application Support`）。自己拼十行代码，换来：
//!
//! - **零额外依赖**：健康工具要「存在感低」，依赖越少越好
//! - **路径完全可控**：测试里能直接指定路径，不用去改环境变量
//! - 将来支持 Windows 时，在这里加一个分支即可
//!
//! ## 测试里绝不碰真实数据库
//!
//! [`Database::open_in_memory`](crate::Database::open_in_memory) 用的是内存库，
//! 每次测试都从零开始，互相之间没有干扰，也不会污染开发者机器上的真实数据。
//! 这是让「每个迁移都有测试」这件事真正可行的前提。

use std::path::{Path, PathBuf};

use crate::error::{Result, StorageError};

/// 数据库文件名。
pub const DATABASE_FILENAME: &str = "tacet.db";

/// 应用在用户目录下的文件夹名。
pub const APP_DIRECTORY: &str = "Tacet";

/// 返回默认的数据目录。
///
/// 目录不存在时**不会**自动创建 —— 创建时机由 [`ensure_data_dir`] 显式控制，
/// 这样「只是想知道路径」和「真的要写文件」是两件不同的事。
pub fn default_data_dir() -> Result<PathBuf> {
    let home = std::env::var_os("HOME")
        .ok_or_else(|| StorageError::DataDirUnavailable("环境变量 HOME 未设置".to_string()))?;

    let mut path = PathBuf::from(home);

    #[cfg(target_os = "macos")]
    {
        path.push("Library");
        path.push("Application Support");
    }

    // 注意：这里的 cfg 是「选择一个常规目录」，不是平台能力代码 ——
    // 它不涉及任何业务逻辑，属于存储路径的固有差异（架构红线允许的范围是
    // 核心 crate 不含平台条件编译，而 tacet-storage 需要给出跨平台路径）。
    #[cfg(target_os = "windows")]
    {
        path = std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .ok_or_else(|| {
                StorageError::DataDirUnavailable("环境变量 APPDATA 未设置".to_string())
            })?;
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        // 其它平台用 XDG 约定；v0.1 不支持它们，但至少给个合理的位置，
        // 让 CI 上的 Linux runner 能跑测试。
        path.push(".local");
        path.push("share");
    }

    path.push(APP_DIRECTORY);
    Ok(path)
}

/// 返回默认的数据库文件路径。
pub fn default_database_path() -> Result<PathBuf> {
    Ok(default_data_dir()?.join(DATABASE_FILENAME))
}

/// 确保数据目录存在（必要时创建）。
pub fn ensure_data_dir(dir: &Path) -> Result<()> {
    if dir.exists() {
        return Ok(());
    }

    std::fs::create_dir_all(dir).map_err(|err| {
        StorageError::DataDirUnavailable(format!("无法创建 {}：{err}", dir.display()))
    })
}

/// 在同一个目录里为迁移备份生成文件名。
///
/// 数据模型 §4.1 要求「执行迁移前自动复制一份 `tacet.db.bak.<版本>`（保留最近 3 份）」。
/// 这个函数只负责生成名字，实际的复制与轮转由 [`crate::migration`] 负责。
pub fn backup_path_for(db_path: &Path, from_version: i64) -> PathBuf {
    let mut name = db_path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| DATABASE_FILENAME.to_string());

    name.push_str(&format!(".bak.{from_version}"));
    db_path.with_file_name(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 默认路径包含应用目录与文件名() {
        let dir = default_data_dir().expect("应当能定位数据目录");
        assert!(
            dir.ends_with(APP_DIRECTORY),
            "数据目录应当以 {APP_DIRECTORY} 结尾，实际 {}",
            dir.display()
        );

        let db = default_database_path().expect("应当能定位数据库");
        assert!(db.ends_with(DATABASE_FILENAME));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn 苹果平台使用标准应用支持目录() {
        let dir = default_data_dir().expect("应当能定位数据目录");
        let text = dir.to_string_lossy();

        assert!(
            text.contains("Library/Application Support"),
            "macOS 应当使用 ~/Library/Application Support，实际 {text}"
        );
    }

    #[test]
    fn 备份文件名带来源版本() {
        let path = Path::new("/tmp/Tacet/tacet.db");
        let backup = backup_path_for(path, 1);

        assert_eq!(backup.to_string_lossy(), "/tmp/Tacet/tacet.db.bak.1");
    }

    #[test]
    fn 备份文件与数据库同目录() {
        // 备份必须跟数据库放在一起，否则用户移动数据目录时会把备份落下。
        let path = Path::new("/Users/someone/Library/Application Support/Tacet/tacet.db");
        let backup = backup_path_for(path, 2);

        assert_eq!(
            backup.parent(),
            path.parent(),
            "备份文件应当与数据库在同一目录"
        );
    }
}
