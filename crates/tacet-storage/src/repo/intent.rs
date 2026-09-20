//! Intent 仓储 —— `intents` 表的读写。
//!
//! ## 这一张表的特殊性
//!
//! `intents.text` 是**用户亲手输入的内容**，属于 P1 级隐私（数据模型 §7）：
//! 本地存储，**默认不进入任何 AI 摘要**。这也是它为什么单独建表而不是
//! 塞进通用事件表的 `payload` 里 —— 将来做数据分级导出或清理时，
//! 「把用户手写的东西全部排除」是一条 SQL 就能表达的事。

use rusqlite::{params, OptionalExtension};
use tacet_core::model::Intent;
use tacet_core::Timestamp;

use crate::db::Database;
use crate::error::Result;

/// `intents` 表的仓储。
pub struct IntentRepo;

impl IntentRepo {
    /// 保存一条 Intent，返回新记录的 id。
    ///
    /// **空文本不会被保存**（返回 `Ok(None)`）。
    /// PRD 说这个输入框「可跳过不填」，那就意味着「跳过」不应该在数据库里
    /// 留下一条空记录 —— 那会让「历史 Intent」列表里出现一堆空白项。
    pub fn save(db: &Database, intent: &Intent) -> Result<Option<i64>> {
        if intent.is_blank() {
            return Ok(None);
        }

        let conn = db.lock();
        conn.execute(
            "INSERT INTO intents (text, created_at, restored_at) VALUES (?1, ?2, ?3)",
            params![
                intent.text,
                intent.created_at.as_millis(),
                intent.restored_at.map(|at| at.as_millis())
            ],
        )?;

        Ok(Some(conn.last_insert_rowid()))
    }

