//! 整屏提醒的渐显 / 渐隐 —— 「到点之后，画面慢慢模糊」。
//!
//! ## 为什么要在原生窗口层做，而不是 CSS
//!
//! 休息窗口的模糊是 macOS 原生毛玻璃（`NSVisualEffectView`，见
//! `windows::blur_effects`），它画在网页**下面**。CSS 能动的只有网页自己：
//! 给调色层做 opacity 渐入，模糊本身仍然是「啪」地一下整块出现 ——
//! 用户看到的是桌面瞬间糊掉，然后白雾才慢慢浮上来。
//!
//! 能让模糊本身「慢慢来」的只有窗口的 `alphaValue`：整块窗口（含毛玻璃）
//! 从 0 淡到 1，视觉上就是清晰的桌面一点点失焦。系统自己的很多过渡
//! 也是这么做的。
//!
//! ## 为什么用一条小线程逐帧设值，而不是 Core Animation
//!
//! `NSAnimationContext` 需要 block2 + QuartzCore 两套绑定，还得处理
//! 「动画进行中被新的显示/隐藏打断」—— 那要在完成回调里判断状态，
//! 回调又跑在主线程上，和 Tauri 的消息队列交错，很难推理。
//!
//! 逐帧设值的代价是每秒 60 条主线程消息（每条只是改一个浮点数），
//! 换来的是：缓动曲线是纯函数、可测；打断靠一个代数计数器，
//! 读起来一目了然。几秒长的淡入里，帧间差不到 2% 透明度，肉眼看不出台阶。
//!
//! ## 并发约定
//!
//! 每个窗口一条「代数」。每开始一次新的渐变就 +1，旧的那条线程
//! 下一帧发现代数变了就自己退出 —— 所以「刚要淡出就来了新提醒」
//! 这类交错不会互相打架，最后一次调用说了算。

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant};

use tauri::WebviewWindow;

/// 提醒出现时的淡入时长。
///
/// 2.2 秒：够长到让人**感觉到**画面在慢慢失焦（而不是一闪），
/// 又不至于让提醒显得拖沓 —— 标题在淡入过半时就已经可读了。
pub const FADE_IN: Duration = Duration::from_millis(2200);

/// 副屏幕布比主屏晚一点开始，让「主屏先暗下来、副屏跟上」有层次。
pub const VEIL_DELAY: Duration = Duration::from_millis(180);

/// 收起时的淡出时长：退场要干脆，只留一点余韵。
pub const FADE_OUT: Duration = Duration::from_millis(480);

/// 系统开了「减弱动态效果」时的时长：不做慢镜头，只留一次短暂的交叉淡化
/// （Apple 的人机指南里，减弱动态效果时推荐用的正是交叉淡化）。
const REDUCED: Duration = Duration::from_millis(240);

/// 一帧的间隔（约 60 fps）。
const FRAME: Duration = Duration::from_millis(16);

/// 每个窗口的渐变状态。
#[derive(Debug, Clone, Copy)]
struct Track {
    /// 代数：每次开始新的渐变 +1，旧线程据此自行退出。
    generation: u64,
    /// 最近一次设下的透明度（打断时从这里接着走，不跳帧）。
    alpha: f64,
    /// 正在淡出（或已经淡出收起）。
    ///
    /// 淡出期间窗口在系统眼里仍是「可见」的，所以不能只靠 `is_visible`
    /// 判断「这次是不是重新出现」。这个标记只在下一次显示时清掉。
    fading_out: bool,
}

impl Default for Track {
    fn default() -> Self {
        Self {
            generation: 0,
            alpha: 1.0,
            fading_out: false,
        }
    }
}

fn tracks() -> &'static Mutex<HashMap<String, Track>> {
    static TRACKS: OnceLock<Mutex<HashMap<String, Track>>> = OnceLock::new();
    TRACKS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn lock_tracks() -> std::sync::MutexGuard<'static, HashMap<String, Track>> {
    // 毒化了也继续用：里面只有几个数字，不存在「改了一半」的危险状态。
    tracks()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// 缓动：正弦的 ease-in-out。
///
/// 起步和收尾都慢 —— 起步慢，是「画面开始有点不对」的那一下；
/// 收尾慢，是模糊「稳稳停住」而不是撞到终点。中段匀速走完大部分变化。
pub fn ease_in_out(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    (1.0 - (std::f64::consts::PI * t).cos()) / 2.0
}

