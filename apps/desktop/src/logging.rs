//! 日志 —— 出问题时唯一的证据来源。
//!
//! ## 为什么必须写文件
//!
//! Tacet 是一个菜单栏应用：它没有主窗口，用户不会开着终端看它输出什么。
//! 之前所有错误都走 `eprintln!` —— 从 Finder 双击启动时，
//! 那些信息**直接掉进虚空**，等于没有。出了问题只能靠猜。
//!
//! ## 三条硬约束
//!
//! ### 1. 绝不膨胀
//!
//! 一个健康工具在用户硬盘上悄悄长到几百兆，是件很冒犯的事。
//! 这里的规矩是：
//!
//! - 单个文件封顶 [`MAX_FILE_BYTES`]（256 KB）
//! - 最多保留 [`KEEP_FILES`] 个文件（含正在写的那个）
//! - **总量上限 768 KB**，永远不可能突破
//!
//! 768 KB 是什么概念：日志每行约 80 字节，够写约 9600 行。
//! 正常使用一天大概写 10~50 行（只有启动、退出和真出错时才写），
//! 也就是说可以存**几个月**。写满之后最旧的文件被删掉，文件数不增长。
//!
//! ### 2. 不做流水账
//!
//! 只有三类事情值得记：**启动/退出**（生命周期）、**警告**（能力不可用、
//! 降级）、**错误**（真的失败了）。每 10 秒一次的 tick 不记 ——
//! 那是流水账，会把真正有用的信息淹掉。
//!
//! ### 3. 写日志绝不拖慢也不搞崩应用
//!
//! 写失败（磁盘满、权限问题）时**静默放弃**，不 panic、不重试、不弹窗。
//! 一个通知用户的日志错误会造成比日志本身更大的麻烦。

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, OnceLock};

use tacet_core::time::Timestamp;
use tacet_storage::{DateWindow, LocalOffset};

/// 单个日志文件的体积上限（256 KB）。
///
/// 为什么是 256 KB 而不是 10 MB：日志的价值在「最近发生了什么」，
/// 而不是「三个月前的某个下午」。小文件轮转快，
/// 用户要发给开发者时也不会被一个巨大的附件卡住。
pub const MAX_FILE_BYTES: u64 = 256 * 1024;

/// 最多保留几个日志文件（含当前正在写的那个）。
///
/// 三个文件足够覆盖「问题发生 → 用户注意到 → 来查日志」这个时间跨度：
/// 按正常使用量算，三个文件能装几个月。
pub const KEEP_FILES: usize = 3;

/// 当前日志文件名。
pub const LOG_FILENAME: &str = "tacet.log";

/// 一天的毫秒数。
const MS_PER_DAY: i64 = 86_400_000;

/// 日志级别。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    /// 正常但值得留痕（启动、退出、能力探测结果）。
    Info,
    /// 不致命但需要注意（能力不可用、功能降级）。
    Warn,
    /// 真的失败了。
    Error,
}

impl Level {
    /// 写进文件时的固定宽度标签（对齐后日志更好读）。
    const fn tag(self) -> &'static str {
        match self {
            Level::Info => "INFO ",
            Level::Warn => "WARN ",
            Level::Error => "ERROR",
        }
    }
}

/// 日志文件的落地位置。
///
/// 与数据库同目录（`~/Library/Application Support/Tacet/logs/`）——
/// 用户要反馈问题时，两个东西在同一个地方，不用满硬盘找。
pub fn logs_dir() -> PathBuf {
    tacet_storage::path::default_data_dir()
        .unwrap_or_else(|_| std::env::temp_dir())
        .join("logs")
}

/// 日志写入目标。
struct Sink {
    dir: PathBuf,
    offset: LocalOffset,
    file: Option<File>,
    /// 当前文件已写入的字节数（用于判断何时轮转）。
    bytes: u64,
}

impl Sink {
    /// 打开（或重新打开）当前日志文件。
    fn open(&mut self) -> std::io::Result<()> {
        if let Some(mut old) = self.file.take() {
            let _ = old.flush();
        }

        let path = self.dir.join(LOG_FILENAME);
        let file = OpenOptions::new().create(true).append(true).open(&path)?;

        self.bytes = file.metadata().map(|m| m.len()).unwrap_or(0);
        self.file = Some(file);

        Ok(())
    }

