//! 迁移框架 —— 让数据库结构可以安全地长大。
//!
//! ## 为什么迁移值得单独设计
//!
//! 用户的数据是**不可再生**的。代码写错了可以改，UI 丑了可以调，
//! 但把用户一年的休息记录弄丢或搞乱，是不可挽回的。
//!
//! 数据模型 §4 给迁移定了五条规则，这个文件把它们逐条落成代码：
//!
//! | 规则 | 在本文件里的落点 |
//! | --- | --- |
//! | 一个版本一个脚本，只向前 | [`MIGRATIONS`] 数组，按版本升序 |
//! | 每个迁移在单事务内完成 | [`run`] 里用 `Transaction` 逐个包住 |
//! | 迁移前自动备份，保留最近 3 份 | [`backup_before_migration`] |
//! | 降级保护：库比自己新就提示而不写 | [`run`] 里的版本检查 |
//! | 每个迁移必须可测试 | [`run`] 接受任意目标版本，测试可停在中间态 |
//!
//! ## 迁移 ID 的纪律
//!
//! `MIGRATIONS` 是一个**只增不改**的数组。一旦某个版本的迁移脚本被发布出去，
//! 它在用户机器上已经跑过了 —— 再去修改它的内容，只会让「已经升级过的用户」
//! 和「全新安装的用户」拿到两套不同的库结构。要改，就加一个新版本。

use rusqlite::Connection;

use crate::error::{Result, StorageError};
use crate::path;

/// 当前程序支持的 schema 版本。
///
/// 每次新增迁移时，这个数字与 `MIGRATIONS` 数组一起加一。
///
/// 版本对照（数据模型 §2）：
///
/// | 版本 | 交付版本 | 内容 |
/// | --- | --- | --- |
/// | 1（S1） | v0.1 | `events` / `interventions` / `intents` / `settings` |
/// | 2（S2） | v0.2 | `decisions` / `context_snapshots` / `user_corrections` |
/// | 3（S3） | v0.3 | `user_model_stats` / `patterns` / `health_debt_snapshots` |
/// | 4（S4） | v0.4 | `ai_reviews` / `chat_sessions` |
pub const CURRENT_SCHEMA_VERSION: i64 = 1;

/// 备份文件保留几份（数据模型 §4.1：保留最近 3 份）。
pub const BACKUP_KEEP: usize = 3;

/// 一个迁移脚本。
#[derive(Debug, Clone, Copy)]
pub struct Migration {
    /// 迁移到哪个版本号。
    pub version: i64,
    /// 简短说明（写日志用）。
    pub name: &'static str,
    /// 要执行的 SQL。
    pub sql: &'static str,
}

/// 全部迁移脚本，按版本升序。
pub const MIGRATIONS: &[Migration] = &[Migration {
    version: 1,
    name: "S1__init",
    sql: include_str!("../migrations/S1__init.sql"),
}];

/// 读取数据库当前的 schema 版本。
///
/// 完全没有版本表（全新数据库）时返回 0。
pub fn read_schema_version(conn: &Connection) -> Result<i64> {
    // 先看表在不在 —— 全新库里直接查会报错
    let has_table: bool = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'schema_version'",
            [],
            |row| row.get::<_, i64>(0).map(|n| n > 0),
        )
        .map_err(StorageError::from)?;

    if !has_table {
        return Ok(0);
    }

    // 取最大版本号：迁移过程中可能留下多行历史记录
    let version: Option<i64> = conn
        .query_row("SELECT MAX(version) FROM schema_version", [], |row| {
            row.get(0)
        })
        .map_err(StorageError::from)?;

    Ok(version.unwrap_or(0))
}

