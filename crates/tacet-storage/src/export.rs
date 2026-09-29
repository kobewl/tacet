//! 数据导出 —— 「数据属于用户」（存储原则 5）的另一半。
//!
//! ## 为什么这件事值得一个模块
//!
//! Local First 承诺了「数据在你机器上」，但如果数据**带不走**，
//! 那只是「数据被锁在你的机器上」。导出是把这句话兑现完整的最后一步：
//! 用户换了电脑、想用表格软件自己分析、或者只是想看一眼原始长什么样，
//! 都不应该需要先理解 SQLite。
//!
//! ## 为什么是 CSV，而不是 JSON
//!
//! 导出的服务对象是**用户**，不是程序：CSV 双击就能用表格软件打开，
//! 每行一条记录、每列一个字段，普通人看得懂。JSON 面向程序，
//! 让一个非开发者面对嵌套结构是推卸责任。
//! 程序间迁移将来真需要时，直接复制 `tacet.db` 文件即可 —— 那本来
//! 就是完整、无损的备份。
//!
//! ## 编码与转义
//!
//! - 文件头带 UTF-8 BOM：没有它，Excel 会按本地编码猜，中文变乱码。
//!   这是实测出来的坑，不是教科书式谨慎。
//! - CSV 转义按 RFC 4180：字段含逗号 / 引号 / 换行时整体加引号，
//!   字段内的引号翻倍。`payload` 与 `reasons` 是 JSON，几乎必然含逗号。

use std::fs;
use std::path::{Path, PathBuf};

use tacet_core::time::Timestamp;

use crate::datewin::{civil_from_days, DateWindow, LocalOffset};
use crate::db::Database;
use crate::error::{Result, StorageError};
use crate::repo::{EventRepo, InterventionRepo};

/// 一次导出产出的两个文件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportedFiles {
    /// 行为记录（`events` 表）。
    pub events_csv: PathBuf,
    /// 干预记录（`interventions` 表）。
    pub interventions_csv: PathBuf,
}

/// 把全部行为与干预记录导出成两个 CSV 文件，返回文件路径。
///
/// 时间列同时给出**本地时间字符串**（给人看）与 **UTC 毫秒**（给程序对）：
/// 只有本地时间，跨时区核对就对不回去；只有 UTC 毫秒，用户看不懂。
///
/// 导出范围是**全部保留期内**的记录 —— 保留策略（`RETENTION_DAYS`）之外
/// 的数据已经被删除，自然不在导出之列；这两件事共享同一个口径。
pub fn export_csv(
    db: &Database,
    dir: &Path,
    now: Timestamp,
    offset: LocalOffset,
) -> Result<ExportedFiles> {
    fs::create_dir_all(dir).map_err(|err| {
        StorageError::DataDirUnavailable(format!("无法创建导出目录 {}：{err}", dir.display()))
    })?;

    // 文件名带上导出当天的日期：导出两次不会互相覆盖，也一眼能认出哪份更新。
    let date_tag = DateWindow::day_of(now, offset)
        .format_date()
        .replace('-', "");

    // 覆盖「纪元起到今天结束」的窗口 —— 也就是保留策略内的全部数据。
    // 不引入新的 `all()` 查询，是因为 `in_window` 的左闭右开口径在这里同样成立，
    // 而且边界计算仍然只发生在 datewin 里（数据模型 §8）。
    let history = DateWindow {
        start: DateWindow::from_day_index(0, offset).start,
        end: DateWindow::day_of(now, offset).end,
        day_index: 0,
    };

    let events_csv = dir.join(format!("tacet-events-{date_tag}.csv"));
    let interventions_csv = dir.join(format!("tacet-interventions-{date_tag}.csv"));

    let events = EventRepo::in_window(db, &history)?;
    write_csv(
        &events_csv,
        &["occurred_at_local", "occurred_at_utc_ms", "kind", "payload"],
        events.iter().map(|row| {
            vec![
                format_local(row.occurred_at, offset),
                row.occurred_at.as_millis().to_string(),
                row.kind.as_str().to_string(),
                row.payload.clone(),
            ]
        }),
    )?;

    let interventions = InterventionRepo::in_window(db, &history)?;
    write_csv(
        &interventions_csv,
        &[
            "fired_at_local",
            "fired_at_utc_ms",
            "kind",
            "level",
            "outcome",
            "snooze_minutes",
            "resolved_at_local",
            "resolved_at_utc_ms",
            "reasons",
        ],
        interventions.iter().map(|row| {
            vec![
                format_local(row.fired_at, offset),
                row.fired_at.as_millis().to_string(),
                row.kind.as_str().to_string(),
                row.level.as_i64().to_string(),
                row.outcome
                    .map(|o| o.as_str().to_string())
                    .unwrap_or_default(),
                row.snooze_minutes
                    .map(|m| m.to_string())
                    .unwrap_or_default(),
                row.resolved_at
                    .map(|at| format_local(at, offset))
                    .unwrap_or_default(),
                row.resolved_at
                    .map(|at| at.as_millis().to_string())
                    .unwrap_or_default(),
                serde_json::to_string(&row.reasons).unwrap_or_else(|_| "[]".to_string()),
            ]
        }),
    )?;

    Ok(ExportedFiles {
        events_csv,
        interventions_csv,
    })
}

