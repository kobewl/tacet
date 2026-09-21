//! 干预记录仓储 —— `interventions` 表的读写与统计。
//!
//! ## 接受率是怎么算的
//!
//! 数据模型 §8 给了唯一口径：
//!
//! > 接受率 = `outcome = completed` 的 interventions / 全部**已发出**的 interventions
//!
//! 而「已发出」在这里被收窄为**真正打扰到用户的等级**（2 级及以上）——
//! Level 0（静默）和 Level 1（菜单栏计数）用户根本感知不到，
//! 把它们算进分母会让接受率变成一个没有意义的数字。
//!
//! 这个收窄的判断写在 [`tacet_core::model::InterventionLevel::disturbs_user`] 里，
//! 统计与决策共用同一份定义，不会出现两处口径不一致。

use rusqlite::{params, OptionalExtension};
use tacet_core::model::{Intervention, InterventionLevel, InterventionOutcome, NeedKind, Reason};
use tacet_core::Timestamp;

use crate::datewin::DateWindow;
use crate::db::{data_error, Database};
use crate::error::Result;

/// 某个时间窗口内的干预统计。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct InterventionStats {
    /// 真正打扰到用户的干预次数（分母）。
    pub disturbing: u32,
    /// 被接受（completed）的次数（分子）。
    pub completed: u32,
    /// 被跳过的次数。
    pub skipped: u32,
    /// 被延后的次数。
    pub snoozed: u32,
    /// 用户完全没响应的次数。
    pub ignored: u32,
    /// 还没有用户的回应。
    pub pending: u32,
}

impl InterventionStats {
    /// 接受率（0.0~1.0）；分母为 0 时返回 `None`。
    ///
    /// 返回 `Option` 而不是 0.0 是有意的：**「没有数据」和「接受率为零」
    /// 是两件完全不同的事**。界面应当显示「暂无数据」，而不是一个 0%
    /// ——后者看起来像在指责用户。
    pub fn acceptance_rate(&self) -> Option<f64> {
        if self.disturbing == 0 {
            return None;
        }
        Some(self.completed as f64 / self.disturbing as f64)
    }

    /// 跳过率。
    pub fn skip_rate(&self) -> Option<f64> {
        if self.disturbing == 0 {
            return None;
        }
        Some(self.skipped as f64 / self.disturbing as f64)
    }

    /// 已经响应的次数（不管接受还是跳过）。
    pub const fn resolved(&self) -> u32 {
        self.completed + self.skipped + self.snoozed + self.ignored
    }

    /// 把接受率格式化成界面文案。
    ///
    /// 没有数据时返回「暂无数据」而不是「0%」—— 这是文案规范的要求：
    /// 客观陈述，不制造负罪感。
    pub fn acceptance_text(&self) -> String {
        match self.acceptance_rate() {
            Some(rate) => format!("{:.0}%", rate * 100.0),
            None => "暂无数据".to_string(),
        }
    }
}

/// `interventions` 表的仓储。
pub struct InterventionRepo;

impl InterventionRepo {
    /// 记录一次提醒已发出，返回新记录的 id。
    pub fn insert(db: &Database, intervention: &Intervention) -> Result<i64> {
        let reasons = serde_json::to_string(&intervention.reasons)?;

        let conn = db.lock();
        conn.execute(
            "INSERT INTO interventions \
             (kind, level, reason, fired_at, resolved_at, outcome, snooze_minutes) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                intervention.kind.as_str(),
                intervention.level.as_i64(),
                reasons,
                intervention.fired_at.as_millis(),
                intervention.resolved_at.map(|at| at.as_millis()),
                intervention.outcome.map(|o| o.as_str()),
                intervention.snooze_minutes.map(|m| m as i64),
            ],
        )?;

