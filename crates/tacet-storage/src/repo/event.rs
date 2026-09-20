//! 行为事件仓储 —— `events` 表的读写。

use rusqlite::{params, OptionalExtension};
use tacet_core::model::BehaviorKind;
use tacet_core::Timestamp;

use crate::datewin::DateWindow;
use crate::db::{data_error, Database};
use crate::error::Result;

/// 从数据库里读出来的一条事件。
#[derive(Debug, Clone, PartialEq)]
pub struct EventRow {
    /// 主键。
    pub id: i64,
    /// 事件类型。
    pub kind: BehaviorKind,
    /// 附加数据（JSON 字符串，原样保留）。
    pub payload: String,
    /// 事件发生时间。
    pub occurred_at: Timestamp,
    /// 入库时间。
    pub created_at: Timestamp,
}

/// `events` 表的仓储。
pub struct EventRepo;

impl EventRepo {
    /// 追加一条事件。
    ///
    /// 返回新记录的 id。写入时会把「入库时间」自动填成当前时刻 ——
    /// 调用方不需要（也不应该）关心这件事。
    pub fn append(
        db: &Database,
        kind: BehaviorKind,
        payload: &str,
        occurred_at: Timestamp,
    ) -> Result<i64> {
        let now = Timestamp::now().as_millis();
        let conn = db.lock();

        conn.execute(
            "INSERT INTO events (kind, payload, occurred_at, created_at) VALUES (?1, ?2, ?3, ?4)",
            params![kind.as_str(), payload, occurred_at.as_millis(), now],
        )?;

        Ok(conn.last_insert_rowid())
    }

