//! 统一日期口径 —— 「今天」到底从哪一刻开始。
//!
//! ## 为什么这个文件必须存在
//!
//! 数据模型 §8 的工程约束写得很直接：
//!
//! > 所有"今天/本周/几点"的计算必须在 storage 层用统一的日期工具函数完成，
//! > UI 层禁止自行计算日期边界（历史教训：时区口径混用会造成统计错位）。
//!
//! 这句话背后是一个具体的坑。数据库里存的是 UTC 毫秒（这是对的，跨时区不会歧义），
//! 但「今天喝了 6 次水」里的「今天」是**用户所在时区的自然日**。
//! 如果界面自己拿 UTC 去切日，中国用户晚上 8 点以后的行为就会被算进「明天」——
//! 于是晚上看一眼统计，数字莫名其妙是 0。
//!
//! 唯一正确的做法是：**把 UTC 时间戳 + 本地时区偏移，换算成本地自然日区间**，
//! 而且只在一个地方算。这里就是那个地方。
//!
//! ## 为什么不用日期库
//!
//! 我们真正需要的只有三件事：
//!
//! 1. 某个时刻落在哪个本地自然日
//! 2. 那个自然日的起止时间戳
//! 3. 把日期格式化成 `2026-09-20` 给人看
//!
//! 这三件事用「纪元日索引 + 一个公历换算公式」就能做到，不需要引入日期库。
//! 好处是存储层保持零额外依赖，而且**这个换算过程完全可测** ——
//! 下面的测试覆盖了闰年、月末、年末、东西半球时区与跨年边界。
//!
//! ## 已知的简化（诚实记录）
//!
//! 我们用一个**固定偏移**计算整天的边界。真正的本地时区在夏令时切换的那两天
//! 会变一小时，所以那两天的边界可能差 60 分钟。
//!
//! 这个简化是可接受的：夏令时切换发生在凌晨 2~3 点，而我们的统计对象是
//! 白天的工作行为；切换日最多影响凌晨那两小时的归属。
//! v0.3 引入分时段策略时会替换为「按天查真实偏移」的实现。

use serde::{Deserialize, Serialize};
use tacet_core::time::{Timestamp, MINUTE};

/// 一天的毫秒数。
const MS_PER_DAY: i64 = 86_400_000;

/// 本地时区相对 UTC 的偏移。
///
/// 用「分钟」而不是「小时」做单位，因为世界上确实存在 30 分钟和 45 分钟时区
/// （印度 +5:30、尼泊尔 +5:45）。用小时当单位会在这些地方静默出错。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LocalOffset {
    /// 本地时间比 UTC 快多少分钟（东八区为 480）。
    minutes: i32,
}

impl Default for LocalOffset {
    fn default() -> Self {
        Self::utc()
    }
}

impl LocalOffset {
    /// UTC（偏移 0）。
    pub const fn utc() -> Self {
        Self { minutes: 0 }
    }

    /// 从分钟构造，并夹取到 ±18 小时（现实中存在的时区范围）。
    pub fn from_minutes(minutes: i32) -> Self {
        let bound = 18 * 60;
        Self {
            minutes: minutes.clamp(-bound, bound),
        }
    }

    /// 从小时构造（东八区传 8）。
    pub fn from_hours(hours: i32) -> Self {
        Self::from_minutes(hours * 60)
    }

    /// 偏移多少分钟。
    pub const fn minutes(self) -> i32 {
        self.minutes
    }

    /// 偏移多少毫秒。
    pub const fn millis(self) -> i64 {
        self.minutes as i64 * MINUTE
    }

    /// 界面显示形式，如 `UTC+08:00`。
    pub fn display_name(self) -> String {
        let sign = if self.minutes < 0 { '-' } else { '+' };
        let abs = self.minutes.abs();
        format!("UTC{sign}{:02}:{:02}", abs / 60, abs % 60)
    }
}

/// 一个本地自然日区间（含起点，不含终点）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DateWindow {
    /// 起始时刻（本地 00:00:00.000 对应的 UTC 时间戳）。
    pub start: Timestamp,
    /// 结束时刻（次日本地 00:00 对应的 UTC 时间戳）。
    pub end: Timestamp,
    /// 这一天距 1970-01-01 的天数（本地口径）。
    pub day_index: i64,
}