        Ok(conn.last_insert_rowid())
    }

    /// 记录用户对某次提醒的回应。
    ///
    /// 返回是否真的更新到了一行。**没更新到是一个值得上报的情况**：
    /// 它意味着界面拿了一个不存在的 id，或者记录已经被删了。
    pub fn resolve(
        db: &Database,
        id: i64,
        outcome: InterventionOutcome,
        snooze_minutes: Option<u32>,
        at: Timestamp,
    ) -> Result<bool> {
        let conn = db.lock();
        let affected = conn.execute(
            "UPDATE interventions SET resolved_at = ?1, outcome = ?2, snooze_minutes = ?3 \
             WHERE id = ?4",
            params![
                at.as_millis(),
                outcome.as_str(),
                snooze_minutes.map(|m| m as i64),
                id
            ],
        )?;

        Ok(affected > 0)
    }

    /// 按 id 读取一条记录。
    pub fn find(db: &Database, id: i64) -> Result<Option<Intervention>> {
        let conn = db.lock();
        let row = conn
            .query_row(
                "SELECT id, kind, level, reason, fired_at, resolved_at, outcome, snooze_minutes \
                 FROM interventions WHERE id = ?1",
                params![id],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, i64>(4)?,
                        row.get::<_, Option<i64>>(5)?,
                        row.get::<_, Option<String>>(6)?,
                        row.get::<_, Option<i64>>(7)?,
                    ))
                },
            )
            .optional()?;

        row.map(decode_row).transpose()
    }

    /// 最近一次**真正打扰到用户**的干预。
    ///
    /// 决策引擎用一个类似的摘要来判断「同类提醒是不是刚发过」。
    ///
    /// ## 为什么只挑打扰等级的记录
    ///
    /// 不然一次 Level 1 的菜单栏计数会把冷却期也占掉。
    ///
    /// ## 为什么排除掉「已延后」的那条
    ///
    /// 这一条修的是一个真实 bug：用户点了「3 分钟后」，**3 分钟后不会再有提醒**。
    ///
    /// ```text
    ///   第 45 分钟  提醒弹出，用户点「3 分钟后」
    ///               → 这条记录 outcome = snoozed
    ///               → snooze_until = 第 48 分钟
    ///   第 48 分钟  延后到点了（snooze_until 过滤器把它放行）
    ///               → 但冷却期是「上次提醒时刻 + 45 分钟」= 第 90 分钟
    ///               → 于是又被冷却拦住，用户白等一场
    /// ```
    ///
    /// 根子在于「延后」是一条**重新约定**：用户按下的那一刻，
    /// 就等于把这次提醒改期到了 3 分钟后。既然后面有 `snooze_until`
    /// 这道闸门负责「在那之前保持安静」，冷却期就不该再拿原始时刻算一遍 ——
    /// 两道闸门叠在一起，延后窗口被盖住，承诺就落空了。
    ///
    /// 所以：被延后的那条记录，不再参与冷却计算。
    pub fn last_disturbing(db: &Database) -> Result<Option<Intervention>> {
        let conn = db.lock();
        let row = conn
            .query_row(
                "SELECT id, kind, level, reason, fired_at, resolved_at, outcome, snooze_minutes \
                 FROM interventions WHERE level >= 2 AND (outcome IS NULL OR outcome != 'snoozed') \
                 ORDER BY fired_at DESC, id DESC LIMIT 1",
                [],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, i64>(4)?,
                        row.get::<_, Option<i64>>(5)?,
                        row.get::<_, Option<String>>(6)?,
                        row.get::<_, Option<i64>>(7)?,
                    ))
                },
            )
            .optional()?;

        row.map(decode_row).transpose()
    }

    /// 还没有用户回应的记录（用于超时判定为「未响应」）。
    pub fn pending(db: &Database) -> Result<Vec<Intervention>> {
        let conn = db.lock();
        let mut statement = conn.prepare(
            "SELECT id, kind, level, reason, fired_at, resolved_at, outcome, snooze_minutes \
             FROM interventions WHERE outcome IS NULL ORDER BY fired_at ASC",
        )?;

        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, Option<i64>>(5)?,
                    row.get::<_, Option<String>>(6)?,
                    row.get::<_, Option<i64>>(7)?,
                ))
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        rows.into_iter().map(decode_row).collect()
    }

    /// 某个时间窗口内的统计。
    pub fn stats_in_window(db: &Database, window: &DateWindow) -> Result<InterventionStats> {
        let interventions = Self::in_window(db, window)?;

        let mut stats = InterventionStats::default();

        for record in interventions {
            // 只有真正打扰到用户的记录才进入分母
            if !record.counts_toward_acceptance_rate() {
                continue;
            }
            stats.disturbing += 1;

            match record.outcome {
                Some(InterventionOutcome::Completed) => stats.completed += 1,
                Some(InterventionOutcome::Skipped) => stats.skipped += 1,
                Some(InterventionOutcome::Snoozed) => stats.snoozed += 1,
                Some(InterventionOutcome::Ignored) => stats.ignored += 1,
                None => stats.pending += 1,
            }
        }

        Ok(stats)
    }

    /// 某个时间窗口内的全部记录。
    pub fn in_window(db: &Database, window: &DateWindow) -> Result<Vec<Intervention>> {
        let conn = db.lock();
        let mut statement = conn.prepare(
            "SELECT id, kind, level, reason, fired_at, resolved_at, outcome, snooze_minutes \
             FROM interventions WHERE fired_at >= ?1 AND fired_at < ?2 \
             ORDER BY fired_at ASC, id ASC",
        )?;

        let rows = statement
            .query_map(
                params![window.start.as_millis(), window.end.as_millis()],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                        row.get::<_, String>(3)?,
                        row.get::<_, i64>(4)?,
                        row.get::<_, Option<i64>>(5)?,
                        row.get::<_, Option<String>>(6)?,
                        row.get::<_, Option<i64>>(7)?,
                    ))
                },
            )?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        rows.into_iter().map(decode_row).collect()
    }
}