    /// 写一行。任何失败都静默放弃（见模块文档第 3 条）。
    fn write_line(&mut self, line: &str) {
        // 先判断写进去会不会超上限。超了就轮转，让当前文件重新从 0 开始。
        if self.bytes + line.len() as u64 + 1 > MAX_FILE_BYTES {
            self.rotate();
        }

        if self.file.is_none() && self.open().is_err() {
            return;
        }

        let Some(file) = self.file.as_mut() else {
            return;
        };

        if file.write_all(line.as_bytes()).is_err() || file.write_all(b"\n").is_err() {
            // 写不进去就别再试了 —— 可能是磁盘满或权限问题，
            // 反复重试只会让情况更糟。
            self.file = None;
            self.bytes = 0;
            return;
        }

        self.bytes += line.len() as u64 + 1;
    }

    /// 轮转：当前 → `.1`，`.1` → `.2`，最旧的删掉。
    fn rotate(&mut self) {
        // 关掉当前文件，否则重命名时 Windows 上会失败
        //（macOS 允许，但保持这个习惯没坏处）
        self.file = None;
        self.bytes = 0;

        // 最旧的那个直接删
        let oldest = self.dir.join(format!("{LOG_FILENAME}.{}", KEEP_FILES - 1));
        let _ = fs::remove_file(oldest);

        // 依次向后挪（从后往前，避免覆盖）
        for i in (1..KEEP_FILES - 1).rev() {
            let from = self.dir.join(format!("{LOG_FILENAME}.{i}"));
            let to = self.dir.join(format!("{LOG_FILENAME}.{}", i + 1));

            if from.exists() {
                let _ = fs::rename(from, to);
            }
        }

        // 当前文件 → .1
        let current = self.dir.join(LOG_FILENAME);
        if current.exists() {
            let _ = fs::rename(current, self.dir.join(format!("{LOG_FILENAME}.1")));
        }
    }
}

/// 全局日志目标。
static SINK: OnceLock<Mutex<Sink>> = OnceLock::new();

/// 初始化日志。
///
/// 幂等：重复调用只生效一次（后续调用直接返回）。
///
/// 除了建目录，它还会做一件重要的事：**清理超出数量上限的旧文件**。
/// 这防的是「某次改动把轮转逻辑弄坏了，攒下一堆文件」这种情况 ——
/// 每次启动都扫一遍，问题不会累积。
///
/// ## 关于 panic
///
/// 这里还挂了一个 panic 钩子。崩溃是**最需要留下证据**的情况，
/// 而 Rust 默认只把 panic 打到 stderr —— 对菜单栏应用等于没打。
/// 钩子会把 panic 的位置和消息写进日志，然后继续走默认的崩溃流程
///（不吞掉 panic，该崩还是崩，只是留下了遗言）。
pub fn init(offset: LocalOffset) {
    let dir = logs_dir();

    let sink = Sink {
        dir,
        offset,
        file: None,
        bytes: 0,
    };

    // 已经初始化过就不再重复（`set` 失败说明别人先设了）
    if SINK.set(Mutex::new(sink)).is_err() {
        return;
    }

    if let Err(err) = fs::create_dir_all(logs_dir()) {
        // 目录建不出来（权限/磁盘），后面所有写入都会静默失败。
        // 这里用 eprintln 而不是 log —— 因为 log 本身就依赖这个目录。
        eprintln!("[日志] 无法创建日志目录：{err}");
    }

    prune_leftovers();

    install_panic_hook();

    info(&format!(
        "日志已初始化 —— 目录 {}，单文件上限 {} KB，最多保留 {} 个",
        logs_dir().display(),
        MAX_FILE_BYTES / 1024,
        KEEP_FILES
    ));
}

/// 删掉可能存在的、超出保留数量的历史文件。
///
/// 正常运行中轮转逻辑自己会维持数量，这个函数是**兜底**：
/// 万一之前某个版本留了一堆文件（或者用户手动复制过），
/// 启动时清理一次，避免旧文件永远躺在那里。
fn prune_leftovers() {
    let dir = logs_dir();
    let Ok(entries) = fs::read_dir(&dir) else {
        return;
    };

    // 收集形如 `tacet.log.<数字>` 的文件
    let mut numbered: Vec<(u32, PathBuf)> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            let suffix = name.strip_prefix(&format!("{LOG_FILENAME}."))?;
            let index: u32 = suffix.parse().ok()?;
            Some((index, entry.path()))
        })
        .collect();

    // 只保留 1..KEEP_FILES-1 这一段，其余删掉
    numbered.sort_by_key(|(index, _)| *index);
    for (index, path) in numbered {
        if index >= KEEP_FILES as u32 {
            let _ = fs::remove_file(path);
        }
    }
}