impl DateWindow {
    /// 区间长度（毫秒）。固定偏移下必然是一天。
    pub const fn duration_ms(&self) -> i64 {
        self.end.millis_since(self.start)
    }

    /// 某个时刻是否落在这个区间里。
    pub const fn contains(&self, at: Timestamp) -> bool {
        at.as_millis() >= self.start.as_millis() && at.as_millis() < self.end.as_millis()
    }

    /// 这一天是「今天」吗。
    pub fn is_today(&self, now: Timestamp, offset: LocalOffset) -> bool {
        Self::day_of(now, offset).day_index == self.day_index
    }

    /// 把这一天格式化成 `2026-09-20`。
    pub fn format_date(&self) -> String {
        let (year, month, day) = civil_from_days(self.day_index);
        format!("{year:04}-{month:02}-{day:02}")
    }

    /// 这一天的本地自然日序号。
    pub const fn day_index(&self) -> i64 {
        self.day_index
    }

    /// 定位某个时刻所在的本地自然日。
    pub fn day_of(at: Timestamp, offset: LocalOffset) -> Self {
        // 先平移到「本地时间轴」，取整日索引，再平移回 UTC。
        let local_ms = at.as_millis() + offset.millis();
        let day_index = local_ms.div_euclid(MS_PER_DAY);
        Self::from_day_index(day_index, offset)
    }

    /// 由本地日索引构造区间。
    pub fn from_day_index(day_index: i64, offset: LocalOffset) -> Self {
        let start_local = day_index * MS_PER_DAY;
        let end_local = start_local + MS_PER_DAY;

        Self {
            start: Timestamp::from_millis(start_local - offset.millis()),
            end: Timestamp::from_millis(end_local - offset.millis()),
            day_index,
        }
    }

    /// 这一天的本地零点对应的时刻。
    pub const fn local_midnight(&self) -> Timestamp {
        self.start
    }

    /// 在区间内按本地时间取某个小时整点。
    ///
    /// 用于「今天 14 点前后」这类分时段查询。`hour` 超出 0~23 时返回 `None`，
    /// 而不是悄悄滚到第二天 —— 静默的越界是统计错位最常见的来源。
    pub fn local_hour_mark(&self, hour: u32) -> Option<Timestamp> {
        if hour > 23 {
            return None;
        }
        Some(self.start.saturating_add_millis(hour as i64 * 60 * MINUTE))
    }

    /// 把一天切成 `buckets` 个等长小段（用于按小时聚合的图表）。
    pub fn split(&self, buckets: u32) -> Vec<DateWindow> {
        if buckets == 0 {
            return Vec::new();
        }

        let span = self.duration_ms();
        let step = span.div_euclid(buckets as i64);

        (0..buckets as i64)
            .map(|i| {
                let start = self.start.saturating_add_millis(i * step);
                let end = if i == buckets as i64 - 1 {
                    self.end
                } else {
                    self.start.saturating_add_millis((i + 1) * step)
                };
                DateWindow {
                    start,
                    end,
                    day_index: self.day_index,
                }
            })
            .collect()
    }
}

/// 一周的第一天。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WeekStart {
    /// 周一（中国大陆与欧洲习惯）。
    Monday,
    /// 周日（美式习惯）。
    Sunday,
}

impl WeekStart {
    /// 从日索引（1970-01-01 是周四）推算一周的第几天，0 表示「周首」。
    fn day_ordinal(self, day_index: i64) -> i64 {
        // 1970-01-01 是星期四。以周一为一周之首时，它是第 4 天（0-based 为 3）。
        let weekday_from_monday = (day_index + 3).rem_euclid(7);
        match self {
            WeekStart::Monday => weekday_from_monday,
            WeekStart::Sunday => (weekday_from_monday + 1).rem_euclid(7),
        }
    }

    /// 定位某个时刻所在的自然周。
    pub fn week_of(self, at: Timestamp, offset: LocalOffset) -> DateWindow {
        let day = DateWindow::day_of(at, offset);
        let start_index = day.day_index - self.day_ordinal(day.day_index);

        DateWindow {
            start: DateWindow::from_day_index(start_index, offset).start,
            end: DateWindow::from_day_index(start_index + 7, offset).start,
            day_index: start_index,
        }
    }