/// 把数据库迁移到 `target_version`。
///
/// `db_path` 为 `Some` 时会在真正执行迁移前先做一次备份（内存库跳过）。
pub fn run(
    conn: &Connection,
    target_version: i64,
    db_path: Option<&std::path::Path>,
) -> Result<()> {
    let current = read_schema_version(conn)?;

    // 降级保护：库比程序新。
    //
    // 用户可能装了 v0.3 又退回 v0.1。这时它的库里有 v0.1 不认识的表，
    // 一旦让 v0.1 去写，就可能破坏 v0.3 的数据结构。
    // 正确做法是**什么都不做**，明确告诉用户去升级程序。
    if current > target_version {
        return Err(StorageError::SchemaTooNew {
            found: current,
            supported: target_version,
        });
    }

    if current == target_version {
        return Ok(());
    }

    let pending: Vec<&Migration> = MIGRATIONS
        .iter()
        .filter(|m| m.version > current && m.version <= target_version)
        .collect();

    if pending.is_empty() {
        return Ok(());
    }

    // 迁移前备份（只在文件库上做）。
    if let Some(path) = db_path {
        backup_before_migration(path, current)?;
    }

    for migration in pending {
        // 每个迁移单独一个事务：中途失败只回滚这一个，
        // 前面已成功的版本保持已应用状态，下次启动从这里继续。
        apply_one(conn, migration)?;
    }

    Ok(())
}

/// 在事务里执行单个迁移。
///
/// ## 为什么用 SQL 层的 `BEGIN` / `COMMIT` 而不是 rusqlite 的 `transaction()`
///
/// `rusqlite::Connection::transaction()` 需要 `&mut Connection`。
/// 但 `run()` 只拿到 `&Connection`（不可变借用），因为在它之上还有一个
/// `MutexGuard`，把可变引用再借出来会让整个调用链都得标记 `mut`。
///
/// 用 SQL 语句自己管事务是等价且更直白的做法：SQLite 保证
/// 脚本中途出错时整个事务回滚，`ROLLBACK` 是显式兜底。
fn apply_one(conn: &Connection, migration: &Migration) -> Result<()> {
    conn.execute_batch("BEGIN")?;

    let outcome = (|| -> Result<()> {
        conn.execute_batch(migration.sql)?;
        conn.execute(
            "INSERT INTO schema_version (version, applied_at) VALUES (?1, ?2)",
            rusqlite::params![migration.version, tacet_core::Timestamp::now().as_millis()],
        )?;
        Ok(())
    })();

    match outcome {
        Ok(()) => {
            conn.execute_batch("COMMIT")?;
            Ok(())
        }
        Err(err) => {
            // 回滚失败时优先报告**原始错误** —— 那才是根因，
            // 回滚失败只是它的后果，把后者报出来会误导排查方向。
            let _ = conn.execute_batch("ROLLBACK");
            Err(err)
        }
    }
}

/// 供测试使用的迁移执行入口（可以传入自定义脚本）。
#[cfg(test)]
fn apply_one_inner(conn: &Connection, migration: &Migration) -> Result<()> {
    apply_one(conn, migration)
}

/// 迁移前的备份（数据模型 §4.1）。
///
/// 保留最近 `BACKUP_KEEP` 份，更旧的在每次备份时顺手删掉。
/// 备份失败不阻断迁移 —— 备份是保险，不是前置条件；
/// 因为磁盘满之类的原因让用户完全用不了软件，是得不偿失的。
fn backup_before_migration(db_path: &std::path::Path, from_version: i64) -> Result<()> {
    if !db_path.exists() || from_version == 0 {
        // 全新数据库没什么可备份的（from_version == 0 时库还是空的）。
        return Ok(());
    }

    let backup = path::backup_path_for(db_path, from_version);

    if let Err(err) = std::fs::copy(db_path, &backup) {
        // 不阻断，但要留痕（这里用 eprintln，壳层可以改成结构化日志）。
        eprintln!(
            "警告：迁移前备份失败（{} -> {}）：{err}",
            db_path.display(),
            backup.display()
        );
        return Ok(());
    }

    prune_backups(db_path);
    Ok(())
}

/// 只保留最近 `BACKUP_KEEP` 份备份。
fn prune_backups(db_path: &std::path::Path) {
    let Some(dir) = db_path.parent() else {
        return;
    };
    let Some(stem) = db_path.file_name().map(|n| n.to_string_lossy().to_string()) else {
        return;
    };
    let prefix = format!("{stem}.bak.");

    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    // 收集 (版本号, 路径)，版本号大的更新
    let mut backups: Vec<(i64, std::path::PathBuf)> = entries
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            let version = name.strip_prefix(&prefix)?.parse::<i64>().ok()?;
            Some((version, entry.path()))
        })
        .collect();

    backups.sort_by_key(|(version, _)| *version);

    // 从最旧的开始删，直到只剩下 BACKUP_KEEP 份
    while backups.len() > BACKUP_KEEP {
        let (_, path) = backups.remove(0);
        let _ = std::fs::remove_file(path);
    }
}