/// 写出一个 CSV 文件：UTF-8 BOM + 表头 + 数据行。
fn write_csv(path: &Path, header: &[&str], rows: impl Iterator<Item = Vec<String>>) -> Result<()> {
    let mut out = String::from("\u{FEFF}");
    out.push_str(&join_csv_row(header.iter().copied()));
    out.push('\n');

    for row in rows {
        out.push_str(&join_csv_row(row.iter().map(String::as_str)));
        out.push('\n');
    }

    fs::write(path, out).map_err(|err| {
        StorageError::DataDirUnavailable(format!("无法写入 {}：{err}", path.display()))
    })?;
    Ok(())
}

fn join_csv_row<'a>(fields: impl Iterator<Item = &'a str>) -> String {
    fields.map(escape_csv_field).collect::<Vec<_>>().join(",")
}

/// RFC 4180 转义：含逗号 / 引号 / 换行的字段整体加引号，字段内引号翻倍。
fn escape_csv_field(field: &str) -> String {
    if field.contains(',') || field.contains('"') || field.contains('\n') || field.contains('\r') {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_string()
    }
}

/// UTC 毫秒 → 本地时间字符串 `2026-09-29 14:03:21`。
///
/// 边界换算全部委托给 [`DateWindow`]，这里只做「天内的偏移」这一步 ——
/// 继续遵守「日期口径只在 datewin 算」的约束。
fn format_local(at: Timestamp, offset: LocalOffset) -> String {
    let day = DateWindow::day_of(at, offset);
    let (year, month, date) = civil_from_days(day.day_index());
    let seconds = at.millis_since(day.start) / 1000;
    format!(
        "{year:04}-{month:02}-{date:02} {:02}:{:02}:{:02}",
        seconds / 3600,
        seconds % 3600 / 60,
        seconds % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tacet_core::model::{BehaviorKind, InterventionLevel, NeedKind, Reason};

    use crate::repo::InterventionRepo;

    /// 每个测试用独立的临时目录，结束后清掉。
    fn temp_dir(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("tacet-export-test-{}-{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn 导出的文件带表头与全部记录() {
        let db = Database::open_in_memory().expect("打开");
        let dir = temp_dir("basic");

        EventRepo::append(&db, BehaviorKind::WaterLogged, "{}", ts(0)).expect("写入");
        EventRepo::append(&db, BehaviorKind::ActivityLogged, "{}", ts(60)).expect("写入");
        InterventionRepo::insert(
            &db,
            &tacet_core::model::Intervention::fired(
                NeedKind::Rest,
                InterventionLevel::FullScreen,
                vec![Reason::ContinuousWork { minutes: 50 }],
                ts(120),
            ),
        )
        .expect("写入");

        let files = export_csv(&db, &dir, ts(120), LocalOffset::utc()).expect("导出");

        let events = fs::read_to_string(&files.events_csv).expect("读回");
        assert!(
            events.starts_with('\u{FEFF}'),
            "必须带 BOM，否则 Excel 打开中文乱码"
        );
        assert_eq!(events.lines().count(), 3, "表头 + 2 行记录");
        assert!(events.contains("occurred_at_local,occurred_at_utc_ms,kind,payload"));
        assert!(events.contains("water.logged"));

        let interventions = fs::read_to_string(&files.interventions_csv).expect("读回");
        assert_eq!(interventions.lines().count(), 2, "表头 + 1 行记录");
        assert!(
            interventions.contains(",rest,4,"),
            "需求类型与干预等级应当按原值出现：{interventions}"
        );

        fs::remove_dir_all(&dir).expect("清理");
    }

    /// `payload` 是 JSON，几乎必然含逗号和引号 —— 转义错了，
    /// 表格软件会把一行拆成多行，用户看到的就是错的表。
    #[test]
    fn 逗号与引号按规范转义() {
        let db = Database::open_in_memory().expect("打开");
        let dir = temp_dir("escape");

        let payload = r#"{"note":"含,逗号和\"引号\""}"#;
        EventRepo::append(&db, BehaviorKind::WaterLogged, payload, ts(0)).expect("写入");

        let files = export_csv(&db, &dir, ts(0), LocalOffset::utc()).expect("导出");
        let events = fs::read_to_string(&files.events_csv).expect("读回");

        assert_eq!(events.lines().count(), 2, "转义失败会让一行裂成多行");
        assert!(
            events.contains("\"{\"\"note\"\":\"\"含,逗号和\\\"\"引号\\\"\"\"\"}\""),
            "引号必须翻倍并整体加引号：{events}"
        );

        fs::remove_dir_all(&dir).expect("清理");
    }

    #[test]
    fn 空库导出只有表头() {
        let db = Database::open_in_memory().expect("打开");
        let dir = temp_dir("empty");

        let files = export_csv(&db, &dir, ts(0), LocalOffset::utc()).expect("导出");

        let events = fs::read_to_string(&files.events_csv).expect("读回");
        assert_eq!(events.lines().count(), 1);

        fs::remove_dir_all(&dir).expect("清理");
    }

    #[test]
    fn 本地时间按偏移换算() {
        let at = ts(0); // UTC 2023-11-14 22:13:20
        let local = format_local(at, LocalOffset::from_hours(8));
        assert_eq!(local, "2023-11-15 06:13:20", "东八区应当是次日清晨");
        assert_eq!(format_local(at, LocalOffset::utc()), "2023-11-14 22:13:20");
    }

    /// 固定测试时刻：UTC 2023-11-14 22:13:20。
    fn ts(minutes: i64) -> Timestamp {
        Timestamp::from_millis(1_700_000_000_000 + minutes * 60_000)
    }
}