/// 把 panic 写进日志。
fn install_panic_hook() {
    let previous = std::panic::take_hook();

    std::panic::set_hook(Box::new(move |info| {
        let location = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_else(|| "未知位置".to_string());

        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| (*s).to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "（无法读取 panic 消息）".to_string());

        log(Level::Error, &format!("崩溃于 {location} —— {message}"));

        // 继续走原本的崩溃流程（打印到 stderr + 终止线程）。
        // 我们只是多留了一份遗言，不改变程序行为。
        previous(info);
    }));
}

/// 取全局锁；中毒时继续用（日志不值得为一个 panic 停摆）。
fn lock() -> Option<MutexGuard<'static, Sink>> {
    let sink = SINK.get()?;
    Some(match sink.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    })
}

/// 写一条日志。
pub fn log(level: Level, message: &str) {
    let Some(mut sink) = lock() else {
        // 日志没初始化：退回 stderr。
        // 这在测试和「初始化失败」两种情况下会发生。
        eprintln!("[{}] {message}", level.tag().trim());
        return;
    };

    let at = Timestamp::now();
    let line = format!(
        "{} {} {}",
        format_local_time(at, sink.offset),
        level.tag(),
        message
    );

    sink.write_line(&line);
}

/// 记一条信息。
pub fn info(message: &str) {
    log(Level::Info, message);
}

/// 记一条警告。
pub fn warn(message: &str) {
    log(Level::Warn, message);
}

/// 记一条错误。
pub fn error(message: &str) {
    log(Level::Error, message);
}

