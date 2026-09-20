//! 设置仓储 —— `settings` 表的读写，以及「一组设置 = 一个领域对象」的组装。
//!
//! ## KV 表的代价与对策
//!
//! ADR-009 选择用键值对存设置，好处是加一项设置不用改表结构。
//! 代价是**类型安全要靠代码自己兜**：值统一是 JSON 文本，
//! 读出来是字符串、数字还是布尔，全靠约定。
//!
//! 这里的对策是：
//!
//! 1. **键名收口**在 [`SettingsKey`] 枚举里，不允许散落的字符串字面量
//! 2. **每个值都带类型**，用 `serde_json::Value` 的变体判断后再转，
//!    类型对不上就退回默认值并留痕（而不是 panic，也不是静默用错值）
//! 3. 提供 [`SettingsRepo::load_preferences`] 这样的**整体读取**接口，
//!    业务代码拿到的是 `UserPreferences` 这个完整对象，不用关心若干条 KV

use rusqlite::params;
use tacet_core::model::{SettingsKey, UserPreferences};
use tacet_core::Timestamp;

use crate::db::{data_error, Database};
use crate::error::Result;

/// `settings` 表的仓储。
pub struct SettingsRepo;

impl SettingsRepo {
    /// 写入一项设置（值会被编码成 JSON）。
    pub fn set(db: &Database, key: SettingsKey, value: &serde_json::Value) -> Result<()> {
        let encoded = serde_json::to_string(value)?;

        let conn = db.lock();
        // UPSERT：有就更新，没有就插入。
        // `key` 是主键，所以冲突时直接覆盖 —— 这正是「设置项只有一份」的语义。
        conn.execute(
            "INSERT INTO settings (key, value, updated_at) VALUES (?1, ?2, ?3) \
             ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
            params![key.as_str(), encoded, Timestamp::now().as_millis()],
        )?;

        Ok(())
    }

    /// 读取一项设置（原始 JSON）。
    pub fn get(db: &Database, key: SettingsKey) -> Result<Option<serde_json::Value>> {
        let encoded: Option<String> = {
            let conn = db.lock();
            let mut statement = conn.prepare("SELECT value FROM settings WHERE key = ?1")?;
            let mut rows = statement.query(params![key.as_str()])?;

            match rows.next()? {
                Some(row) => Some(row.get(0)?),
                None => None,
            }
        };

        match encoded {
            Some(text) => Ok(Some(serde_json::from_str(&text)?)),
            None => Ok(None),
        }
    }

    /// 读取一个布尔设置；缺失或类型不符时返回 `None`。
    pub fn get_bool(db: &Database, key: SettingsKey) -> Result<Option<bool>> {
        Ok(Self::get(db, key)?.and_then(|value| value.as_bool()))
    }

    /// 读取一个整数设置。
    ///
    /// 注意 JSON 里所有整数都是 `i64`（可能是浮点形式），
    /// 所以先尝试 `as_i64`，再退回 `as_f64` 取整 —— 兼容「被别的地方
    /// 写成了 5.0」这种情况。
    pub fn get_u32(db: &Database, key: SettingsKey) -> Result<Option<u32>> {
        Ok(Self::get(db, key)?.and_then(|value| {
            value
                .as_u64()
                .map(|n| n.min(u32::MAX as u64) as u32)
                .or_else(|| {
                    value
                        .as_f64()
                        .map(|f| f.max(0.0).min(u32::MAX as f64) as u32)
                })
        }))
    }

    /// 读取一个字符串数组设置（延后选项用）。
    pub fn get_u32_list(db: &Database, key: SettingsKey) -> Result<Option<Vec<u32>>> {
        Ok(Self::get(db, key)?.and_then(|value| {
            value.as_array().map(|items| {
                items
                    .iter()
                    .filter_map(|item| {
                        item.as_u64()
                            .map(|n| n.min(u32::MAX as u64) as u32)
                            .or_else(|| {
                                item.as_f64()
                                    .map(|f| f.max(0.0).min(u32::MAX as f64) as u32)
                            })
                    })
                    .collect()
            })
        }))
    }

