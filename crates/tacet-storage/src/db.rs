//! 数据库连接与打开流程。
//!
//! ## 为什么用 `rusqlite` 而不是 `sqlx`
//!
//! 这是待决策项 D-01，在这个文件里落定：**用 `rusqlite`**，理由有三条。
//!
//! 1. **我们是同步的**。决策引擎、状态机、迁移都是同步逻辑，没有任何
//!    `async` 需求。`sqlx` 的异步能力在这里是纯负担 —— 它会拉着整个项目
//!    进 `tokio` 运行时，而健康工具的预算里没有这笔开销。
//! 2. **迁移要可控**。数据模型 §4 要求「一个版本一个脚本、单事务、失败回滚、
//!    迁移前备份」。这些用原生 SQL + 事务写出来只有几十行，且完全透明；
//!    交给 `sqlx::migrate!` 反而要看它的心情。
//! 3. **依赖更少**。`rusqlite` 的 `bundled` 特性把 SQLite 源码一起编进来，
//!    不依赖系统库版本 —— 用户机器上的 SQLite 是哪个版本，与产品行为无关。
//!
//! ## 连接参数的选择
//!
//! | 参数 | 值 | 为什么 |
//! | --- | --- | --- |
//! | `journal_mode` | WAL | 写入不阻塞读取，且崩溃后能恢复 |
//! | `synchronous` | NORMAL | WAL 模式下的安全默认值；`FULL` 会让每次写入都等磁盘 |
//! | `foreign_keys` | ON | SQLite 默认不开外键，必须显式打开 |
//! | `busy_timeout` | 5s | 后台线程与界面同时写时不至于立刻报错 |
//!
//! 我们要写的量很小（一天几十条记录），这些参数的意义主要是**正确性**而非性能。

use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};

use rusqlite::Connection;

use crate::error::{Result, StorageError};
use crate::migration::{self, CURRENT_SCHEMA_VERSION};
use crate::path;

/// 一个打开着的 Tacet 数据库。
///
/// 内部用 `Mutex` 包住连接：`rusqlite::Connection` 本身不是 `Sync` 的，
/// 而我们的使用场景是「后台调度线程写、界面线程读」。
/// 加锁的开销在每天几十次写入的量级上完全可以忽略。
///
/// ## 为什么不用连接池
///
/// 连接池是为高并发设计的。健康工具一天写几十条记录，
/// 引入连接池只会增加一层需要理解和调试的东西。
/// 等真的出现瓶颈再换 —— 而那时候的瓶颈大概也不是数据库。
pub struct Database {
    conn: Mutex<Connection>,
    /// 数据库文件路径；内存库为 `None`。
    path: Option<PathBuf>,
}

impl Database {
    /// 打开（或创建）默认位置的数据库。
    ///
    /// 这个方法会：
    /// 1. 确保数据目录存在
    /// 2. 打开数据库、设置连接参数
    /// 3. 执行迁移，把 schema 升到最新版本
    pub fn open_default() -> Result<Self> {
        let dir = path::default_data_dir()?;
        path::ensure_data_dir(&dir)?;
        Self::open(dir.join(path::DATABASE_FILENAME))
    }

    /// 打开指定路径的数据库。
    pub fn open(db_path: impl AsRef<Path>) -> Result<Self> {
        let db_path = db_path.as_ref().to_path_buf();

        if let Some(parent) = db_path.parent() {
            path::ensure_data_dir(parent)?;
        }

        let conn = Connection::open(&db_path)?;
        let mut database = Self {
            conn: Mutex::new(conn),
            path: Some(db_path),
        };

        database.configure()?;
        database.migrate()?;
        Ok(database)
    }

    /// 打开一个内存数据库（测试专用）。
    ///
    /// ## 为什么每个测试都用内存库
    ///
    /// 1. **快**：不碰磁盘，几百个测试瞬间跑完
    /// 2. **互相隔离**：每个测试从零开始，不会因为执行顺序不同而结果不同
    /// 3. **安全**：永远不会误删开发者机器上的真实数据
    ///
    /// 唯一的代价是「内存库与文件库的行为可能有细微差别」（比如 WAL 不适用），
    /// 所以另有一个专门的测试验证**真实文件库**也能正确打开与迁移。
    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let mut database = Self {
            conn: Mutex::new(conn),
            path: None,
        };