    /// 这一周包含哪些天（7 个日区间）。
    pub fn days_of(self, at: Timestamp, offset: LocalOffset) -> Vec<DateWindow> {
        let week = self.week_of(at, offset);
        (0..7)
            .map(|i| DateWindow::from_day_index(week.day_index + i, offset))
            .collect()
    }
}

/// 由「自 1970-01-01 起的天数」推回公历年月日。
///
/// 这是 Howard Hinnant 提出的 `civil_from_days` 算法：把纪元日转换成公历日期。
/// 它的正确性来自「把 3 月当作一年的开始」这个技巧 —— 这样闰日就正好落在年末，
/// 一个月内的天数模式变得规整，不需要任何查表或分支。
///
/// 顺带一提：**这个算法的形式恰好对应「为什么闰年规则是 4 年一闰、
/// 百年不闰、四百年再闰」** —— `-1/4 + 1/100 - 1/400` 那几项就是那三条规则。
pub fn civil_from_days(days: i64) -> (i64, u32, u32) {
    // 把纪元平移到 0000-03-01（让闰日成为「上一年的最后一天」）
    let z = days + 719_468;

    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let day_of_era = z - era * 146_097; // [0, 146096]

    // 下面的整数除法都在做「起点的月序」
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;

    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);

    // 把「3 月起始」的月序换算回 1~12 月
    let mp = (5 * day_of_year + 2) / 153;
    let day = (day_of_year - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;

    (year + i64::from(month <= 2), month, day)
}