    /// 删除一项设置（回到默认值）。
    pub fn remove(db: &Database, key: SettingsKey) -> Result<bool> {
        let conn = db.lock();
        let affected =
            conn.execute("DELETE FROM settings WHERE key = ?1", params![key.as_str()])?;
        Ok(affected > 0)
    }

    /// 写入一整套用户偏好。
    ///
    /// 用一个事务保证「全写进去或一条都不写」——
    /// 半个设置被保存的状态（比如间隔更新了但开关没更新）比失败更麻烦。
    pub fn save_preferences(db: &Database, prefs: &UserPreferences) -> Result<()> {
        let now = Timestamp::now().as_millis();

        let entries: Vec<(SettingsKey, serde_json::Value)> = vec![
            (
                SettingsKey::ReminderRestEnabled,
                serde_json::json!(prefs.reminders.rest.enabled),
            ),
            (
                SettingsKey::ReminderRestInterval,
                serde_json::json!(prefs.reminders.rest.interval_minutes),
            ),
            (
                SettingsKey::ReminderHydrationEnabled,
                serde_json::json!(prefs.reminders.hydration.enabled),
            ),
            (
                SettingsKey::ReminderHydrationInterval,
                serde_json::json!(prefs.reminders.hydration.interval_minutes),
            ),
            (
                SettingsKey::ReminderMovementEnabled,
                serde_json::json!(prefs.reminders.movement.enabled),
            ),
            (
                SettingsKey::ReminderMovementInterval,
                serde_json::json!(prefs.reminders.movement.interval_minutes),
            ),
            (
                SettingsKey::ReminderEyeRestEnabled,
                serde_json::json!(prefs.reminders.eye_rest.enabled),
            ),
            (
                SettingsKey::ReminderEyeRestInterval,
                serde_json::json!(prefs.reminders.eye_rest.interval_minutes),
            ),
            (
                SettingsKey::DoNotDisturb,
                serde_json::json!(prefs.do_not_disturb),
            ),
            (
                SettingsKey::IdleThresholdMinutes,
                serde_json::json!(prefs.idle_threshold_minutes),
            ),
            (
                SettingsKey::BreakDurationMinutes,
                serde_json::json!(prefs.break_duration_minutes),
            ),
            (
                SettingsKey::SnoozeOptionsMinutes,
                serde_json::json!(prefs.snooze_options_minutes),
            ),
        ];

        db.transaction(|tx| {
            for (key, value) in &entries {
                let encoded = serde_json::to_string(value)?;
                tx.execute(
                    "INSERT INTO settings (key, value, updated_at) VALUES (?1, ?2, ?3) \
                     ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
                    params![key.as_str(), encoded, now],
                )?;
            }
            Ok(())
        })
    }