        database.configure()?;
        database.migrate()?;
        Ok(database)
    }

    /// 数据库文件路径（内存库为 `None`）。
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// 取连接锁。
    ///
    /// 锁中毒时照样继续用：某个线程写库时 panic 了，我们宁可继续服务，
    /// 也不要让整个应用再也存不下数据（那会让所有行为记录静默丢失）。
    pub(crate) fn lock(&self) -> MutexGuard<'_, Connection> {
        match self.conn.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        }
    }

    /// 当前 schema 版本。
    pub fn schema_version(&self) -> Result<i64> {
        migration::read_schema_version(&self.lock())
    }

    /// 设置连接参数。
    fn configure(&mut self) -> Result<()> {
        let conn = self.lock();

        // WAL 在内存库上不适用（会返回 "memory"），不是错误，忽略即可。
        let _ = conn.pragma_update(None, "journal_mode", "WAL");
        conn.pragma_update(None, "synchronous", "NORMAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.busy_timeout(std::time::Duration::from_secs(5))?;

        Ok(())
    }

    /// 执行迁移。
    fn migrate(&mut self) -> Result<()> {
        migration::run(&self.lock(), CURRENT_SCHEMA_VERSION, self.path.as_deref())
    }

    /// 在事务里执行一段操作，失败自动回滚。
    ///
    /// 仓储层的写操作都应该走这里 —— 尤其是「写事件 + 更新关联记录」
    /// 这类需要一起成功或一起失败的操作。
    pub fn transaction<T, F>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&rusqlite::Transaction<'_>) -> Result<T>,
    {
        let mut conn = self.lock();
        let tx = conn.transaction()?;
        let value = f(&tx)?;
        tx.commit()?;
        Ok(value)
    }
}

impl std::fmt::Debug for Database {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Database")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

/// 把「数据库里读出来的值不符合预期」包装成统一错误。
pub(crate) fn data_error(what: &str, detail: impl std::fmt::Display) -> StorageError {
    StorageError::Data(format!("{what}：{detail}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn 内存库可以直接打开() {
        let db = Database::open_in_memory().expect("应当打开成功");

        assert_eq!(
            db.schema_version().expect("应当能读到版本"),
            CURRENT_SCHEMA_VERSION
        );
        assert!(db.path().is_none());
    }

    #[test]
    fn 文件库能创建并在关闭后保留数据() {
        let dir = TempDir::new().expect("应当能创建临时目录");
        let db_path = dir.path().join("tacet.db");

        {
            let db = Database::open(&db_path).expect("应当打开成功");
            db.lock()
                .execute(
                    "INSERT INTO settings (key, value, updated_at) VALUES (?1, ?2, ?3)",
                    rusqlite::params!["general.do_not_disturb", "false", 0i64],
                )
                .expect("写入应当成功");
        }

        assert!(db_path.exists(), "数据库文件应当被创建");

        // 重新打开，数据还在
        let reopened = Database::open(&db_path).expect("应当重新打开成功");
        let count: i64 = reopened
            .lock()
            .query_row("SELECT COUNT(*) FROM settings", [], |row| row.get(0))
            .expect("查询应当成功");
        assert_eq!(count, 1, "重开之后数据不该丢");
    }

    #[test]
    fn 文件库会自动创建父目录() {
        let dir = TempDir::new().expect("应当能创建临时目录");
        let nested = dir.path().join("a").join("b").join("tacet.db");

        let db = Database::open(&nested).expect("应当能自动建目录并打开");
        assert!(nested.exists());
        assert_eq!(db.path(), Some(nested.as_path()));
    }

    #[test]
    fn 重复打开同一个库是安全的() {
        let dir = TempDir::new().expect("应当能创建临时目录");
        let db_path = dir.path().join("tacet.db");

        let first = Database::open(&db_path).expect("首次打开");
        drop(first);

        let second = Database::open(&db_path).expect("再次打开");
        assert_eq!(
            second.schema_version().expect("读版本"),
            CURRENT_SCHEMA_VERSION,
            "重复打开不该破坏 schema"
        );
    }

    #[test]
    fn 事务提交与回滚() {
        let db = Database::open_in_memory().expect("打开");

        // 成功提交
        db.transaction(|tx| {
            tx.execute(
                "INSERT INTO settings (key, value, updated_at) VALUES ('a', '1', 0)",
                [],
            )?;
            Ok(())
        })
        .expect("事务应当成功");

        // 失败回滚
        let failed: Result<()> = db.transaction(|tx| {
            tx.execute(
                "INSERT INTO settings (key, value, updated_at) VALUES ('b', '2', 0)",
                [],
            )?;
            Err(StorageError::Data("故意失败".to_string()))
        });
        assert!(failed.is_err());

        let count: i64 = db
            .lock()
            .query_row("SELECT COUNT(*) FROM settings", [], |row| row.get(0))
            .expect("查询");
        assert_eq!(count, 1, "失败的事务应当整体回滚，'b' 不该被写入");
    }

    #[test]
    fn 外键约束已开启() {
        let db = Database::open_in_memory().expect("打开");
        let enabled: i64 = db
            .lock()
            .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
            .expect("查询");
        assert_eq!(enabled, 1, "外键约束必须开启");
    }

    #[test]
    fn 连接可跨线程使用() {
        // 后台调度线程与界面线程都要访问数据库。
        use std::sync::Arc;

        let db = Arc::new(Database::open_in_memory().expect("打开"));
        let clone = Arc::clone(&db);

        let handle = std::thread::spawn(move || clone.schema_version());

        assert_eq!(handle.join().expect("线程不应 panic").expect("读版本"), 1);
    }
}