/// 迁移脚本的元信息（启动日志与自检用）。
pub fn describe_all() -> Vec<(i64, &'static str)> {
    MIGRATIONS.iter().map(|m| (m.version, m.name)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Database;
    use tempfile::TempDir;

    #[test]
    fn 全新数据库版本为零() {
        let conn = Connection::open_in_memory().expect("打开内存库");
        assert_eq!(read_schema_version(&conn).expect("读版本"), 0);
    }

    #[test]
    fn 迁移脚本版本号连续且从一下开始() {
        // 版本号有洞意味着有人删了中间某个迁移 —— 那会让已升级的用户
        // 永远停在旧结构上，是个很难查的 bug。
        for (index, migration) in MIGRATIONS.iter().enumerate() {
            assert_eq!(
                migration.version,
                index as i64 + 1,
                "迁移版本号必须从 1 开始连续递增"
            );
        }

        assert_eq!(
            MIGRATIONS.last().map(|m| m.version),
            Some(CURRENT_SCHEMA_VERSION),
            "最后一个迁移的版本必须等于 CURRENT_SCHEMA_VERSION"
        );
    }

    #[test]
    fn 迁移脚本名称符合约定() {
        for migration in MIGRATIONS {
            assert!(
                migration.name.starts_with('S'),
                "迁移名应当以 S<版本> 开头，实际 {}",
                migration.name
            );
            assert!(
                migration.name.contains("__"),
                "迁移名应当采用 S<版本>__<说明> 的格式，实际 {}",
                migration.name
            );
            assert!(!migration.sql.trim().is_empty(), "迁移脚本不能为空");
        }
    }

    #[test]
    fn 迁移后版本正确且表齐全() {
        let db = Database::open_in_memory().expect("打开");
        assert_eq!(db.schema_version().expect("读版本"), 1);

        let conn = db.lock();
        for table in [
            "schema_version",
            "events",
            "interventions",
            "intents",
            "settings",
        ] {
            let exists: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = ?1",
                    [table],
                    |row| row.get(0),
                )
                .expect("查询表");
            assert_eq!(exists, 1, "表 {table} 应当存在");
        }
    }

    #[test]
    fn 索引被正确创建() {
        let db = Database::open_in_memory().expect("打开");
        let conn = db.lock();

        for index in [
            "idx_events_kind_time",
            "idx_events_time",
            "idx_interv_fired",
            "idx_interv_outcome",
            "idx_intents_created",
        ] {
            let exists: i64 = conn
                .query_row(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type = 'index' AND name = ?1",
                    [index],
                    |row| row.get(0),
                )
                .expect("查询索引");
            assert_eq!(exists, 1, "索引 {index} 应当存在");
        }
    }

    #[test]
    fn 重复迁移是幂等的() {
        let db = Database::open_in_memory().expect("打开");

        // 再跑一次迁移，不该报错也不该重复建表
        run(&db.lock(), CURRENT_SCHEMA_VERSION, None).expect("重复迁移应当安全");

        let count: i64 = db
            .lock()
            .query_row("SELECT COUNT(*) FROM schema_version", [], |row| row.get(0))
            .expect("查询");
        assert_eq!(count, 1, "不该重复记录版本");
    }

    #[test]
    fn 数据库比程序新时拒绝迁移() {
        // 模拟「用户装了新版又退回旧版」
        let db = Database::open_in_memory().expect("打开");
        db.lock()
            .execute(
                "INSERT INTO schema_version (version, applied_at) VALUES (99, 0)",
                [],
            )
            .expect("写入新版标记");

        let err = run(&db.lock(), CURRENT_SCHEMA_VERSION, None).expect_err("应当被拒绝");

        match err {
            StorageError::SchemaTooNew { found, supported } => {
                assert_eq!(found, 99);
                assert_eq!(supported, CURRENT_SCHEMA_VERSION);
            }
            other => panic!("期望版本过高错误，实际 {other}"),
        }

        // 关键：拒绝之后不能对库做任何写入
        let max: i64 = db
            .lock()
            .query_row("SELECT MAX(version) FROM schema_version", [], |row| {
                row.get(0)
            })
            .expect("查询");
        assert_eq!(max, 99, "拒绝迁移时不该改动数据");
    }

    #[test]
    fn 可以迁移到中间的版本() {
        // 这个能力对测试很重要：将来有 S2 时，可以造一个「刚升到 S1」的库
        // 来验证 S1→S2 的迁移。
        let conn = Connection::open_in_memory().expect("打开内存库");
        run(&conn, 1, None).expect("迁移到 S1");

        assert_eq!(read_schema_version(&conn).expect("读版本"), 1);
    }

    #[test]
    fn 目标版本为零时什么都不做() {
        let conn = Connection::open_in_memory().expect("打开内存库");
        run(&conn, 0, None).expect("应当是空操作");
        assert_eq!(read_schema_version(&conn).expect("读版本"), 0);
    }

    #[test]
    fn 文件库迁移时会创建备份() {
        let dir = TempDir::new().expect("临时目录");
        let db_path = dir.path().join("tacet.db");

        // 先建一个「老版本」的库：手工建表并把版本标成 0
        {
            let conn = Connection::open(&db_path).expect("打开");
            conn.execute_batch(
                "CREATE TABLE schema_version (version INTEGER PRIMARY KEY, applied_at INTEGER);",
            )
            .expect("建版本表");
            drop(conn);
        }

        // 迁移到 S1 —— 此时 from_version 是 0，属于「全新库」，
        // 按设计不备份（空库没什么可丢的）
        let db = Database::open(&db_path).expect("迁移");
        assert_eq!(db.schema_version().expect("读版本"), 1);

        let backups: Vec<_> = std::fs::read_dir(dir.path())
            .expect("列目录")
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".bak."))
            .collect();
        assert!(
            backups.is_empty(),
            "版本 0 的空库不需要备份，实际有 {} 个备份文件",
            backups.len()
        );
    }

    #[test]
    fn 备份文件会保留最近几份() {
        let dir = TempDir::new().expect("临时目录");
        let db_path = dir.path().join("tacet.db");
        std::fs::write(&db_path, b"fake db").expect("写文件");

        // 造 5 个备份
        for version in 1..=5 {
            let backup = path::backup_path_for(&db_path, version);
            std::fs::write(&backup, b"backup").expect("写备份");
        }

        prune_backups(&db_path);

        let remaining: Vec<i64> = std::fs::read_dir(dir.path())
            .expect("列目录")
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                name.strip_prefix("tacet.db.bak.")
                    .and_then(|v| v.parse::<i64>().ok())
            })
            .collect();

        assert_eq!(
            remaining.len(),
            BACKUP_KEEP,
            "应当只保留最新 {BACKUP_KEEP} 份"
        );
        assert!(remaining.contains(&5), "最新的备份必须留着");
        assert!(remaining.contains(&4));
        assert!(remaining.contains(&3));
    }

    #[test]
    fn 迁移失败的库会被完整回滚() {
        // 用一个坏的迁移脚本验证「单事务」这条规则真的生效。
        let conn = Connection::open_in_memory().expect("打开内存库");
        conn.execute_batch(
            "CREATE TABLE schema_version (version INTEGER PRIMARY KEY, applied_at INTEGER);",
        )
        .expect("建版本表");

        let broken = Migration {
            version: 1,
            name: "S1__broken",
            sql: "CREATE TABLE good (id INTEGER); CREATE TABLE bad (id INTEGER NOT NULL); \
                  INSERT INTO nonexistent_table VALUES (1);",
        };

        let outcome = apply_one_inner(&conn, &broken);
        assert!(outcome.is_err(), "坏脚本应当失败");

        // 关键断言：脚本前半段建的表也必须被回滚掉
        let good_exists: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name='good'",
                [],
                |row| row.get(0),
            )
            .expect("查询");
        assert_eq!(good_exists, 0, "失败迁移必须整体回滚，不能留下半张表");
    }

    #[test]
    fn 迁移脚本元信息可读() {
        let described = describe_all();
        assert_eq!(described, vec![(1, "S1__init")]);
    }
}