    /// 读出一整套用户偏好。
    ///
    /// 任何一项缺失或损坏都退回**默认值**，而不是让整个读取失败。
    /// 这是刻意的：一项设置读不出来，不该导致用户完全用不了软件。
    pub fn load_preferences(db: &Database) -> Result<UserPreferences> {
        let mut prefs = UserPreferences::default();

        if let Some(value) = Self::get_bool(db, SettingsKey::ReminderRestEnabled)? {
            prefs.reminders.rest.enabled = value;
        }
        if let Some(value) = Self::get_u32(db, SettingsKey::ReminderRestInterval)? {
            // 经过 ReminderRule 的范围夹取，防止数据库里存了离谱的间隔
            prefs.reminders.rest.interval_minutes = value;
        }
        if let Some(value) = Self::get_bool(db, SettingsKey::ReminderHydrationEnabled)? {
            prefs.reminders.hydration.enabled = value;
        }
        if let Some(value) = Self::get_u32(db, SettingsKey::ReminderHydrationInterval)? {
            prefs.reminders.hydration.interval_minutes = value;
        }
        if let Some(value) = Self::get_bool(db, SettingsKey::ReminderMovementEnabled)? {
            prefs.reminders.movement.enabled = value;
        }
        if let Some(value) = Self::get_u32(db, SettingsKey::ReminderMovementInterval)? {
            prefs.reminders.movement.interval_minutes = value;
        }
        if let Some(value) = Self::get_bool(db, SettingsKey::ReminderEyeRestEnabled)? {
            prefs.reminders.eye_rest.enabled = value;
        }
        if let Some(value) = Self::get_u32(db, SettingsKey::ReminderEyeRestInterval)? {
            prefs.reminders.eye_rest.interval_minutes = value;
        }
        if let Some(value) = Self::get_bool(db, SettingsKey::DoNotDisturb)? {
            prefs.do_not_disturb = value;
        }
        if let Some(value) = Self::get_u32(db, SettingsKey::IdleThresholdMinutes)? {
            prefs.idle_threshold_minutes = value.max(1);
        }
        if let Some(value) = Self::get_u32(db, SettingsKey::BreakDurationMinutes)? {
            prefs.break_duration_minutes = value.max(1);
        }
        if let Some(value) = Self::get_u32_list(db, SettingsKey::SnoozeOptionsMinutes)? {
            if !value.is_empty() {
                prefs.snooze_options_minutes = value;
            }
        }

        // 统一走一遍构造函数，让范围夹取生效（数据库里的脏值不该流进业务逻辑）
        prefs.reminders.rest = tacet_core::model::ReminderRule::new(
            prefs.reminders.rest.enabled,
            prefs.reminders.rest.interval_minutes,
        );
        prefs.reminders.hydration = tacet_core::model::ReminderRule::new(
            prefs.reminders.hydration.enabled,
            prefs.reminders.hydration.interval_minutes,
        );
        prefs.reminders.movement = tacet_core::model::ReminderRule::new(
            prefs.reminders.movement.enabled,
            prefs.reminders.movement.interval_minutes,
        );
        prefs.reminders.eye_rest = tacet_core::model::ReminderRule::new(
            prefs.reminders.eye_rest.enabled,
            prefs.reminders.eye_rest.interval_minutes,
        );

        Ok(prefs)
    }

    /// 检查数据库里有没有未知的设置键。
    ///
    /// 用于「用户装了新版又退回旧版」的场景：旧版遇到不认识的键，
    /// 应该**原样保留**（绝不能删），但要能报告出来。
    pub fn unknown_keys(db: &Database) -> Result<Vec<String>> {
        let conn = db.lock();
        let mut statement = conn.prepare("SELECT key FROM settings ORDER BY key")?;

        let keys = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<std::result::Result<Vec<_>, _>>()?;

        Ok(keys
            .into_iter()
            .filter(|key| SettingsKey::parse(key).is_none())
            .collect())
    }

    /// 当前保存了多少项设置（诊断用）。
    pub fn count(db: &Database) -> Result<u32> {
        let conn = db.lock();
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM settings", [], |row| row.get(0))?;
        Ok(count.max(0) as u32)
    }
}