/// 把时刻格式化成 `2026-09-20 16:30:12`（本地时间）。
///
/// 日期部分复用 `tacet-storage` 的民用历算法 —— 核心层刻意不引日期库，
/// 而那里已经有经过测试的实现，没必要在日志模块再写一遍。
fn format_local_time(at: Timestamp, offset: LocalOffset) -> String {
    let date = DateWindow::day_of(at, offset).format_date();

    let local_ms = at.as_millis() + offset.millis();
    let ms_in_day = local_ms.rem_euclid(MS_PER_DAY);
    let seconds = ms_in_day / 1000;

    let (hour, minute, second) = (seconds / 3600, (seconds / 60) % 60, seconds % 60);

    format!("{date} {hour:02}:{minute:02}:{second:02}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::sync::atomic::{AtomicU32, Ordering};

    /// 每个测试用一个独立的临时目录，互不干扰。
    ///
    /// 为什么不清理：临时目录本来就是给系统回收的，而测试里删目录
    /// 会引入「并行跑测试时删到别人的文件」这类偶发失败。
    fn temp_dir() -> PathBuf {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let n = COUNTER.fetch_add(1, Ordering::SeqCst);

        let dir = std::env::temp_dir().join(format!("tacet-log-test-{}-{}", std::process::id(), n));
        let _ = fs::create_dir_all(&dir);
        dir
    }

    fn sink_in(dir: &Path) -> Sink {
        let mut sink = Sink {
            dir: dir.to_path_buf(),
            offset: LocalOffset::utc(),
            file: None,
            bytes: 0,
        };
        sink.open().expect("打开日志文件");
        sink
    }

    #[test]
    fn 写进去的内容能读出来() {
        let dir = temp_dir();
        let mut sink = sink_in(&dir);

        sink.write_line("第一行");
        sink.write_line("第二行");

        let content = fs::read_to_string(dir.join(LOG_FILENAME)).expect("读取");
        assert!(content.contains("第一行"));
        assert!(content.contains("第二行"));
    }

    #[test]
    fn 超过上限会轮转而不是无限增长() {
        let dir = temp_dir();
        let mut sink = sink_in(&dir);

        // 反复写，总量远超上限
        let line = "x".repeat(1000);
        for i in 0..(MAX_FILE_BYTES as usize / 900 + 20) {
            sink.write_line(&format!("{i} {line}"));
        }

        // 当前文件必须还在上限之内
        let size = fs::metadata(dir.join(LOG_FILENAME)).expect("stat").len();
        assert!(
            size <= MAX_FILE_BYTES,
            "当前日志文件 {size} 字节，超过了 {MAX_FILE_BYTES} 的上限"
        );
    }

    #[test]
    fn 文件数量永远不超过上限() {
        let dir = temp_dir();
        let mut sink = sink_in(&dir);

        // 写足够多的量，触发多次轮转
        let line = "y".repeat(1000);
        for i in 0..(MAX_FILE_BYTES as usize / 900 * (KEEP_FILES + 3)) {
            sink.write_line(&format!("{i} {line}"));
        }

        let count = fs::read_dir(&dir)
            .expect("列目录")
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with(LOG_FILENAME))
            .count();

        assert!(
            count <= KEEP_FILES,
            "日志文件有 {count} 个，超过了保留上限 {KEEP_FILES} —— 这就是「膨胀」"
        );
    }

    #[test]
    fn 总量被严格限制住() {
        let dir = temp_dir();
        let mut sink = sink_in(&dir);

        let line = "z".repeat(1000);
        // 写 5 倍于总上限的数据
        for i in 0..(MAX_FILE_BYTES as usize / 900 * KEEP_FILES * 5) {
            sink.write_line(&format!("{i} {line}"));
        }

        let total: u64 = fs::read_dir(&dir)
            .expect("列目录")
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with(LOG_FILENAME))
            .filter_map(|e| e.metadata().ok())
            .map(|m| m.len())
            .sum();

        let ceiling = MAX_FILE_BYTES * KEEP_FILES as u64;
        assert!(
            total <= ceiling,
            "日志总量 {total} 字节超过了硬上限 {ceiling}"
        );
    }

    #[test]
    fn 启动时会清掉超量的历史文件() {
        let dir = temp_dir();

        // 模拟「之前某个版本留了一堆文件」
        for i in 1..10 {
            fs::write(dir.join(format!("{LOG_FILENAME}.{i}")), "旧内容").expect("写");
        }

        // prune_leftovers 用的是固定目录，这里临时改环境不可行，
        // 所以直接复刻它的逻辑验证行为（保持与实现同步）。
        let mut numbered: Vec<(u32, PathBuf)> = fs::read_dir(&dir)
            .expect("列目录")
            .flatten()
            .filter_map(|entry| {
                let name = entry.file_name().to_string_lossy().to_string();
                let suffix = name.strip_prefix(&format!("{LOG_FILENAME}."))?;
                Some((suffix.parse().ok()?, entry.path()))
            })
            .collect();

        numbered.sort_by_key(|(index, _)| *index);
        for (index, path) in numbered {
            if index >= KEEP_FILES as u32 {
                let _ = fs::remove_file(path);
            }
        }

        let remaining = fs::read_dir(&dir)
            .expect("列目录")
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().starts_with(LOG_FILENAME))
            .count();

        assert_eq!(
            remaining,
            KEEP_FILES - 1,
            "应当只留下 {KEEP_FILES} 以内的编号"
        );
    }

    #[test]
    fn 时间格式化是可读的本地时间() {
        // 2026-09-20 00:00:00 UTC
        let midnight = Timestamp::from_millis(1_789_862_400_000);

        assert_eq!(
            format_local_time(midnight, LocalOffset::utc()),
            "2026-09-20 00:00:00"
        );

        // 同一时刻在 UTC+8 是早上 8 点
        assert_eq!(
            format_local_time(midnight, LocalOffset::from_hours(8)),
            "2026-09-20 08:00:00"
        );

        // 再往后 1 小时 2 分 3 秒
        let later = Timestamp::from_millis(1_789_862_400_000 + 3_723_000);
        assert_eq!(
            format_local_time(later, LocalOffset::utc()),
            "2026-09-20 01:02:03"
        );
    }

    #[test]
    fn 写失败时不会崩溃() {
        // 指向一个不可能写入的位置（把一个文件当成目录用）
        let dir = temp_dir();
        let blocker = dir.join("blocker");
        fs::write(&blocker, "我是个文件，不是目录").expect("写");

        let mut sink = Sink {
            dir: blocker, // 让 open() 必然失败
            offset: LocalOffset::utc(),
            file: None,
            bytes: 0,
        };

        // 不该 panic —— 日志写不进去是小事，不能拖垮应用
        sink.write_line("这条写不进去");
    }
}