/// 从 `from` 到 `to`、走了 `elapsed` 时（总长 `duration`）应当处于的透明度。
fn alpha_at(from: f64, to: f64, elapsed: Duration, duration: Duration) -> f64 {
    let t = if duration.is_zero() {
        1.0
    } else {
        elapsed.as_secs_f64() / duration.as_secs_f64()
    };
    from + (to - from) * ease_in_out(t)
}

/// 系统是否开了「减弱动态效果」。
#[cfg(target_os = "macos")]
fn reduce_motion() -> bool {
    objc2_app_kit::NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion()
}

#[cfg(not(target_os = "macos"))]
fn reduce_motion() -> bool {
    false
}

/// 按系统设置缩短时长。
fn effective(duration: Duration) -> Duration {
    if reduce_motion() {
        duration.min(REDUCED)
    } else {
        duration
    }
}

/// 设置窗口透明度（派发到主线程执行；`NSWindow` 只能在主线程上动）。
///
/// 派发与 `window.show()` / `hide()` 走同一条事件循环队列，先后顺序有保证：
/// 先派发「透明度 0」再 `show()`，窗口第一帧就是透明的，不会闪一下全亮。
#[cfg(target_os = "macos")]
fn apply_alpha(window: &WebviewWindow, alpha: f64) {
    let target = window.clone();
    let alpha = alpha.clamp(0.0, 1.0);
    let _ = window.run_on_main_thread(move || {
        // 每次都现取指针，而不是缓存：幕布窗口可能因为换显示器被销毁，
        // 缓存下来的指针会悬空。取不到就说明窗口已经没了，什么都不做。
        let Ok(ptr) = target.ns_window() else {
            return;
        };
        if ptr.is_null() {
            return;
        }
        // SAFETY：指针来自 Tauri 对当前存活窗口的查询，且此刻在主线程上。
        let ns_window: &objc2_app_kit::NSWindow = unsafe { &*(ptr as *const _) };
        ns_window.setAlphaValue(alpha);
    });
}

#[cfg(not(target_os = "macos"))]
fn apply_alpha(_window: &WebviewWindow, _alpha: f64) {}

/// 渐变走完（且未被打断）之后要做的事。
type OnDone = Box<dyn FnOnce(&WebviewWindow) + Send>;

/// 起一条线程，把透明度从 `from` 渐变到 `to`；走完且未被打断时执行 `on_done`。
fn spawn_ramp(
    window: WebviewWindow,
    generation: u64,
    from: f64,
    to: f64,
    delay: Duration,
    duration: Duration,
    on_done: Option<OnDone>,
) {
    let label = window.label().to_string();
    thread::spawn(move || {
        if !delay.is_zero() {
            thread::sleep(delay);
        }
        let started = Instant::now();
        loop {
            let alpha = {
                let mut map = lock_tracks();
                let Some(track) = map.get_mut(&label) else {
                    return;
                };
                if track.generation != generation {
                    // 被更新的一次渐变接手了：这条线程到此为止。
                    return;
                }
                let alpha = alpha_at(from, to, started.elapsed(), duration);
                track.alpha = alpha;
                alpha
            };
            apply_alpha(&window, alpha);
            if started.elapsed() >= duration {
                break;
            }
            thread::sleep(FRAME);
        }

        if let Some(done) = on_done {
            // 在锁里核对代数再收尾：核对与收尾之间不能插进一次新的显示，
            // 否则会把刚弹出来的提醒又藏回去。
            let map = lock_tracks();
            if map.get(&label).map(|t| t.generation) == Some(generation) {
                done(&window);
            }
        }
    });
}

/// 显示窗口，从当前透明度淡入到完全不透明。
///
/// 返回这次是否算「重新出现」：原本是隐藏的，或者正在淡出。
/// 已经完整显示着的窗口（比如休息流程中途再次调用）什么都不改，
/// 只照常 `show()`，返回 `false`。
pub fn show_with_fade(
    window: &WebviewWindow,
    delay: Duration,
    duration: Duration,
) -> tauri::Result<bool> {
    let visible = window.is_visible().unwrap_or(false);
    let label = window.label().to_string();

    let plan = {
        let mut map = lock_tracks();
        let track = map.entry(label).or_default();
        let reappearing = !visible || track.fading_out;
        if reappearing {
            track.generation += 1;
            track.fading_out = false;
            // 隐藏的窗口从 0 起；淡出到一半被叫回来的，从当前值接着亮。
            let from = if visible { track.alpha } else { 0.0 };
            track.alpha = from;
            Some((track.generation, from))
        } else {
            None
        }
    };

    if let Some((_, from)) = plan {
        apply_alpha(window, from);
        // 淡出时关掉了鼠标响应（防止退场途中被误点），重新出现时要恢复。
        let _ = window.set_ignore_cursor_events(false);
    }

    window.show()?;

    if let Some((generation, from)) = plan {
        spawn_ramp(
            window.clone(),
            generation,
            from,
            1.0,
            delay,
            effective(duration),
            None,
        );
    }

    Ok(plan.is_some())
}