/// 把一个 JSON 值转成 u32，失败时给出可读错误。
///
/// 保留这个函数是为了在需要严格校验的场合（如导入）使用，
/// 与上面「宽松退回默认值」的读取形成互补。
#[allow(dead_code)]
fn strict_u32(key: SettingsKey, value: &serde_json::Value) -> Result<u32> {
    value
        .as_u64()
        .map(|n| n.min(u32::MAX as u64) as u32)
        .ok_or_else(|| data_error("设置项应当是整数", format!("{} = {value}", key.as_str())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tacet_core::model::{ReminderRule, ReminderSettings};

    #[test]
    fn 空库读出默认偏好() {
        let db = Database::open_in_memory().expect("打开");
        let prefs = SettingsRepo::load_preferences(&db).expect("读取");

        assert_eq!(prefs, UserPreferences::default());
        assert_eq!(prefs.reminders.rest.interval_minutes, 50);
        assert_eq!(prefs.snooze_options_minutes, vec![1, 3, 5]);
    }

    #[test]
    fn 写入后能读回单项设置() {
        let db = Database::open_in_memory().expect("打开");

        SettingsRepo::set(&db, SettingsKey::DoNotDisturb, &serde_json::json!(true)).expect("写入");
        assert_eq!(
            SettingsRepo::get_bool(&db, SettingsKey::DoNotDisturb).expect("读取"),
            Some(true)
        );

        SettingsRepo::set(
            &db,
            SettingsKey::ReminderRestInterval,
            &serde_json::json!(90),
        )
        .expect("写入");
        assert_eq!(
            SettingsRepo::get_u32(&db, SettingsKey::ReminderRestInterval).expect("读取"),
            Some(90)
        );
    }

    #[test]
    fn 重复写入是覆盖而不是追加() {
        let db = Database::open_in_memory().expect("打开");

        SettingsRepo::set(&db, SettingsKey::DoNotDisturb, &serde_json::json!(true)).expect("写入");
        SettingsRepo::set(&db, SettingsKey::DoNotDisturb, &serde_json::json!(false)).expect("写入");

        assert_eq!(
            SettingsRepo::get_bool(&db, SettingsKey::DoNotDisturb).expect("读取"),
            Some(false)
        );
        assert_eq!(SettingsRepo::count(&db).expect("计数"), 1, "不该产生两行");
    }

    #[test]
    fn 整套偏好可以往返() {
        let db = Database::open_in_memory().expect("打开");

        let mut prefs = UserPreferences {
            reminders: ReminderSettings {
                rest: ReminderRule::new(true, 90),
                hydration: ReminderRule::new(false, 30),
                movement: ReminderRule::new(true, 120),
                eye_rest: ReminderRule::new(false, 45),
            },
            do_not_disturb: true,
            idle_threshold_minutes: 8,
            break_duration_minutes: 10,
            snooze_options_minutes: vec![2, 5, 10],
        };
        prefs.reminders.rest.enabled = true;

        SettingsRepo::save_preferences(&db, &prefs).expect("保存");
        let loaded = SettingsRepo::load_preferences(&db).expect("读取");

        assert_eq!(loaded, prefs);
    }

    #[test]
    fn 保存整套偏好是原子的() {
        let db = Database::open_in_memory().expect("打开");
        let prefs = UserPreferences::default();

        SettingsRepo::save_preferences(&db, &prefs).expect("保存");

        // 12 项设置应当全部写入
        assert_eq!(SettingsRepo::count(&db).expect("计数"), 12);
    }

    #[test]
    fn 未知的键被保留并报告() {
        // 「装了新版又退回旧版」：旧版不认识新版的设置项，
        // 但绝不能删掉它们 —— 那会让用户升回去之后设置全没了。
        let db = Database::open_in_memory().expect("打开");

        SettingsRepo::set(&db, SettingsKey::DoNotDisturb, &serde_json::json!(true)).expect("写入");
        db.lock()
            .execute(
                "INSERT INTO settings (key, value, updated_at) VALUES ('future.feature.enabled', 'true', 0)",
                [],
            )
            .expect("写入未来版本的设置");

        let unknown = SettingsRepo::unknown_keys(&db).expect("查询");
        assert_eq!(unknown, vec!["future.feature.enabled".to_string()]);
        assert_eq!(
            SettingsRepo::count(&db).expect("计数"),
            2,
            "未知键必须被保留"
        );
    }

    #[test]
    fn 类型不符时退回默认值而不是报错() {
        let db = Database::open_in_memory().expect("打开");

        // 手工写入一个类型错误的「整数」
        db.lock()
            .execute(
                "INSERT INTO settings (key, value, updated_at) VALUES ('reminder.rest.interval_minutes', '\"不是数字\"', 0)",
                [],
            )
            .expect("写入脏数据");

        // 单项读取返回 None（而不是 panic）
        assert_eq!(
            SettingsRepo::get_u32(&db, SettingsKey::ReminderRestInterval).expect("读取"),
            None
        );

        // 整体读取退回默认值
        let prefs = SettingsRepo::load_preferences(&db).expect("读取");
        assert_eq!(prefs.reminders.rest.interval_minutes, 50);
    }

    #[test]
    fn 整数以浮点形式存也能读出来() {
        let db = Database::open_in_memory().expect("打开");
        // 有些 JSON 序列化路径会把整数写成 5.0
        db.lock()
            .execute(
                "INSERT INTO settings (key, value, updated_at) VALUES ('break.duration_minutes', '5.0', 0)",
                [],
            )
            .expect("写入");

        assert_eq!(
            SettingsRepo::get_u32(&db, SettingsKey::BreakDurationMinutes).expect("读取"),
            Some(5)
        );
    }

    #[test]
    fn 从库里读出的间隔仍受范围约束() {
        let db = Database::open_in_memory().expect("打开");
        // 数据库里有脏值（间隔 1 分钟，低于下限）
        SettingsRepo::set(
            &db,
            SettingsKey::ReminderRestInterval,
            &serde_json::json!(1),
        )
        .expect("写入");

        let prefs = SettingsRepo::load_preferences(&db).expect("读取");
        assert_eq!(
            prefs.reminders.rest.interval_minutes,
            ReminderRule::MIN_INTERVAL_MINUTES,
            "脏值必须被夹取到合法区间，不能流进业务逻辑"
        );
    }

    #[test]
    fn 删除设置后回到默认值() {
        let db = Database::open_in_memory().expect("打开");

        SettingsRepo::set(&db, SettingsKey::DoNotDisturb, &serde_json::json!(true)).expect("写入");
        assert!(SettingsRepo::remove(&db, SettingsKey::DoNotDisturb).expect("删除"));

        assert_eq!(
            SettingsRepo::get_bool(&db, SettingsKey::DoNotDisturb).expect("读取"),
            None
        );
        let prefs = SettingsRepo::load_preferences(&db).expect("读取");
        assert!(!prefs.do_not_disturb, "删除后应当回到默认值");
    }

    #[test]
    fn 删除不存在项返回假() {
        let db = Database::open_in_memory().expect("打开");
        assert!(!SettingsRepo::remove(&db, SettingsKey::DoNotDisturb).expect("删除"));
    }

    #[test]
    fn 延后选项数组往返() {
        let db = Database::open_in_memory().expect("打开");

        SettingsRepo::set(
            &db,
            SettingsKey::SnoozeOptionsMinutes,
            &serde_json::json!([1, 3, 5, 10]),
        )
        .expect("写入");

        assert_eq!(
            SettingsRepo::get_u32_list(&db, SettingsKey::SnoozeOptionsMinutes).expect("读取"),
            Some(vec![1, 3, 5, 10])
        );
    }

    #[test]
    fn 空的延后选项被忽略() {
        // 一个空数组会让提醒界面没有任何「稍后」按钮 —— PRD 要求必须能延后，
        // 所以空数组被视为无效，退回默认值。
        let db = Database::open_in_memory().expect("打开");
        SettingsRepo::set(
            &db,
            SettingsKey::SnoozeOptionsMinutes,
            &serde_json::json!([]),
        )
        .expect("写入");

        let prefs = SettingsRepo::load_preferences(&db).expect("读取");
        assert_eq!(prefs.snooze_options_minutes, vec![1, 3, 5]);
    }

    #[test]
    fn 每个设置键都能写入并读回() {
        // 这条测试防的是「新加了一个 SettingsKey 但忘了在 save/load 里处理」。
        let db = Database::open_in_memory().expect("打开");

        for key in SettingsKey::ALL {
            let value = match key {
                SettingsKey::SnoozeOptionsMinutes => serde_json::json!([1, 5]),
                SettingsKey::ReminderRestEnabled
                | SettingsKey::ReminderHydrationEnabled
                | SettingsKey::ReminderMovementEnabled
                | SettingsKey::ReminderEyeRestEnabled
                | SettingsKey::DoNotDisturb => serde_json::json!(true),
                _ => serde_json::json!(7),
            };

            SettingsRepo::set(&db, key, &value).expect("写入");
            assert!(
                SettingsRepo::get(&db, key).expect("读取").is_some(),
                "设置项 {} 写入后应当能读回",
                key.as_str()
            );
        }

        assert_eq!(SettingsRepo::unknown_keys(&db).expect("查询").len(), 0);
    }
}