/// 由公历年月日推出「自 1970-01-01 起的天数」。
pub fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]

    let mp = if month > 2 { month - 3 } else { month + 9 } as i64;
    let doy = (153 * mp + 2) / 5 + day as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;

    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 东八区（中国标准时间）。
    fn cst() -> LocalOffset {
        LocalOffset::from_hours(8)
    }

    /// 把 `YYYY-MM-DDTHH:MM:SSZ` 形式的字面量转成时间戳（只支持测试里写死的这几种形态）。
    ///
    /// 这里刻意手写解析而不是引入日期库：测试辅助代码不值得为它加一个依赖。
    fn ts(iso_like: &str) -> Timestamp {
        let text = iso_like.trim_end_matches('Z');
        let (date_part, time_part) = text.split_once('T').expect("缺少 T 分隔符");

        let mut date_iter = date_part.split('-');
        let year: i64 = date_iter.next().expect("缺年").parse().expect("年不是数字");
        let month: u32 = date_iter.next().expect("缺月").parse().expect("月不是数字");
        let day: u32 = date_iter.next().expect("缺日").parse().expect("日不是数字");

        let mut time_iter = time_part.split(':');
        let hour: i64 = time_iter.next().expect("缺时").parse().expect("时不是数字");
        let minute: i64 = time_iter.next().unwrap_or("0").parse().expect("分不是数字");
        let second: i64 = time_iter.next().unwrap_or("0").parse().expect("秒不是数字");

        let days = days_from_civil(year, month, day);
        Timestamp::from_millis(
            days * MS_PER_DAY + hour * 3_600_000 + minute * 60_000 + second * 1000,
        )
    }

    #[test]
    fn 公历换算往返一致() {
        for (year, month, day) in [
            (1970, 1, 1),
            (2000, 2, 29),
            (2024, 2, 29),
            (2026, 9, 20),
            (1999, 12, 31),
            (2100, 3, 1),
        ] {
            let days = days_from_civil(year, month, day);
            assert_eq!(
                civil_from_days(days),
                (year, month, day),
                "{year}-{month}-{day} 往返不一致"
            );
        }
    }

    #[test]
    fn 纪元当天是零() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(civil_from_days(0), (1970, 1, 1));
    }

    #[test]
    fn 闰年判定正确() {
        // 2024 是闰年（2 月有 29 天）
        let feb28 = days_from_civil(2024, 2, 28);
        let feb29 = days_from_civil(2024, 2, 29);
        let mar1 = days_from_civil(2024, 3, 1);
        assert_eq!(feb29 - feb28, 1);
        assert_eq!(mar1 - feb29, 1);

        // 2026 不是闰年（2 月 28 日直接接 3 月 1 日）
        let feb28 = days_from_civil(2026, 2, 28);
        let mar1 = days_from_civil(2026, 3, 1);
        assert_eq!(mar1 - feb28, 1);

        // 1900 不是闰年（百年不闰）
        assert_eq!(
            days_from_civil(1900, 3, 1) - days_from_civil(1900, 2, 28),
            1
        );
        // 2000 是闰年（四百年再闰）
        assert_eq!(
            days_from_civil(2000, 2, 29) - days_from_civil(2000, 2, 28),
            1
        );
    }

    #[test]
    fn 东八区的自然日边界() {
        // 2026-09-20 00:00 UTC = 北京时间 08:00，属于北京时间的 9 月 20 日
        let at = ts("2026-09-20T00:00:00Z");
        let day = DateWindow::day_of(at, cst());

        assert_eq!(day.format_date(), "2026-09-20");
        // 北京时间的 9 月 20 日 00:00 = UTC 9 月 19 日 16:00
        assert_eq!(
            day.start.as_millis(),
            days_from_civil(2026, 9, 19) * MS_PER_DAY + 16 * 3_600_000
        );
        assert!(day.contains(at));
    }

    #[test]
    fn 同一时刻在东西半球属于不同的自然日() {
        // UTC 2026-09-20 00:30 → 北京 09-20 08:30；纽约（-4）09-19 20:30
        let at = Timestamp::from_millis(days_from_civil(2026, 9, 20) * MS_PER_DAY + 30 * MINUTE);

        assert_eq!(DateWindow::day_of(at, cst()).format_date(), "2026-09-20");
        assert_eq!(
            DateWindow::day_of(at, LocalOffset::from_hours(-4)).format_date(),
            "2026-09-19",
            "西半球应当还在前一天"
        );
    }

    #[test]
    fn 跨午夜的时刻归属次日() {
        // 北京时间 2026-09-20 23:59
        let late = Timestamp::from_millis(
            days_from_civil(2026, 9, 20) * MS_PER_DAY + 23 * 3_600_000 + 59 * MINUTE
                - 8 * 3_600_000,
        );
        assert_eq!(DateWindow::day_of(late, cst()).format_date(), "2026-09-20");

        // 再过两分钟就是次日
        let next_day = late.saturating_add_millis(2 * MINUTE);
        assert_eq!(
            DateWindow::day_of(next_day, cst()).format_date(),
            "2026-09-21"
        );
    }

    #[test]
    fn 跨年边界() {
        let nye = Timestamp::from_millis(
            days_from_civil(2026, 12, 31) * MS_PER_DAY + 23 * 3_600_000 - 8 * 3_600_000,
        );
        assert_eq!(DateWindow::day_of(nye, cst()).format_date(), "2026-12-31");

        let new_year = nye.saturating_add_millis(2 * 3_600_000);
        assert_eq!(
            DateWindow::day_of(new_year, cst()).format_date(),
            "2027-01-01"
        );
    }

    #[test]
    fn 区间长度恰好是一天() {
        let day = DateWindow::day_of(ts("2026-09-20T00:00:00Z"), cst());
        assert_eq!(day.duration_ms(), MS_PER_DAY);
        assert_eq!(day.end.millis_since(day.start), MS_PER_DAY);
    }

    #[test]
    fn 相邻两天首尾相接不重叠() {
        let day1 = DateWindow::day_of(ts("2026-09-20T00:00:00Z"), cst());
        let day2 = DateWindow::day_of(day1.end, cst());

        assert_eq!(day1.end, day2.start, "前一天结束就是后一天开始");
        assert_eq!(day2.day_index - day1.day_index, 1);
        assert!(!day1.contains(day2.start), "区间左闭右开");
        assert!(day1.contains(day1.start));
        assert!(!day1.contains(day1.end));
    }

    #[test]
    fn 今天判定() {
        let now = ts("2026-09-20T00:00:00Z");
        let today = DateWindow::day_of(now, cst());
        let tomorrow = DateWindow::from_day_index(today.day_index + 1, cst());

        assert!(today.is_today(now, cst()));
        assert!(!tomorrow.is_today(now, cst()));
    }

    #[test]
    fn 周一为一周之首() {
        // 2026-09-20 是周日
        let sunday = ts("2026-09-20T12:00:00Z");
        let week = WeekStart::Monday.week_of(sunday, cst());

        // 包含它的这一周应当从 9 月 14 日（周一）开始
        assert_eq!(week.format_date(), "2026-09-14");
        assert!(week.contains(sunday));
        assert_eq!(week.duration_ms(), 7 * MS_PER_DAY);
    }

    #[test]
    fn 周日为一周之首() {
        let sunday = ts("2026-09-20T12:00:00Z");
        let week = WeekStart::Sunday.week_of(sunday, cst());

        assert_eq!(
            week.format_date(),
            "2026-09-20",
            "美式习惯里周日是新一周第一天"
        );
    }

    #[test]
    fn 一周恰好七天且连续() {
        let days = WeekStart::Monday.days_of(ts("2026-09-20T12:00:00Z"), cst());
        assert_eq!(days.len(), 7);

        for pair in days.windows(2) {
            assert_eq!(pair[0].end, pair[1].start, "一周内的天必须首尾相接");
        }
        assert_eq!(days[0].format_date(), "2026-09-14");
        assert_eq!(days[6].format_date(), "2026-09-20");
    }

    #[test]
    fn 周一时刻本身归属本周而不是上周() {
        // 这是个经典 off-by-one：周一那天算「本周」还是「上周」？
        // 2026-09-14 是周一
        let monday =
            Timestamp::from_millis(days_from_civil(2026, 9, 14) * MS_PER_DAY + 10 * 3_600_000);
        let week = WeekStart::Monday.week_of(monday, cst());

        assert_eq!(week.format_date(), "2026-09-14");
        assert_eq!(week.day_index, days_from_civil(2026, 9, 14));
    }

    #[test]
    fn 按小时定位整点() {
        let day = DateWindow::day_of(ts("2026-09-20T00:00:00Z"), cst());
        let hour14 = day.local_hour_mark(14).expect("14 点在范围内");

        assert_eq!(hour14.millis_since(day.start), 14 * 3_600_000);
        assert!(day.contains(hour14));

        // 越界返回 None，而不是悄悄滚到第二天
        assert_eq!(day.local_hour_mark(24), None);
        assert_eq!(day.local_hour_mark(99), None);
    }

    #[test]
    fn 分桶覆盖整天且无缝隙() {
        let day = DateWindow::day_of(ts("2026-09-20T00:00:00Z"), cst());
        let buckets = day.split(24);

        assert_eq!(buckets.len(), 24);
        assert_eq!(buckets[0].start, day.start);
        assert_eq!(buckets[23].end, day.end);

        for pair in buckets.windows(2) {
            assert_eq!(pair[0].end, pair[1].start, "分桶之间不能有缝隙");
        }
    }

    #[test]
    fn 分桶数为零时返回空() {
        let day = DateWindow::day_of(ts("2026-09-20T00:00:00Z"), cst());
        assert!(day.split(0).is_empty());
    }

    #[test]
    fn 半小时时区也能正确处理() {
        // 印度 +5:30
        let offset = LocalOffset::from_minutes(330);
        assert_eq!(offset.display_name(), "UTC+05:30");

        let day = DateWindow::day_of(ts("2026-09-20T00:00:00Z"), offset);
        assert_eq!(day.duration_ms(), MS_PER_DAY);
    }

    #[test]
    fn 偏移被夹取到现实范围() {
        assert_eq!(LocalOffset::from_hours(100).minutes(), 18 * 60);
        assert_eq!(LocalOffset::from_hours(-100).minutes(), -18 * 60);
        // 中国标准时间不受影响
        assert_eq!(cst().minutes(), 480);
    }

    #[test]
    fn 偏移显示形式() {
        assert_eq!(LocalOffset::utc().display_name(), "UTC+00:00");
        assert_eq!(cst().display_name(), "UTC+08:00");
        assert_eq!(LocalOffset::from_hours(-5).display_name(), "UTC-05:00");
        assert_eq!(LocalOffset::from_minutes(345).display_name(), "UTC+05:45");
    }

    #[test]
    fn 统计口径示例_北京时间晚上仍在同一天() {
        // 这是文档里点名的那个坑：北京时间 20:00 = UTC 12:00，必须算「今天」。
        let utc_noon =
            Timestamp::from_millis(days_from_civil(2026, 9, 20) * MS_PER_DAY + 12 * 3_600_000);
        let now = utc_noon;
        let today = DateWindow::day_of(now, cst());

        // 晚上 20:00 的喝水记录
        let water_at = utc_noon;
        assert!(
            today.contains(water_at),
            "北京时间晚上 8 点的记录必须算在今天，而不是明天"
        );
        assert_eq!(today.format_date(), "2026-09-20");
    }
}