/// 把一行数据库记录解码成领域对象。
#[allow(clippy::type_complexity)]
fn decode_row(
    row: (
        i64,
        String,
        i64,
        String,
        i64,
        Option<i64>,
        Option<String>,
        Option<i64>,
    ),
) -> Result<Intervention> {
    let (id, kind, level, reason, fired_at, resolved_at, outcome, snooze_minutes) = row;

    let kind = NeedKind::parse(&kind).ok_or_else(|| data_error("未知的需求类型", &kind))?;
    let level = InterventionLevel::from_i64(level)?;

    let reasons: Vec<Reason> = serde_json::from_str(&reason)?;

    let outcome = match outcome {
        Some(text) => Some(
            InterventionOutcome::parse(&text).ok_or_else(|| data_error("未知的用户回应", &text))?,
        ),
        None => None,
    };

    Ok(Intervention::from_stored(
        id,
        kind,
        level,
        reasons,
        Timestamp::from_millis(fired_at),
        resolved_at.map(Timestamp::from_millis),
        outcome,
        snooze_minutes.map(|m| m.max(0) as u32),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::datewin::LocalOffset;

    fn t0() -> Timestamp {
        Timestamp::from_millis(1_700_000_000_000)
    }

    fn window_around(at: Timestamp) -> DateWindow {
        DateWindow::day_of(at, LocalOffset::utc())
    }

    fn record(level: InterventionLevel, at: Timestamp) -> Intervention {
        Intervention::fired(
            NeedKind::Rest,
            level,
            vec![Reason::ContinuousWork { minutes: 78 }],
            at,
        )
    }

    #[test]
    fn 写入后能按原样读回() {
        let db = Database::open_in_memory().expect("打开");
        let original = record(InterventionLevel::FullScreen, t0());

        let id = InterventionRepo::insert(&db, &original).expect("写入");
        let loaded = InterventionRepo::find(&db, id)
            .expect("查询")
            .expect("应当存在");

        assert_eq!(loaded.kind, NeedKind::Rest);
        assert_eq!(loaded.level, InterventionLevel::FullScreen);
        assert_eq!(loaded.reasons, original.reasons);
        assert_eq!(loaded.fired_at, t0());
        assert!(loaded.outcome.is_none());
    }

    #[test]
    fn 记录用户完成操作() {
        let db = Database::open_in_memory().expect("打开");
        let id = InterventionRepo::insert(&db, &record(InterventionLevel::FullScreen, t0()))
            .expect("写入");

        let resolved_at = t0().saturating_add_millis(30_000);
        let updated =
            InterventionRepo::resolve(&db, id, InterventionOutcome::Completed, None, resolved_at)
                .expect("更新");
        assert!(updated, "应当更新到一行");

        let loaded = InterventionRepo::find(&db, id)
            .expect("查询")
            .expect("存在");
        assert_eq!(loaded.outcome, Some(InterventionOutcome::Completed));
        assert_eq!(loaded.resolved_at, Some(resolved_at));
        assert_eq!(loaded.response_ms(), Some(30_000));
    }

    #[test]
    fn 记录延后时长() {
        let db = Database::open_in_memory().expect("打开");
        let id = InterventionRepo::insert(&db, &record(InterventionLevel::FullScreen, t0()))
            .expect("写入");

        InterventionRepo::resolve(&db, id, InterventionOutcome::Snoozed, Some(3), t0())
            .expect("更新");

        let loaded = InterventionRepo::find(&db, id)
            .expect("查询")
            .expect("存在");
        assert_eq!(loaded.outcome, Some(InterventionOutcome::Snoozed));
        assert_eq!(loaded.snooze_minutes, Some(3));
    }

    #[test]
    fn 更新不存在的记录会返回假() {
        let db = Database::open_in_memory().expect("打开");
        let updated = InterventionRepo::resolve(&db, 999, InterventionOutcome::Skipped, None, t0())
            .expect("查询本身应当成功");

        assert!(!updated, "不该声称更新成功");
    }

    #[test]
    fn 只有打扰级别的记录计入接受率分母() {
        let db = Database::open_in_memory().expect("打开");

        // Level 0 与 Level 1 不该进统计
        InterventionRepo::insert(&db, &record(InterventionLevel::Silent, t0())).expect("写入");
        InterventionRepo::insert(&db, &record(InterventionLevel::Ambient, t0())).expect("写入");
        // Level 2 起算
        InterventionRepo::insert(&db, &record(InterventionLevel::Notification, t0()))
            .expect("写入");
        InterventionRepo::insert(&db, &record(InterventionLevel::FullScreen, t0())).expect("写入");

        let stats = InterventionRepo::stats_in_window(&db, &window_around(t0())).expect("统计");
        assert_eq!(stats.disturbing, 2, "只应统计 Level 2 及以上");
    }

    #[test]
    fn 统计各类回应数量() {
        let db = Database::open_in_memory().expect("打开");

        let outcomes = [
            InterventionOutcome::Completed,
            InterventionOutcome::Completed,
            InterventionOutcome::Skipped,
            InterventionOutcome::Snoozed,
            InterventionOutcome::Ignored,
        ];

        for outcome in outcomes {
            let id = InterventionRepo::insert(&db, &record(InterventionLevel::FullScreen, t0()))
                .expect("写入");
            InterventionRepo::resolve(&db, id, outcome, None, t0()).expect("更新");
        }

        // 再来一条没回应的
        InterventionRepo::insert(&db, &record(InterventionLevel::Notification, t0()))
            .expect("写入");

        let stats = InterventionRepo::stats_in_window(&db, &window_around(t0())).expect("统计");
        assert_eq!(stats.disturbing, 6);
        assert_eq!(stats.completed, 2);
        assert_eq!(stats.skipped, 1);
        assert_eq!(stats.snoozed, 1);
        assert_eq!(stats.ignored, 1);
        assert_eq!(stats.pending, 1);
        assert_eq!(stats.resolved(), 5);
    }

    #[test]
    fn 接受率计算() {
        let stats = InterventionStats {
            disturbing: 4,
            completed: 1,
            skipped: 3,
            ..Default::default()
        };

        let rate = stats.acceptance_rate().expect("应当有值");
        assert!((rate - 0.25).abs() < 1e-9);
        assert_eq!(stats.acceptance_text(), "25%");
    }

    #[test]
    fn 没有数据时接受率是未知而不是零() {
        // 「暂无数据」和「0%」在用户眼里完全是两件事。
        // 后者看起来像在指责用户，前者只是陈述事实。
        let stats = InterventionStats::default();

        assert_eq!(stats.acceptance_rate(), None);
        assert_eq!(stats.acceptance_text(), "暂无数据");
        assert_eq!(stats.skip_rate(), None);
    }

    #[test]
    fn 统计只覆盖指定时间窗口() {
        let db = Database::open_in_memory().expect("打开");
        let offset = LocalOffset::utc();
        let base = crate::datewin::days_from_civil(2026, 9, 20) * 86_400_000;
        let today = DateWindow::day_of(Timestamp::from_millis(base + 3_600_000), offset);

        // 昨天有一条，今天有两条
        InterventionRepo::insert(
            &db,
            &record(
                InterventionLevel::FullScreen,
                Timestamp::from_millis(base - 3_600_000),
            ),
        )
        .expect("写入");
        InterventionRepo::insert(
            &db,
            &record(
                InterventionLevel::FullScreen,
                Timestamp::from_millis(base + 3_600_000),
            ),
        )
        .expect("写入");
        InterventionRepo::insert(
            &db,
            &record(
                InterventionLevel::FullScreen,
                Timestamp::from_millis(base + 5 * 3_600_000),
            ),
        )
        .expect("写入");

        let stats = InterventionRepo::stats_in_window(&db, &today).expect("统计");
        assert_eq!(stats.disturbing, 2, "只应统计今天的记录");
    }

    #[test]
    fn 查询最近一次真正打扰的提醒() {
        let db = Database::open_in_memory().expect("打开");

        assert_eq!(InterventionRepo::last_disturbing(&db).expect("查询"), None);

        // 一条 Level 1（用户感知不到）+ 一条 Level 2
        InterventionRepo::insert(&db, &record(InterventionLevel::Ambient, t0())).expect("写入");
        let later = t0().saturating_add_millis(60_000);
        InterventionRepo::insert(&db, &record(InterventionLevel::Notification, later))
            .expect("写入");

        let last = InterventionRepo::last_disturbing(&db)
            .expect("查询")
            .expect("应当有记录");

        assert_eq!(last.level, InterventionLevel::Notification);
        assert_eq!(last.fired_at, later, "应当取最近的那条打扰记录");
    }

    /// 回归测试：被用户「延后」的那条提醒，不该再占着冷却期。
    ///
    /// ## 这个 bug 长什么样
    ///
    /// 用户点了「3 分钟后」，然后 3 分钟后**什么都没发生**。
    ///
    /// 原因就是这条查询会把那条 `snoozed` 记录当成「最近一次打扰」返回，
    /// 而冷却期是从**它的原始时刻**起算的 —— 于是延后窗口（3 分钟）
    /// 整个落在冷却期（一个完整间隔）里面，延后到点时又被冷却拦下。
    ///
    /// 用户看到的是「我按了 3 分钟后，它就再也不理我了」。
    /// 这条测试钉住的是：延后之后，冷却不该再叠一层。
    #[test]
    fn 已延后的提醒不再占用冷却期() {
        let db = Database::open_in_memory().expect("打开");

        // 第 45 分钟弹出提醒，用户点了「3 分钟后」
        let fired = t0();
        let id = InterventionRepo::insert(&db, &record(InterventionLevel::FullScreen, fired))
            .expect("写入");
        InterventionRepo::resolve(&db, id, InterventionOutcome::Snoozed, Some(3), fired)
            .expect("标记延后");

        // 第 48 分钟：延后到点了，此时应当**没有**「最近一次打扰」挡路，
        // 让决策引擎能重新开口（真正的静默期由 snooze_until 负责）
        assert_eq!(
            InterventionRepo::last_disturbing(&db).expect("查询"),
            None,
            "被延后的记录不该再作为「上次打扰」参与冷却计算 —— \
             否则用户按了「3 分钟后」就再也等不到提醒"
        );
    }

    /// 延后之后又有新的提醒发出，冷却要按**新**的那条算。
    ///
    /// 上一条测试保证了「延后不占冷却」，这条保证不会因此放过新记录。
    #[test]
    fn 延后之后的再次提醒会重新开始冷却() {
        let db = Database::open_in_memory().expect("打开");

        let fired = t0();
        let id = InterventionRepo::insert(&db, &record(InterventionLevel::FullScreen, fired))
            .expect("写入");
        InterventionRepo::resolve(&db, id, InterventionOutcome::Snoozed, Some(3), fired)
            .expect("标记延后");

        // 延后到点后重新提醒了一次
        let refired = fired.saturating_add_millis(3 * 60_000);
        InterventionRepo::insert(&db, &record(InterventionLevel::FullScreen, refired))
            .expect("写入");

        let last = InterventionRepo::last_disturbing(&db)
            .expect("查询")
            .expect("应当有记录");

        assert_eq!(last.fired_at, refired, "应当取延后之后那条新提醒的时刻");
    }

    #[test]
    fn 查询待响应的提醒() {
        let db = Database::open_in_memory().expect("打开");

        let id1 = InterventionRepo::insert(&db, &record(InterventionLevel::FullScreen, t0()))
            .expect("写入");
        InterventionRepo::insert(&db, &record(InterventionLevel::Notification, t0()))
            .expect("写入");
        InterventionRepo::resolve(&db, id1, InterventionOutcome::Skipped, None, t0())
            .expect("更新");

        let pending = InterventionRepo::pending(&db).expect("查询");
        assert_eq!(pending.len(), 1, "只应剩一条未响应的");
        assert_eq!(pending[0].level, InterventionLevel::Notification);
    }

    #[test]
    fn 数据库里的非法等级会报错() {
        let db = Database::open_in_memory().expect("打开");
        db.lock()
            .execute(
                "INSERT INTO interventions (kind, level, reason, fired_at) \
                 VALUES ('rest', 9, '[]', 0)",
                [],
            )
            .expect("写入脏数据");

        let err = InterventionRepo::find(&db, 1).expect_err("应当报错");
        assert!(
            err.to_string().contains('9'),
            "错误信息应当带上非法值：{err}"
        );
    }

    #[test]
    fn 理由清单能正确地序列化往返() {
        let db = Database::open_in_memory().expect("打开");
        let mut record = record(InterventionLevel::FullScreen, t0());
        record.reasons = vec![
            Reason::ContinuousWork { minutes: 78 },
            Reason::SinceLastHydration { minutes: 93 },
            Reason::DoNotDisturb,
        ];

        let id = InterventionRepo::insert(&db, &record).expect("写入");
        let loaded = InterventionRepo::find(&db, id)
            .expect("查询")
            .expect("存在");

        assert_eq!(loaded.reasons.len(), 3);
        assert_eq!(loaded.reasons[0], Reason::ContinuousWork { minutes: 78 });
        assert_eq!(loaded.reasons[2], Reason::DoNotDisturb);
        assert_eq!(loaded.why_lines().len(), 3);
    }
}