    /// 按 id 读取。
    pub fn find(db: &Database, id: i64) -> Result<Option<Intent>> {
        let conn = db.lock();
        let row = conn
            .query_row(
                "SELECT id, text, created_at, restored_at FROM intents WHERE id = ?1",
                params![id],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, Option<i64>>(3)?,
                    ))
                },
            )
            .optional()?;

        Ok(row.map(|(id, text, created_at, restored_at)| {
            Intent::from_stored(
                id,
                text,
                Timestamp::from_millis(created_at),
                restored_at.map(Timestamp::from_millis),
            )
        }))
    }

    /// 标记「已在休息结束界面展示过」。
    pub fn mark_restored(db: &Database, id: i64, at: Timestamp) -> Result<bool> {
        let conn = db.lock();
        let affected = conn.execute(
            "UPDATE intents SET restored_at = ?1 WHERE id = ?2",
            params![at.as_millis(), id],
        )?;

        Ok(affected > 0)
    }

    /// 最近一条 Intent（不管有没有被恢复过）。
    ///
    /// 休息结束时用它取「休息前记录的那句」。
    pub fn latest(db: &Database) -> Result<Option<Intent>> {
        let conn = db.lock();
        let row = conn
            .query_row(
                "SELECT id, text, created_at, restored_at FROM intents \
                 ORDER BY created_at DESC, id DESC LIMIT 1",
                [],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, Option<i64>>(3)?,
                    ))
                },
            )
            .optional()?;

        Ok(row.map(|(id, text, created_at, restored_at)| {
            Intent::from_stored(
                id,
                text,
                Timestamp::from_millis(created_at),
                restored_at.map(Timestamp::from_millis),
            )
        }))
    }

    /// 最近一条**尚未恢复**的 Intent。
    ///
    /// 这是休息结束界面的正确取数方式：如果用户上次休息的 Intent 还没展示过，
    /// 就应当先把它还给用户；已经展示过的就不该再弹一遍。
    pub fn latest_unrestored(db: &Database) -> Result<Option<Intent>> {
        let conn = db.lock();
        let row = conn
            .query_row(
                "SELECT id, text, created_at, restored_at FROM intents \
                 WHERE restored_at IS NULL ORDER BY created_at DESC, id DESC LIMIT 1",
                [],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, Option<i64>>(3)?,
                    ))
                },
            )
            .optional()?;

        Ok(row.map(|(id, text, created_at, restored_at)| {
            Intent::from_stored(
                id,
                text,
                Timestamp::from_millis(created_at),
                restored_at.map(Timestamp::from_millis),
            )
        }))
    }

    /// 最近的若干条历史（新的在前）。
    pub fn recent(db: &Database, limit: u32) -> Result<Vec<Intent>> {
        let conn = db.lock();
        let mut statement = conn.prepare(
            "SELECT id, text, created_at, restored_at FROM intents \
             ORDER BY created_at DESC, id DESC LIMIT ?1",
        )?;

        let rows = statement
            .query_map(params![limit], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, Option<i64>>(3)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(rows
            .into_iter()
            .map(|(id, text, created_at, restored_at)| {
                Intent::from_stored(
                    id,
                    text,
                    Timestamp::from_millis(created_at),
                    restored_at.map(Timestamp::from_millis),
                )
            })
            .collect())
    }

    /// 总条数（诊断用）。
    pub fn count(db: &Database) -> Result<u32> {
        let conn = db.lock();
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM intents", [], |row| row.get(0))?;
        Ok(count.max(0) as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tacet_core::model::MAX_INTENT_CHARS;

    fn t0() -> Timestamp {
        Timestamp::from_millis(1_700_000_000_000)
    }

    fn intent(text: &str) -> Intent {
        Intent::new(text, t0()).expect("创建 Intent")
    }

    #[test]
    fn 保存并读回() {
        let db = Database::open_in_memory().expect("打开");
        let id = IntentRepo::save(&db, &intent("完成 Auth 模块测试"))
            .expect("保存")
            .expect("应当返回 id");

        let loaded = IntentRepo::find(&db, id).expect("查询").expect("应当存在");
        assert_eq!(loaded.text, "完成 Auth 模块测试");
        assert_eq!(loaded.created_at, t0());
        assert!(!loaded.is_restored());
    }

    #[test]
    fn 空白内容不落库() {
        // 用户选择跳过不填 —— 这是被允许的答案，不该在历史里留一条空白。
        let db = Database::open_in_memory().expect("打开");

        let id = IntentRepo::save(&db, &intent("   ")).expect("保存不应报错");
        assert_eq!(id, None);
        assert_eq!(IntentRepo::count(&db).expect("统计"), 0);
    }

    #[test]
    fn 标记已恢复() {
        let db = Database::open_in_memory().expect("打开");
        let id = IntentRepo::save(&db, &intent("写周报"))
            .expect("保存")
            .expect("有 id");

        let restored_at = t0().saturating_add_millis(300_000);
        assert!(IntentRepo::mark_restored(&db, id, restored_at).expect("更新"));

        let loaded = IntentRepo::find(&db, id).expect("查询").expect("存在");
        assert!(loaded.is_restored());
        assert_eq!(loaded.restored_at, Some(restored_at));
    }

    #[test]
    fn 标记不存在的记录返回假() {
        let db = Database::open_in_memory().expect("打开");
        assert!(!IntentRepo::mark_restored(&db, 999, t0()).expect("查询本身应当成功"));
    }

    #[test]
    fn 取最近一条() {
        let db = Database::open_in_memory().expect("打开");

        IntentRepo::save(&db, &intent("第一件事")).expect("保存");
        let later = Intent::new("第二件事", t0().saturating_add_millis(3_600_000)).expect("创建");
        IntentRepo::save(&db, &later).expect("保存");

        let latest = IntentRepo::latest(&db).expect("查询").expect("应当有");
        assert_eq!(latest.text, "第二件事");
    }

    #[test]
    fn 取最近一条尚未恢复的() {
        // 这是休息结束界面的正确取数逻辑：
        // 已经还给用户看过的 Intent，不该再弹一遍。
        let db = Database::open_in_memory().expect("打开");

        let first = IntentRepo::save(&db, &intent("已经看过的"))
            .expect("保存")
            .expect("有 id");
        let second =
            Intent::new("还没看过的", t0().saturating_add_millis(3_600_000)).expect("创建");
        IntentRepo::save(&db, &second).expect("保存");

        // 只有第一条被恢复过
        IntentRepo::mark_restored(&db, first, t0()).expect("更新");

        let pending = IntentRepo::latest_unrestored(&db)
            .expect("查询")
            .expect("应当有");
        assert_eq!(pending.text, "还没看过的");
    }

    #[test]
    fn 全部恢复后没有待恢复的() {
        let db = Database::open_in_memory().expect("打开");
        let id = IntentRepo::save(&db, &intent("唯一一条"))
            .expect("保存")
            .expect("有 id");
        IntentRepo::mark_restored(&db, id, t0()).expect("更新");

        assert_eq!(IntentRepo::latest_unrestored(&db).expect("查询"), None);
    }

    #[test]
    fn 空库查询返回空而不是报错() {
        let db = Database::open_in_memory().expect("打开");

        assert_eq!(IntentRepo::latest(&db).expect("查询"), None);
        assert_eq!(IntentRepo::latest_unrestored(&db).expect("查询"), None);
        assert_eq!(IntentRepo::find(&db, 1).expect("查询"), None);
        assert!(IntentRepo::recent(&db, 10).expect("查询").is_empty());
    }

    #[test]
    fn 历史列表按时间倒序() {
        let db = Database::open_in_memory().expect("打开");

        for (index, text) in ["早", "中", "晚"].iter().enumerate() {
            let at = t0().saturating_add_millis(index as i64 * 3_600_000);
            let item = Intent::new(*text, at).expect("创建");
            IntentRepo::save(&db, &item).expect("保存");
        }

        let history = IntentRepo::recent(&db, 10).expect("查询");
        assert_eq!(history.len(), 3);
        assert_eq!(history[0].text, "晚");
        assert_eq!(history[2].text, "早");
    }

    #[test]
    fn 长文本按字符数校验() {
        let db = Database::open_in_memory().expect("打开");

        // 恰好 100 个中文字应当被接受（300 字节）
        let ok = Intent::new("测".repeat(MAX_INTENT_CHARS), t0()).expect("应当接受");
        assert!(IntentRepo::save(&db, &ok).expect("保存").is_some());

        // 101 个应当被拒绝，且拒绝发生在构造阶段
        let err = Intent::new("测".repeat(MAX_INTENT_CHARS + 1), t0()).expect_err("应当拒绝");
        assert!(err.to_string().contains("100"));
    }

    #[test]
    fn 中文与表情符号都能正确存取() {
        let db = Database::open_in_memory().expect("打开");
        let text = "回复小李的邮件 📧，然后继续改 bug";

        let id = IntentRepo::save(&db, &intent(text))
            .expect("保存")
            .expect("有 id");

        let loaded = IntentRepo::find(&db, id).expect("查询").expect("存在");
        assert_eq!(loaded.text, text);
    }
}