/// 窗口是否正在退场（淡出中，系统仍报告它可见）。
///
/// 调用方用它把「退场中」当成「已经收起」看待 —— 比如 tick 里的幕布补铺：
/// 主窗口淡出的那半秒里它仍是可见的，不认这一条的话，
/// 正在淡出的幕布会被当成漏盖的屏幕重新淡入。
pub fn is_fading_out(window: &WebviewWindow) -> bool {
    lock_tracks()
        .get(window.label())
        .is_some_and(|track| track.fading_out)
}

/// 淡出后隐藏窗口。已经隐藏的窗口直接返回。
///
/// 淡出期间窗口不接收鼠标：用户点完「继续」后手快又点了一下，
/// 不该落到一个正在退场的界面上。
pub fn hide_with_fade(window: &WebviewWindow) {
    if !window.is_visible().unwrap_or(false) {
        return;
    }
    let label = window.label().to_string();

    let (generation, from) = {
        let mut map = lock_tracks();
        let track = map.entry(label).or_default();
        if track.fading_out {
            // 已经在退场了，不用再开一条。
            return;
        }
        track.generation += 1;
        track.fading_out = true;
        (track.generation, track.alpha)
    };

    let _ = window.set_ignore_cursor_events(true);

    spawn_ramp(
        window.clone(),
        generation,
        from,
        0.0,
        Duration::ZERO,
        effective(FADE_OUT),
        Some(Box::new(|window: &WebviewWindow| {
            let _ = window.hide();
            // 收起后把透明度还原：万一将来有哪条路径绕过 show_with_fade
            // 直接 show()，窗口也不会是一块看不见的「隐形墙」。
            apply_alpha(window, 1.0);
            let _ = window.set_ignore_cursor_events(false);
        })),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 缓动两端固定且单调() {
        assert_eq!(ease_in_out(0.0), 0.0);
        assert!((ease_in_out(1.0) - 1.0).abs() < 1e-12);
        assert!((ease_in_out(0.5) - 0.5).abs() < 1e-12);

        let mut last = 0.0;
        for step in 1..=100 {
            let value = ease_in_out(step as f64 / 100.0);
            assert!(value >= last, "缓动必须单调，否则画面会忽明忽暗");
            last = value;
        }
    }

    #[test]
    fn 缓动越界时夹在两端() {
        assert_eq!(ease_in_out(-0.3), 0.0);
        assert!((ease_in_out(1.7) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn 起步是慢的() {
        // 「慢慢模糊」的关键：头 10% 的时间里只走完很小一段，
        // 用户感觉到的是「画面开始有点不对」，而不是「一下就糊了」。
        assert!(ease_in_out(0.1) < 0.03);
    }

    #[test]
    fn 透明度按时长插值() {
        let total = Duration::from_millis(1000);
        assert_eq!(alpha_at(0.0, 1.0, Duration::ZERO, total), 0.0);
        assert!((alpha_at(0.0, 1.0, total, total) - 1.0).abs() < 1e-12);
        // 超时也停在终点，不会冲过头
        assert!((alpha_at(0.0, 1.0, total * 2, total) - 1.0).abs() < 1e-12);
        // 反向（淡出）同样成立
        assert!((alpha_at(1.0, 0.0, total, total)).abs() < 1e-12);
        // 从中途接着走：起点就是给定的 from
        assert!((alpha_at(0.4, 1.0, Duration::ZERO, total) - 0.4).abs() < 1e-12);
    }

    #[test]
    fn 零时长直接到终点() {
        assert!((alpha_at(0.0, 1.0, Duration::ZERO, Duration::ZERO) - 1.0).abs() < 1e-12);
    }
}