    /// 按类型读取最近若干条（新的在前）。
    pub fn recent_of_kind(db: &Database, kind: BehaviorKind, limit: u32) -> Result<Vec<EventRow>> {
        let conn = db.lock();
        let mut statement = conn.prepare(
            "SELECT id, payload, occurred_at, created_at FROM events \
             WHERE kind = ?1 ORDER BY occurred_at DESC, id DESC LIMIT ?2",
        )?;

        let rows = statement
            .query_map(params![kind.as_str(), limit], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        rows.into_iter()
            .map(|(id, payload, occurred_at, created_at)| {
                let parsed = BehaviorKind::parse(kind.as_str())
                    .ok_or_else(|| data_error("事件类型", kind.as_str()))?;
                Ok(EventRow {
                    id,
                    kind: parsed,
                    payload,
                    occurred_at: Timestamp::from_millis(occurred_at),
                    created_at: Timestamp::from_millis(created_at),
                })
            })
            .collect()
    }

    /// 读取某个时间区间内的事件（左闭右开）。
    ///
    /// 传 [`DateWindow`] 进来就自动获得「本地自然日」的正确口径 ——
    /// 这正是数据模型 §8 想达到的效果：调用方不需要自己算边界。
    pub fn in_window(db: &Database, window: &DateWindow) -> Result<Vec<EventRow>> {
        let conn = db.lock();
        let mut statement = conn.prepare(
            "SELECT id, kind, payload, occurred_at, created_at FROM events \
             WHERE occurred_at >= ?1 AND occurred_at < ?2 \
             ORDER BY occurred_at ASC, id ASC",
        )?;

        let rows = statement
            .query_map(
                params![window.start.as_millis(), window.end.as_millis()],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, i64>(4)?,
                    ))
                },
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        rows.into_iter()
            .map(|(id, kind, payload, occurred_at, created_at)| {
                Ok(EventRow {
                    id,
                    kind: BehaviorKind::parse(&kind)
                        .ok_or_else(|| data_error("未知的事件类型", &kind))?,
                    payload,
                    occurred_at: Timestamp::from_millis(occurred_at),
                    created_at: Timestamp::from_millis(created_at),
                })
            })
            .collect()
    }

    /// 统计某个时间区间内某类事件发生了多少次。
    ///
    /// 这是统计页要用到的核心查询：「今天喝了 6 次水」就是
    /// `count_in_window(db, WaterLogged, today)`。
    pub fn count_in_window(db: &Database, kind: BehaviorKind, window: &DateWindow) -> Result<u32> {
        let conn = db.lock();
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM events \
             WHERE kind = ?1 AND occurred_at >= ?2 AND occurred_at < ?3",
            params![
                kind.as_str(),
                window.start.as_millis(),
                window.end.as_millis()
            ],
            |row| row.get(0),
        )?;

        Ok(count.max(0) as u32)
    }

    /// 某类事件最近一次发生的时间。
    ///
    /// 需求评分到处在用它：「距上次喝水多久」就是拿这个值算的。
    pub fn last_occurrence(db: &Database, kind: BehaviorKind) -> Result<Option<Timestamp>> {
        let conn = db.lock();
        let value: Option<i64> = conn
            .query_row(
                "SELECT MAX(occurred_at) FROM events WHERE kind = ?1",
                params![kind.as_str()],
                |row| row.get(0),
            )
            .optional()?
            .flatten();

        Ok(value.map(Timestamp::from_millis))
    }

    /// 事件总数（诊断用）。
    pub fn count_all(db: &Database) -> Result<u32> {
        let conn = db.lock();
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))?;
        Ok(count.max(0) as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::datewin::LocalOffset;

    fn t0() -> Timestamp {
        Timestamp::from_millis(1_700_000_000_000)
    }

    #[test]
    fn 写入后能读回来() {
        let db = Database::open_in_memory().expect("打开");

        let id = EventRepo::append(&db, BehaviorKind::WaterLogged, "{}", t0()).expect("写入");
        assert!(id > 0);

        let rows = EventRepo::recent_of_kind(&db, BehaviorKind::WaterLogged, 10).expect("读取");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].kind, BehaviorKind::WaterLogged);
        assert_eq!(rows[0].occurred_at, t0());
    }

    #[test]
    fn 只返回指定类型的事件() {
        let db = Database::open_in_memory().expect("打开");

        EventRepo::append(&db, BehaviorKind::WaterLogged, "{}", t0()).expect("写入");
        EventRepo::append(&db, BehaviorKind::ActivityLogged, "{}", t0()).expect("写入");

        let water = EventRepo::recent_of_kind(&db, BehaviorKind::WaterLogged, 10).expect("读取");
        assert_eq!(water.len(), 1);
        assert_eq!(water[0].kind, BehaviorKind::WaterLogged);
    }

    #[test]
    fn 最近记录按时间倒序且遵守上限() {
        let db = Database::open_in_memory().expect("打开");

        for minute in 0..5 {
            EventRepo::append(
                &db,
                BehaviorKind::WaterLogged,
                "{}",
                t0().saturating_add_millis(minute * 60_000),
            )
            .expect("写入");
        }

        let recent = EventRepo::recent_of_kind(&db, BehaviorKind::WaterLogged, 3).expect("读取");
        assert_eq!(recent.len(), 3);
        assert!(
            recent[0].occurred_at > recent[1].occurred_at,
            "应当新的在前"
        );
    }

    #[test]
    fn 附加数据原样保留() {
        let db = Database::open_in_memory().expect("打开");
        let payload = r#"{"source":"menu_bar","count":2}"#;

        EventRepo::append(&db, BehaviorKind::WaterLogged, payload, t0()).expect("写入");

        let rows = EventRepo::recent_of_kind(&db, BehaviorKind::WaterLogged, 1).expect("读取");
        assert_eq!(rows[0].payload, payload);
    }

    #[test]
    fn 按本地自然日统计() {
        let db = Database::open_in_memory().expect("打开");
        let offset = LocalOffset::from_hours(8); // 东八区

        // 北京时间 2026-09-20 09:00 = UTC 01:00
        let day_start_utc =
            crate::datewin::days_from_civil(2026, 9, 20) * 86_400_000 - 8 * 3_600_000;
        let morning = Timestamp::from_millis(day_start_utc + 9 * 3_600_000);
        let afternoon = Timestamp::from_millis(day_start_utc + 15 * 3_600_000);

        EventRepo::append(&db, BehaviorKind::WaterLogged, "{}", morning).expect("写入");
        EventRepo::append(&db, BehaviorKind::WaterLogged, "{}", afternoon).expect("写入");

        let today = DateWindow::day_of(morning, offset);
        assert_eq!(
            EventRepo::count_in_window(&db, BehaviorKind::WaterLogged, &today).expect("统计"),
            2
        );

        // 次日归零
        let tomorrow = DateWindow::from_day_index(today.day_index + 1, offset);
        assert_eq!(
            EventRepo::count_in_window(&db, BehaviorKind::WaterLogged, &tomorrow).expect("统计"),
            0
        );
    }

    #[test]
    fn 区间统计是左闭右开() {
        let db = Database::open_in_memory().expect("打开");
        let offset = LocalOffset::utc();
        let base = crate::datewin::days_from_civil(2026, 9, 20) * 86_400_000;
        let window = DateWindow::day_of(Timestamp::from_millis(base), offset);

        // 区间起点：算在内
        EventRepo::append(&db, BehaviorKind::WaterLogged, "{}", window.start).expect("写入");
        // 区间终点：不算在内
        EventRepo::append(&db, BehaviorKind::WaterLogged, "{}", window.end).expect("写入");

        assert_eq!(
            EventRepo::count_in_window(&db, BehaviorKind::WaterLogged, &window).expect("统计"),
            1,
            "应当只统计起点那次，终点那次属于次日"
        );
    }

    #[test]
    fn 查询最近一次发生时间() {
        let db = Database::open_in_memory().expect("打开");

        assert_eq!(
            EventRepo::last_occurrence(&db, BehaviorKind::WaterLogged).expect("查询"),
            None,
            "没有记录时应当是 None，而不是 0"
        );

        EventRepo::append(&db, BehaviorKind::WaterLogged, "{}", t0()).expect("写入");
        let later = t0().saturating_add_millis(3_600_000);
        EventRepo::append(&db, BehaviorKind::WaterLogged, "{}", later).expect("写入");

        assert_eq!(
            EventRepo::last_occurrence(&db, BehaviorKind::WaterLogged).expect("查询"),
            Some(later)
        );
    }

    #[test]
    fn 区间读取按时间升序() {
        let db = Database::open_in_memory().expect("打开");
        let offset = LocalOffset::utc();
        let base = crate::datewin::days_from_civil(2026, 9, 20) * 86_400_000;
        let window = DateWindow::day_of(Timestamp::from_millis(base), offset);

        for hour in [9, 7, 8] {
            EventRepo::append(
                &db,
                BehaviorKind::WaterLogged,
                "{}",
                Timestamp::from_millis(base + hour * 3_600_000),
            )
            .expect("写入");
        }

        let rows = EventRepo::in_window(&db, &window).expect("读取");
        assert_eq!(rows.len(), 3);
        assert!(rows[0].occurred_at < rows[1].occurred_at);
        assert!(rows[1].occurred_at < rows[2].occurred_at);
    }

    #[test]
    fn 数据库里的未知事件类型会报错而不是静默丢弃() {
        // 静默丢弃会让统计数字莫名其妙地少，属于最难查的 bug。
        let db = Database::open_in_memory().expect("打开");
        db.lock()
            .execute(
                "INSERT INTO events (kind, payload, occurred_at, created_at) VALUES ('unknown.kind', '{}', 0, 0)",
                [],
            )
            .expect("直接写入脏数据");

        let err = EventRepo::in_window(
            &db,
            &DateWindow::day_of(Timestamp::from_millis(0), LocalOffset::utc()),
        )
        .expect_err("应当报错");

        assert!(
            err.to_string().contains("unknown.kind"),
            "错误信息应当指出具体是哪个类型：{err}"
        );
    }

    #[test]
    fn 统计总数() {
        let db = Database::open_in_memory().expect("打开");
        assert_eq!(EventRepo::count_all(&db).expect("统计"), 0);

        EventRepo::append(&db, BehaviorKind::WaterLogged, "{}", t0()).expect("写入");
        EventRepo::append(&db, BehaviorKind::BreakCompleted, "{}", t0()).expect("写入");

        assert_eq!(EventRepo::count_all(&db).expect("统计"), 2);
    }
}
