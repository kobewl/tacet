//! 窗口管理 —— 把「该提醒了」翻译成屏幕上真实发生的事。
//!
//! ## 四类窗口
//!
//! | label | 用途 | 特性 |
//! | --- | --- | --- |
//! | `panel` | 菜单栏下拉主面板 | 无边框、透明、常在最前、失焦自动隐藏 |
//! | `break` | 全屏休息流程 | 覆盖全屏、毛玻璃 |
//! | `settings` | 设置页 | 常规窗口，可调整大小 |
//! | `today` | 今日记录 | 常规窗口 |
//!
//! ## 全屏 Overlay 的三条硬约束（PRD §3.3）
//!
//! 1. **永不锁屏** —— 这里只是显示一个窗口，不碰任何系统级拦截
//! 2. **必须提供 Skip** —— 界面里有「这次不用」，且始终可见
//! 3. **不得阻塞系统快捷键** —— 窗口不设置为 modal，
//!    `cmd+tab` / 输入法切换照常可用
//!
//! ## 关于「不抢焦点」
//!
//! 这一条在 macOS 上需要小心处理。全屏提醒窗口如果抢走键盘焦点，
//! 用户正在输入的内容会中断（比如代码写到一半）。
//!
//! 当前实现选择**允许**窗口获得焦点：因为休息流程的第一步就是
//! 让用户点「现在休息」，如果窗口不接受点击，整个流程就走不下去。
//! 代价是用户如果在输入长文本时被打断，可能会丢一点点正在打的内容。
//!
//! 这是一个刻意的取舍，也是 **R-01 风险**里说的「Overlay 窗口层级/焦点
//! 行为需实测」的那一项。等真实使用一段时间后再决定要不要改成
//! 非激活面板（`NSPanel` + `nonactivatingPanel`）。

use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::state::AppState;
use std::sync::{Arc, Mutex};

/// 覆盖其它屏幕的「幕布」窗口的 label 前缀。
///
/// 用前缀而不是固定名字，是因为幕布的数量取决于用户接了几块屏 ——
/// 一台笔记本可能一块，接上扩展坞就变成三块。
const VEIL_PREFIX: &str = "break-veil-";

/// 整屏提醒用的原生毛玻璃。
///
/// ## 为什么要用系统的，而不是 CSS 的 `backdrop-filter`
///
/// `backdrop-filter` 只能采样**同一个网页内部**画在它下层的东西。
/// 提醒窗口是一个独立窗口，它背后是桌面和别的应用 —— 那些像素
/// 根本不在这个网页的光栅化范围里，采样不到。早期版本试过，
/// 结果是「一点都不模糊，只剩一层白粉刷在屏幕上」（用户反馈：
/// 「我的显示器还正常啊，你这弄了个什么啊？」）。
///
/// `NSVisualEffectView` 是 macOS 在**窗口合成层**做的采样与模糊，
/// 所以它能看到窗口背后的真实内容。`fullScreenUI` 这个材质是
/// 系统为「整屏覆盖」场景准备的（就是启动台、任务控制挡屏时用的那种），
/// 和这一屏的用途正好对上。
///
/// ## 为什么 `state` 必须是 active
///
/// 窗口失去焦点时，系统的默认行为是把材质切成 inactive（变灰、几乎不透明）。
/// 而这一屏**故意不抢焦点**（幕布窗口用 `focusable(false)` 创建），
/// 于是它一显示出来就是「未激活」状态 —— 用默认值的话，
/// 用户看到的会是一层灰板，而不是透出背后内容的模糊。
fn blur_effects() -> tauri::utils::config::WindowEffectsConfig {
    tauri::utils::config::WindowEffectsConfig {
        effects: vec![tauri::utils::WindowEffect::FullScreenUI],
        state: Some(tauri::utils::WindowEffectState::Active),
        radius: None,
        color: None,
    }
}

/// 显示整屏休息提醒（询问界面 + 所有屏幕的蒙层）。
///
/// ## 为什么连其它屏幕一起蒙（这条改过一次）
///
/// 早期版本**只**盖鼠标所在的那块屏，理由写在代码里：
/// 「提醒刚弹出、用户还没做决定，此刻把副屏糊掉是绑架；先问，再做」。
///
/// 那条推理有个漏洞：它假设用户只在一块屏上工作。真实反馈推翻了它 ——
/// 用户有两块屏，提醒弹出时**另一块屏完全不受影响**，他低头继续在那边干活，
/// 提醒等于没发生。原话：
///
/// > 「这是啥东西啊，而且只有[一块]显示器有」
///
/// 「提醒」和「绑架」的分界不在「盖几块屏」，而在**盖住之后能不能立刻退出**：
///
/// - 有出口（按钮、Esc、点任意处）→ 是提醒
/// - 没有出口 → 才是绑架
///
/// 现在这套界面三个出口都在，所以盖满所有屏幕是合理的：用户要的是
/// 「整个画面慢慢模糊，然后问我一句」。
///
/// ## 蒙层与主界面是两个窗口
///
/// 主界面（`break`）只在一块屏上，因为它带着按钮 —— 四个窗口各有一份
/// 可点的按钮，用户的点击就会分叉成互相矛盾的操作（详见 `BreakVeil` 的说明）。
/// 其它屏幕只负责「挡住 + 告诉你还剩多久」，操作集中在一处。
///
/// ## 顺序：主界面先显示，蒙层后铺
///
/// 这个顺序不能反，反了会让提醒**迟到十秒**。
///
/// 主界面窗口是启动时预建的（`tauri.conf.json` 里 `visible: false` 声明），
/// `show()` 是瞬间的；而蒙层窗口是**用的时候才建**的第一块屏 ——
/// 新建一个 WebView 要拉起渲染进程、加载并执行整个前端，在慢机器上
/// 能到十秒量级。
///
/// 曾经把蒙层放在前面（理由是「让整个画面一起亮起来」），结果是
/// 那十秒里用户**什么都看不到**：主窗口还没显示，屏幕上毫无动静，
/// 而提醒已经在日志里记成「已发出」了。
///
/// 现在先让主界面出现（立刻可见、可点），蒙层随后铺上。
/// 两块屏之间差个几百毫秒完全可以接受 —— 用户先看到问题、
/// 再看到背景慢慢变糊，这个次序甚至更自然。
pub fn show_break_window(app: &AppHandle) -> tauri::Result<()> {
    let target = monitor_under_cursor(app);

    if let Some(window) = app.get_webview_window("break") {
        // 只有「从隐藏变成显示」才算一次重新打开。
        //
        // 这个判断很关键：用户在休息流程中间（比如填完待办点「开始休息」）
        // 会再次触发 `start_break`，那时窗口本来就是可见的。
        // 如果把这种情况也当成「重新打开」，前端会把阶段重置回
        // 「填写待办」—— 用户刚点完「开始休息」，界面却又问他要做什么。
        let reopening = !window.is_visible().unwrap_or(false);

        // 每次显示前重新适配当前屏幕 —— 用户可能换了显示器、
        // 或者把窗口拖到了另一块屏上。
        if let Some(monitor) = &target {
            fit_to_monitor(&window, monitor);
        }
        window.show()?;
        window.set_focus()?;

        if reopening {
            announce_break_shown(app);
        }

        // 主界面已经在了 —— 现在再铺蒙层。它慢一点没关系，
        // 用户至少已经能看到提醒本身（理由见函数文档）。
        veil_other_screens(app);

        return Ok(());
    }

    // 窗口在 tauri.conf.json 里已经声明过（label = "break"），
    // 正常情况下 `get_webview_window` 就能拿到。走到这里说明配置有出入，
    // 我们动态创建一个作为兜底 —— 提醒不能因为配置问题就发不出来。
    let window = WebviewWindowBuilder::new(
        app,
        "break",
        WebviewUrl::App("index.html?view=break".into()),
    )
    .title("休息一下")
    .inner_size(1440.0, 900.0)
    .decorations(false)
    .transparent(true)
    .always_on_top(true)
    .skip_taskbar(true)
    // 真模糊：交给 macOS 的 NSVisualEffectView 在窗口背后采样。
    // 这条兜底路径和 tauri.conf.json 里声明的必须一致 ——
    // 两边不一致的话，配置出问题时建出来的窗口就是「不模糊」的那一个。
    .effects(blur_effects())
    .build()?;

    if let Some(monitor) = &target {
        fit_to_monitor(&window, monitor);
    }
    window.show()?;
    window.set_focus()?;
    announce_break_shown(app);

    // 兜底路径同样在主界面之后铺蒙层（顺序理由见函数文档）。
    veil_other_screens(app);

    Ok(())
}

/// 把其它屏幕蒙上，失败只记日志。
///
/// 「失败降级」这个处理放在这里而不是各个调用点：蒙层是附加的遮挡层，
/// 主屏的休息界面已经正常显示了，核心功能没有丢 ——
/// 因为一个附加层让整个休息流程失败是本末倒置。
///
/// 调用它的地方有两处（主界面显示、兜底创建），两处的降级策略一致，
/// 所以合成一个入口，免得改了一处忘了另一处。
fn veil_other_screens(app: &AppHandle) {
    if let Err(err) = show_break_veils(app) {
        crate::logging::warn(&format!("铺蒙层失败（提醒照常显示）：{err}"));
    }
}

/// 告诉休息界面「你被重新打开了，把流程重置到正确的阶段」。
///
/// ## 为什么必须做这件事
///
/// 隐藏窗口**不会卸载网页**。窗口再次显示时，React 组件还停在
/// 上一次离开时的阶段 —— 如果上一次休息是正常结束的，那个阶段是
/// `done`（「欢迎回来」）。
///
/// 于是第二次提醒弹出来的时候，用户看到的是「欢迎回来」和一个
/// 「继续」按钮，而不是「建议休息一下」。整个提醒就废了：
/// 它看起来像一个没关掉的旧窗口。
///
/// ## 为什么不带参数
///
/// 事件的载荷里不写「应该进哪个阶段」，而是让前端自己去读一次最新快照。
/// 理由是**状态真值只有一份**（在 Rust 侧）：把阶段判断放在前端，
/// 就多了一处可能与真实状态不同步的地方。前端拿到 `state === "breaking"`
/// 就知道「用户已经同意休息了，该显示待办输入」，否则显示询问。
pub fn announce_break_shown(app: &AppHandle) {
    broadcast(app, serde_json::json!({ "type": "breakShown" }));
}

/// 找出用户此刻在看哪块屏幕。
///
/// ## 为什么选「鼠标所在的屏幕」
///
/// 多显示器场景下，用户此刻在看哪块屏？鼠标位置是最可靠的信号
/// （macOS 上鼠标坐标就是全局坐标）。用主屏是次优选择 ——
/// 如果用户把笔记本合上盖、只用外接显示器，主屏可能不是他在看的那块。
///
/// 取不到鼠标所在屏幕时退回主屏；再取不到就返回 `None`
/// （调用方会保持窗口原有尺寸，宁可显示得小一点，也不能因为探测失败
/// 就不显示提醒）。
///
/// ## 三个调用方
///
/// 休息窗口（决定盖哪块屏）、设置窗口与今日记录窗口（决定摆在哪）。
/// 后两者是后来加上的：窗口如果不自己摆位置，macOS 会按自己的心情放 ——
/// 实测会跑到另一块显示器上，而用户正在看的是这一块，
/// 于是「点了设置，屏幕上什么都没出现」。
fn monitor_under_cursor(app: &AppHandle) -> Option<tauri::Monitor> {
    monitor_containing(app, cursor_position(app))
}

/// 鼠标现在在哪（物理像素，左上角原点）。取不到返回 `None`。
///
/// 用 `get_webview_window("break")` 拿光标位置是历史原因：Tauri 只在
/// **窗口**上暴露 `cursor_position()`，没有全局的光标 API。这个窗口在启动时
/// 就建好了（只是隐藏着），所以正常情况下永远拿得到。
///
/// ## 精度：够用，但不精确
///
/// `cursor_position()` 内部把全局逻辑坐标一律按**主屏**的缩放比换算成物理
/// 像素（见 tao 的 `util::cursor_position`）。两块屏缩放比相同的机器上没问题；
/// 一旦不同（比如内置 Retina 2x + 一台 1080p 的 1x 外接屏），副屏上的点会算偏。
/// 所以它只适合回答「大致在哪块屏」，不适合拿来算窗口的精确落点 ——
/// 后者一律用托盘图标自己的位置（见 [`TrayAnchor`]）。
fn cursor_position(app: &AppHandle) -> Option<(i32, i32)> {
    let cursor = app.get_webview_window("break")?.cursor_position().ok()?;
    Some((cursor.x as i32, cursor.y as i32))
}

/// 点 (x, y) 落在哪块屏上？返回它在 `screens` 里的下标。
///
/// 矩形按**左闭右开**处理：右边界和下边界上的点归相邻那块屏。这样两块屏
/// 紧挨着摆时既不会留缝，也不会有一行像素同时命中两块屏。
fn screen_index_at(screens: &[ScreenBox], x: i32, y: i32) -> Option<usize> {
    screens
        .iter()
        .position(|s| x >= s.x && x < s.x + s.width as i32 && y >= s.y && y < s.y + s.height as i32)
}

/// 这个点在哪块屏上？点命中不了任何屏（或压根没给点）时退回主屏。
fn monitor_containing(app: &AppHandle, point: Option<(i32, i32)>) -> Option<tauri::Monitor> {
    let monitors = app.available_monitors().ok()?;

    if let Some((x, y)) = point {
        let screens: Vec<ScreenBox> = monitors.iter().map(ScreenBox::from).collect();
        if let Some(index) = screen_index_at(&screens, x, y) {
            return monitors.into_iter().nth(index);
        }
    }

    app.primary_monitor().ok().flatten()
}

/// 主屏在 `screens` 里的下标。认不出主屏时退回 0。
///
/// 比较「位置 + 尺寸」而不是名字：外接屏的名字可能是空串，同型号的两块屏
/// 名字还完全一样（和 [`veil_targets`] 那条注释是同一个理由）。
fn primary_index(app: &AppHandle, screens: &[ScreenBox]) -> usize {
    let Some(primary) = app.primary_monitor().ok().flatten() else {
        return 0;
    };
    let wanted = ScreenBox::from(&primary);
    screens.iter().position(|s| *s == wanted).unwrap_or(0)
}

/// 把窗口撑满指定的那块屏幕。
///
/// ## 为什么必须做这件事
///
/// 窗口尺寸在 `tauri.conf.json` 里写的是 1440×900。这个值在 13 寸
/// MacBook（正好 1440×900）上恰好铺满，看起来很对 —— 所以这个问题
/// 在开发机上不会暴露。
///
/// 但换到 27 寸显示器（逻辑 2560×1440）或 Studio Display 上，
/// 它只占屏幕的四分之一，变成一个居中的小方块。这不影响功能，
/// 但**破坏了这个界面的设计意图**：它原本要靠「铺满、无信息可看、
/// 留白极大」在心理上推用户离开屏幕。缩成小方块之后，
/// 看起来像程序出错了，而不是一个刻意的休息提醒。
///
/// ## 关于「撑满」的边界
///
/// 用 `set_size` 到屏幕的物理尺寸、`set_position` 到屏幕原点，
/// 而不是调用 `set_fullscreen(true)`。原因见文件顶部注释：
/// 进入系统原生全屏会让 macOS 为窗口单独创建一个 Space，
/// 那种情况下 Esc 退不出来、Cmd+Tab 行为也会变 —— 那才是
/// 真正会困住用户的机制。我们要的是「浮在上面的一层玻璃」，
/// 不是「霸占一个虚拟桌面」。
fn fit_to_monitor(window: &tauri::WebviewWindow, monitor: &tauri::Monitor) {
    let size = monitor.size();
    let position = monitor.position();

    let _ = window.set_position(tauri::PhysicalPosition::new(position.x, position.y));
    let _ = window.set_size(tauri::PhysicalSize::new(size.width, size.height));
}

/// 在**除当前屏以外**的每块屏幕上都盖一层幕布。
///
/// ## 为什么需要这件事
///
/// 只盖住一块屏的「全屏休息」是假的：用户直接把鼠标移到另一块屏幕
/// 就能继续干活，休息被绕过去了。产品承诺的是真的让人停下来，
/// 那就得把每一块屏都算进去。
///
/// ## 为什么是「幕布」而不是「每块屏一个完整界面」
///
/// 每块屏都渲染一套完整的倒计时+按钮+输入框，会有两个问题：
///
/// 1. **状态不同步**：四个窗口各自维护自己的阶段（询问/填写/休息中），
///    用户在副屏点了「跳过」，主屏还停在询问态 —— 界面互相打脸。
/// 2. **注意力被摊薄**：用户不知道该看哪块屏，视线在屏幕间跳来跳去。
///
/// 所以副屏只做一件最简单的事：**挡住视线，并告诉他还要多久**。
/// 所有操作集中在主屏那一处。（副屏也留了出口：点击或按 Esc
/// 都会把意图转给主屏处理，见 `BreakVeil` 组件。）
///
/// ## 失败要降级，不能连坐
///
/// 幕布建不出来（权限、系统限制）时只记一条日志 —— 主屏的休息界面
/// 已经正常显示了，核心功能没有丢。因为一个附加的遮挡层而让整个
/// 休息流程失败，是本末倒置。
pub fn show_break_veils(app: &AppHandle) -> tauri::Result<()> {
    show_break_veils_inner(app, true)
}

/// `verbose`：提醒刚弹出时打完整判断依据；tick 补铺时只在真的新盖上才记。
fn show_break_veils_inner(app: &AppHandle, verbose: bool) -> tauri::Result<()> {
    let primary = monitor_under_cursor(app);
    let monitors = app.available_monitors()?;

    if verbose {
        crate::logging::info(&format!(
            "幕布判断：检测到 {} 块屏 [{}]；鼠标判定在 {:?}",
            monitors.len(),
            monitors
                .iter()
                .map(|m| {
                    let p = m.position();
                    let s = m.size();
                    format!("({},{}) {}x{}", p.x, p.y, s.width, s.height)
                })
                .collect::<Vec<_>>()
                .join(" / "),
            primary.as_ref().map(|m| {
                let p = m.position();
                let s = m.size();
                format!("({},{}) {}x{}", p.x, p.y, s.width, s.height)
            })
        ));
    }

    // 合盖/休眠刚醒时 macOS 会报 0 块屏。主界面照常显示，tick 里再补铺。
    if monitors.is_empty() {
        if verbose {
            crate::logging::warn(
                "幕布判断：系统此刻报了 0 块屏（常见于刚唤醒）。主界面照常显示，稍后再补铺副屏。",
            );
        }
        return Ok(());
    }

    // 单屏用户（大多数）走这条路：什么都不用做。
    if monitors.len() < 2 {
        return Ok(());
    }

    // 这次需要哪几块幕布。跳过主屏（它上面是完整的休息界面）。
    //
    // 真正的决定交给下面那个纯函数：`tauri::Monitor` 没法在单测里构造，
    // 但「哪块屏该盖、哪块该跳过」恰恰是这个功能里最容易写错的部分，
    // 必须能测。
    let wanted = wanted_veils(&monitors, primary.as_ref());

    if wanted.is_empty() {
        if verbose {
            crate::logging::warn("幕布：有多块屏，但没有一块需要遮挡（屏幕被认成同一块了？）");
        }
        return Ok(());
    }

    if verbose {
        crate::logging::info(&format!(
            "幕布：{} 块屏幕需要遮挡（共检测到 {} 块屏）",
            wanted.len(),
            monitors.len()
        ));
    }

    // 收掉不再需要的幕布。用户可能换过显示器 —— 屏幕数量一变，
    // 旧的编号就指到别的屏上了，留着会盖错地方。
    for (label, window) in app.webview_windows() {
        if label.starts_with(VEIL_PREFIX) && !wanted.iter().any(|(l, _)| l == &label) {
            // 这里是真正销毁而不是隐藏：这块屏已经不存在了，
            // 留着这个窗口网页只是白占内存。
            let _ = window.close();
        }
    }

    let mut newly_shown = 0u32;
    for (label, monitor) in &wanted {
        let window = veil_window(app, label)?;
        let was_visible = window.is_visible().unwrap_or(false);
        fit_to_monitor(&window, monitor);
        window.show()?;
        if !was_visible {
            newly_shown += 1;
        }
    }

    if !verbose && newly_shown > 0 {
        crate::logging::info(&format!(
            "补铺幕布：新显示 {newly_shown} 块（当前检测到 {} 块屏）",
            monitors.len()
        ));
    }

    Ok(())
}

/// 预建副屏蒙层窗口（建好即隐藏，等着被显示）。
///
/// ## 为什么需要「预建」这件事
///
/// 蒙层窗口带着一整个 WebView，**首次创建**要拉起渲染进程、加载并执行
/// 前端 —— 实测能到十几秒。
///
/// 等到提醒弹出时才建的话，首次提醒的副屏会晚十几秒才蒙住，
/// 而用户可能早就低头在那边干上活了。更麻烦的是这个延迟**只出现在
/// 第一次提醒**上（之后复用已建好的窗口，是瞬间的）——
/// 一个「第一次慢、后面正常」的问题最难复现，也最容易被当成偶发故障。
///
/// 所以放在应用启动时建：此时用户本来就在等启动，代价可以接受，
/// 换来的是每一次提醒都及时。
///
/// ## 为什么建好之后立刻隐藏
///
/// 窗口声明成 `visible: false`，但 `build()` 之后仍要显式 hide ——
/// 不同平台上 `visible` 的语义有差异（有的平台是「不激活」而不是
/// 「不显示」），显式 hide 一次能保证它在启动瞬间绝不出现在屏幕上。
pub fn prepare_break_veils(app: &AppHandle) -> tauri::Result<()> {
    let monitors = app.available_monitors()?;
    if monitors.len() < 2 {
        // 单屏用户：根本没有副屏要盖，不用建任何窗口。
        return Ok(());
    }

    // 用和显示那条路**完全同一份**清单（见 `wanted_veils` 的说明）——
    // label 对不上就等于白建。
    let primary = monitor_under_cursor(app);
    let wanted = wanted_veils(&monitors, primary.as_ref());

    for (label, monitor) in &wanted {
        let window = veil_window(app, label)?;
        fit_to_monitor(&window, monitor);
        // 建完立刻藏起来 —— 它此刻没有任何理由出现在屏幕上。
        let _ = window.hide();
    }

    crate::logging::info(&format!(
        "预建副屏幕布：{} 块（共检测到 {} 块屏）",
        wanted.len(),
        monitors.len()
    ));

    Ok(())
}

/// 算出这次需要哪几块幕布：`(label, 对应的屏幕)`。
///
/// ## 为什么抽成共用函数
///
/// 有两条路径需要这份清单 —— 启动时预建（`prepare_break_veils`）
/// 和提醒弹出时显示（`show_break_veils`）。两边的 **label 必须完全一致**，
/// 否则预建出来的窗口没人用（显示那条路会另建一个新的，白等十几秒），
/// 而预建的那个永远挂着白占内存。
///
/// 曾经两边不一致：预建用「屏幕在列表里的下标」编号，显示用
/// 「第几个需要盖的屏」编号 —— 三块屏、主屏在中间时，
/// 一个建出 `veil-0` + `veil-2`，另一个要的是 `veil-0` + `veil-1`，
/// 正好错开。所以这份计算只能有一处。
///
/// 编号用**需要盖的位置序号**（0、1、2…）而不是屏幕下标：
/// 前者永远连续、没有空洞，也不受「哪块屏是主屏」的变化影响。
fn wanted_veils(
    monitors: &[tauri::Monitor],
    primary: Option<&tauri::Monitor>,
) -> Vec<(String, tauri::Monitor)> {
    let screens: Vec<ScreenBox> = monitors.iter().map(ScreenBox::from).collect();

    veil_labels(&screens, primary.map(ScreenBox::from))
        .into_iter()
        .map(|(label, index)| (label, monitors[index].clone()))
        .collect()
}

/// 幕布的编号 —— 纯函数，可以单测。
///
/// 返回 `(label, 屏幕下标)`。编号用「第几个需要盖的屏」（0、1、2…），
/// 不用屏幕下标：前者连续无空洞，也不受「哪块是主屏」变化的影响。
fn veil_labels(screens: &[ScreenBox], primary: Option<ScreenBox>) -> Vec<(String, usize)> {
    veil_targets(screens, primary)
        .into_iter()
        .enumerate()
        .map(|(ordinal, index)| (format!("{VEIL_PREFIX}{ordinal}"), index))
        .collect()
}

/// 取（必要时创建）一块副屏的幕布窗口。
///
/// 创建参数集中在这里，理由和 `blur_effects` 一样：这套窗口属性
/// （无边框、置顶、不抢焦点、真模糊）是**必须一致**的一组约定，
/// 分散在两处（预建、显示）迟早会漂移。
fn veil_window(app: &AppHandle, label: &str) -> tauri::Result<tauri::WebviewWindow> {
    if let Some(window) = app.get_webview_window(label) {
        // 复用上次留下的窗口：页面已经加载好了，显示出来是瞬间的。
        // 重新创建会在屏幕上闪一下白，那种「闪」在一个刻意安静的
        // 界面上非常刺眼。
        return Ok(window);
    }

    WebviewWindowBuilder::new(app, label, WebviewUrl::App("index.html?view=veil".into()))
        .title("Tacet")
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .shadow(false)
        // ── 焦点：幕布永远不抢键盘焦点 ──
        //
        // 这一条不是「更优雅」，是「必须」。
        //
        // macOS 上显示一个窗口（`set_visible(true)`）走的是
        // `makeKeyAndOrderFront` —— 它会把窗口设为 key window，
        // 也就是**抢走键盘焦点**。而幕布可能出现在用户正要输入的
        // 那一刻：焦点被抢走，输入框就废了，用户打不了字也不知道为什么。
        //
        // `focusable(false)` 让这个窗口永远不会成为 key window，
        // 从根上避免这件事。用户想用幕布上的出口（点击）也不需要焦点 ——
        // 点击会先激活窗口，而 Esc 走的是主窗口那条路。
        .focusable(false)
        .focused(false)
        .visible(false)
        // 和主界面同一套真模糊 —— 副屏看起来必须和主屏是一件事，
        // 否则用户会以为自己开了两个不同的东西。
        .effects(blur_effects())
        .build()
}

/// 一块屏的标识 —— 只有「位置 + 尺寸」。
///
/// 为什么不用 `tauri::Monitor` 直接比较：一是它没法在单测里构造，
/// 二是我们本来也只需要这两项。**不比较 `name()`**：
/// 外接屏的名字可能是空字符串，而且同型号的两块屏名字完全一样。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ScreenBox {
    x: i32,
    y: i32,
    width: u32,
    height: u32,
}

impl From<&tauri::Monitor> for ScreenBox {
    fn from(monitor: &tauri::Monitor) -> Self {
        let position = monitor.position();
        let size = monitor.size();
        Self {
            x: position.x,
            y: position.y,
            width: size.width,
            height: size.height,
        }
    }
}

/// 算出这次要给哪几块屏盖幕布（返回它们在 `screens` 里的下标）。
///
/// 规则只有一条：**跳过主屏** —— 那块屏上是完整的休息界面，
/// 再盖一层幕布会把倒计时和按钮一起糊掉。
///
/// 主屏认不出来时（`None`）返回全部屏幕。这是有意的降级：
/// 宁可多盖一块（用户还能在主屏上操作），也不要漏盖 ——
/// 漏盖意味着休息可以被绕过，那这个功能就白做了。
fn veil_targets(screens: &[ScreenBox], primary: Option<ScreenBox>) -> Vec<usize> {
    screens
        .iter()
        .enumerate()
        .filter(|(_, screen)| match primary {
            Some(p) => **screen != p,
            // 认不出主屏：全盖（理由见上）。
            None => true,
        })
        .map(|(index, _)| index)
        .collect()
}

/// 收起所有幕布窗口（只隐藏，留着下次复用）。
pub fn hide_break_veils(app: &AppHandle) {
    for (label, window) in app.webview_windows() {
        if label.starts_with(VEIL_PREFIX) {
            let _ = window.hide();
        }
    }
}

/// 兜底：主窗口不在时，幕布也不能在。
///
/// ## 为什么需要这条不变量
///
/// 幕布存在的唯一理由是「给主屏的休息界面打配合」。如果主界面已经不在了，
/// 幕布就变成了纯粹的故障：用户的副屏糊着一层白雾，主屏什么都没有，
/// 而且界面上没有任何按钮能让他恢复 —— 这比不休息糟得多。
///
/// 正常路径下两者永远成对（`hide_break_window` 一起收），但「正常路径」
/// 覆盖不了所有情况：webview 崩了、前端某个分支忘了调关闭命令、
/// 将来有人加了新的关闭方式却没读这段注释。这类问题的代价太高，
/// 值得用一条每秒都在执行的不变量兜住。
///
/// ## 为什么不在状态变化时检查，而是每次 tick
///
/// tick 是应用里唯一持续推进的节拍（10 秒一次），所有窗口状态的最终一致
/// 都可以搭它的车。相比在每个可能出错的地方都记得调用一次，
/// 「每次 tick 都核对一遍事实」是更省心也更难写错的做法。
///
/// 这条检查很便宜：就是读两个窗口的可见性，10 秒一次。
pub fn reconcile_break_windows(app: &AppHandle) {
    let Some(main) = app.get_webview_window("break") else {
        // 主窗口还没建出来（应用刚启动）：那就不该有任何幕布。
        hide_break_veils(app);
        return;
    };

    // 主窗口不可见 → 幕布也不该可见。
    //
    // 注意这里判的是**可见性**而不是工作状态：询问阶段（用户还没点
    // 「现在休息」）主窗口是可见的，而工作状态仍是 Working ——
    // 拿状态当依据会把询问界面一起收掉，那是个 bug。
    if !main.is_visible().unwrap_or(false) {
        hide_break_veils(app);
        return;
    }

    // 主窗口还在：显示器可能刚从休眠里回来（当时 available_monitors
    // 是空的，副屏没盖上）。每 10 秒对一次，漏盖的补上。
    if let Err(err) = show_break_veils_inner(app, false) {
        crate::logging::warn(&format!("补铺蒙层失败：{err}"));
    }
}

/// 关闭全屏休息窗口（连同所有幕布）。
///
/// 这两件事永远成对发生，所以合成一个入口 —— 只关主窗口会让副屏
/// 继续盖着白雾，用户看到的是「休息结束了但屏幕还是坏的」。
pub fn hide_break_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("break") {
        let _ = window.hide();
    }
    hide_break_veils(app);
}

/// 面板与托盘图标之间留的空隙（逻辑像素）。
///
/// 不贴死是因为面板自带 20pt 圆角与阴影 —— 贴死会让阴影糊在菜单栏上。
const PANEL_GAP: f64 = 6.0;

/// 面板与屏幕左右边缘的最小距离（逻辑像素）。
const PANEL_MARGIN: f64 = 16.0;

/// 拿不到图标位置时，面板顶边距屏幕顶边的距离（逻辑像素）。
///
/// 这是**估计值**不是测量值：菜单栏高度因屏而异（本机实测内置屏 33pt、
/// 外接屏 30pt），而 Tauri 的 `Monitor` 不暴露「可见区域」，问不到真值。
/// 只有单实例唤醒、Dock 点击这两条拿不到图标矩形的路径用得到它。
const PANEL_TOP_FALLBACK: f64 = 33.0;

/// 布局所需的几处间距（物理像素，调用方已按屏幕缩放比换算过）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Insets {
    /// 面板顶边与托盘图标下沿之间的空隙。
    gap: i32,
    /// 面板与屏幕左右边缘的最小距离。
    margin: i32,
    /// 拿不到图标时，面板顶边距屏幕顶边的距离。
    fallback_top: i32,
}

/// 托盘图标在屏幕上的矩形（物理像素，左上角原点）。
///
/// ## 为什么不直接用 `tauri::Rect`
///
/// 那个类型的坐标可能是物理的也可能是逻辑的（看平台实现），两个变体要分别
/// 处理；而且它没法在单测里构造。在入口处一次性换算成物理像素，后面就只剩
/// 纯算术 —— 也就都能测。
///
/// ## 坐标系是通用的
///
/// 它和 `Monitor::position()` / `size()` 在同一个坐标系里：全局物理像素、
/// 左上角原点，副屏的原点可能是负数（本机外接屏的 y 是 -370）。所以
/// 「这枚图标在哪块屏上」可以直接拿图标中心去和屏幕矩形做命中判断，
/// 中间不需要任何换算。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TrayAnchor {
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}

impl TrayAnchor {
    /// 图标中心的横坐标 —— 面板要跟它水平对齐。
    fn center_x(&self) -> i32 {
        self.x + self.width / 2
    }

    /// 图标中心的纵坐标 —— 用来判断它在哪块屏上。
    fn center_y(&self) -> i32 {
        self.y + self.height / 2
    }

    /// 图标下沿 —— 面板的顶边从这里往下让开一点。
    fn bottom(&self) -> i32 {
        self.y + self.height
    }
}

impl From<&tauri::Rect> for TrayAnchor {
    fn from(rect: &tauri::Rect) -> Self {
        // macOS 给的是物理像素（tray-icon 内部已经 `to_physical` 过）。
        // 逻辑坐标那个分支是给别的平台兜底的，四舍五入即可 ——
        // 差半个像素在这个用途上看不出来。
        let (x, y) = match rect.position {
            tauri::Position::Physical(p) => (p.x, p.y),
            tauri::Position::Logical(p) => (p.x.round() as i32, p.y.round() as i32),
        };
        let (width, height) = match rect.size {
            tauri::Size::Physical(s) => (s.width as i32, s.height as i32),
            tauri::Size::Logical(s) => (s.width.round() as i32, s.height.round() as i32),
        };

        Self {
            x,
            y,
            width,
            height,
        }
    }
}

/// 显示（或聚焦）菜单栏主面板。
///
/// 这条路径拿不到托盘图标的矩形（单实例唤醒、Dock 点击、托盘菜单里的
/// 「打开面板」），于是按「鼠标在哪块屏」选屏，面板落在**那块屏**的右上角。
///
/// 从左键点击托盘图标进来的路径请用 [`toggle_panel_under_icon`] ——
/// 只有它能精确贴到**用户点的那一枚**图标下面。
pub fn toggle_panel(app: &AppHandle) -> tauri::Result<()> {
    toggle_panel_at(app, None)
}

/// 显示（或聚焦）菜单栏主面板，并把它贴到**刚被点的那枚托盘图标**下面。
///
/// `icon` 来自 `TrayIconEvent::Click` 的 `rect`。这件事必须由事件带进来，
/// 应用自己猜不出来：只要系统开着「显示器各自有独立的桌面」（默认开启），
/// **每块屏的菜单栏上都会有一枚 Tacet 图标**（本机实测：内置屏一枚、
/// 外接屏一枚），点哪一枚就该在哪一块屏上弹面板。
///
/// ## 为什么不用 `TrayIcon::rect()` 现问一次
///
/// 因为点击事件里的 `rect` 比它更准，理由在两边各自的实现里：
///
/// - 事件里的 `rect` 由 tray-icon 的 `send_mouse_event` 算出，读的是
///   **事件真正发生的那扇窗口**（`NSEvent.window`）—— 用户点的是哪一枚，
///   它就是哪一枚。
/// - `TrayIcon::rect()` 读的是 `NSStatusItem.button().window`，也就是
///   **按钮所属的那一个窗口**。多显示器下它只有单一来源，代表不了
///   「用户刚点的那一枚」。
///
/// 既然点击发生时事件已经把准确答案递到手上，就没有理由再去问一次
/// 那个语义更弱的接口。
pub fn toggle_panel_under_icon(app: &AppHandle, icon: &tauri::Rect) -> tauri::Result<()> {
    toggle_panel_at(app, Some(TrayAnchor::from(icon)))
}

fn toggle_panel_at(app: &AppHandle, anchor: Option<TrayAnchor>) -> tauri::Result<()> {
    let Some(window) = app.get_webview_window("panel") else {
        return Ok(());
    };

    if window.is_visible().unwrap_or(false) {
        window.hide()?;
        return Ok(());
    }

    position_panel(app, &window, anchor);
    window.show()?;
    window.set_focus()?;
    Ok(())
}

/// 把面板窗口摆到该去的地方。
fn position_panel(app: &AppHandle, window: &tauri::WebviewWindow, anchor: Option<TrayAnchor>) {
    let Ok(monitors) = app.available_monitors() else {
        return;
    };
    let Ok(window_size) = window.outer_size() else {
        return;
    };

    let screens: Vec<ScreenBox> = monitors.iter().map(ScreenBox::from).collect();

    // 选屏的优先级：用户刚点的托盘图标 > 鼠标 > 主屏。
    //
    // 图标排第一是关键：鼠标可能已经移开了（点完菜单栏图标顺手把手挪回主屏是
    // 常事），而「刚才点的是哪一枚图标」是个明确的事实，不会骗人。
    let index = anchor
        .map(|icon| (icon.center_x(), icon.center_y()))
        .or_else(|| cursor_position(app))
        .and_then(|(x, y)| screen_index_at(&screens, x, y))
        .unwrap_or_else(|| primary_index(app, &screens));

    let Some(screen) = screens.get(index) else {
        return;
    };

    let scale = monitors[index].scale_factor();
    let insets = Insets {
        gap: (PANEL_GAP * scale).round() as i32,
        margin: (PANEL_MARGIN * scale).round() as i32,
        fallback_top: (PANEL_TOP_FALLBACK * scale).round() as i32,
    };

    let (x, y) = panel_origin(screen, window_size.width as i32, anchor, insets);
    let _ = window.set_position(tauri::PhysicalPosition::new(x, y));
}

/// 面板左上角该放哪（物理像素）？纯函数，便于测试。
///
/// ## 有图标位置：贴着图标，像原生菜单那样
///
/// 水平方向与图标中心对齐，竖直方向从图标下沿再往下让开 `gap`。
/// 这是 macOS 菜单栏下拉面板的既定行为，用户不需要学。
///
/// 竖直方向**不要**去减「菜单栏高度」：菜单栏高度因屏而异（本机实测内置屏
/// 33pt、外接屏 30pt），写死任何一个值都会在另一块屏上差几个像素。
/// 图标的矩形本身已经包含了这个高度 —— 它的下沿就是菜单栏的下沿。
///
/// ## 没有图标位置：屏幕右上角
///
/// 退化成贴着这块屏的右上角。水平位置可能与真正的图标不一致，
/// 但至少出现在**用户正在看的那块屏**上 —— 这已经比「永远弹在主屏」好得多。
///
/// ## 为什么要夹紧
///
/// 图标常常就贴在屏幕最右边，此时「与图标居中对齐」算出来的面板会伸到屏幕外
/// （本机实测：内置屏的菜单栏图标中心在 1059pt，而屏宽 1728pt，算出来正好
/// 越界）。所以最后要把 x 夹回屏幕内。
///
/// 竖直方向不夹：面板高度固定 520pt，比任何一块现代屏幕都矮，
/// 强行夹反而会在极端情况下把它挪到奇怪的地方。
fn panel_origin(
    screen: &ScreenBox,
    window_width: i32,
    icon: Option<TrayAnchor>,
    insets: Insets,
) -> (i32, i32) {
    let right = screen.x as i64 + screen.width as i64;

    let y = match icon {
        Some(icon) => icon.bottom() as i64 + insets.gap as i64,
        None => screen.y as i64 + insets.fallback_top as i64,
    };

    let wanted = match icon {
        Some(icon) => icon.center_x() as i64 - window_width as i64 / 2,
        None => right - window_width as i64 - insets.margin as i64,
    };

    // 屏幕比窗口还窄时 max 会小于 min，而 `clamp` 在那种情况下会 panic ——
    // 先兜住：让窗口左边缘贴齐屏幕左边缘，右半边露在屏幕外，
    // 但至少左边是看得见的（比整个窗口消失强）。
    let min_x = screen.x as i64 + insets.margin as i64;
    let max_x = (right - window_width as i64 - insets.margin as i64).max(min_x);

    (wanted.clamp(min_x, max_x) as i32, y as i32)
}

/// 把一扇「常规窗口」呈现到用户面前 —— 设置页、今日记录都走这里。
///
/// ## 三步，每一步都对应一个真实踩过的坑
///
/// **一、先还原。** 窗口可能被用户最小化过（黄按钮 / `⌘M`）。
/// `tao` 的 `set_focus()` 在窗口处于最小化状态时会**直接跳过**
/// （见 `tao/src/platform_impl/macos/window.rs`：`if !is_minimized && is_visible`），
/// 而它内部用的 `makeKeyAndOrderFront` 也不会把窗口从 Dock 里拉回来。
/// 于是「点设置」的表现就是**什么都没有发生** —— 窗口确实还在，
/// 只是永远回不到屏幕上。这一条是「点了没反应」最隐蔽的来源。
///
/// **二、自己摆位置。** 不摆的话位置由 macOS 决定。实测在双屏机器上
/// 它会跑到**另一块显示器**上（用户正看着的那块屏幕什么都不出现），
/// 而且窗口还会被系统按目标屏的缩放比重算尺寸。所以这里显式地
/// 摆到「鼠标所在那块屏」的正中间 —— 用户正在看哪块屏，窗口就出现在哪块屏。
///
/// 代价是用户拖动过窗口位置后，下次打开会回到居中。这个取舍是有意的：
/// 一个「总是在你看的那块屏中央出现」的设置窗口，比一个「记得上次位置、
/// 但可能出现在你看不到的地方」的设置窗口可靠得多。后者正是这次要修的 bug。
///
/// **三、显示 + 取焦，顺序不能反。** `show()` 只负责让它可见；
/// 把应用激活到前台、把键盘焦点抢过来的是 `set_focus()`
/// （`tao` 的 `util::set_focus` 会在 `makeKeyAndOrderFront` 之后调
/// `activateIgnoringOtherApps`）。少了后者，窗口可能可见但不是 key window，
/// 用户会觉得界面「点不动」。
///
/// 失败一律忽略：这三步里任何一步在某些系统配置下都可能不被允许，
/// 但把窗口留在屏幕上总比让整个命令报错好 —— 命令层会记日志。
fn present_document_window(app: &AppHandle, window: &tauri::WebviewWindow) {
    let _ = window.unminimize();
    center_on_cursor_monitor(app, window);
    let _ = window.show();
    let _ = window.set_focus();
}

/// 把窗口摆到鼠标所在那块屏的正中间。
///
/// 取不到屏幕或窗口尺寸时什么都不做 —— 保持原位置也比摆到一个
/// 算错的坐标上强。
fn center_on_cursor_monitor(app: &AppHandle, window: &tauri::WebviewWindow) {
    let Some(monitor) = monitor_under_cursor(app) else {
        return;
    };
    let Ok(size) = window.outer_size() else {
        return;
    };

    let (x, y) = centered_origin(&ScreenBox::from(&monitor), size.width, size.height);
    let _ = window.set_position(tauri::PhysicalPosition::new(x, y));
}

/// 居中摆放的坐标计算（纯函数，便于测试）。
///
/// ## 为什么要用 i64 中间量
///
/// 窗口可能比屏幕还大（用户在 13 寸屏上把设置窗口拉到很宽，然后
/// 换到一块更小的屏上）。那种情况下 `宽度差 / 2` 是**负数**，
/// 在 u32 上会回绕成一个巨大的正数，把窗口甩到屏幕外。
/// 用有符号计算就不会：负数只是让窗口的左边缘超出屏幕一点，仍然可见。
fn centered_origin(screen: &ScreenBox, window_width: u32, window_height: u32) -> (i32, i32) {
    let dx = (screen.width as i64 - window_width as i64) / 2;
    let dy = (screen.height as i64 - window_height as i64) / 2;

    ((screen.x as i64 + dx) as i32, (screen.y as i64 + dy) as i32)
}

/// 打开设置窗口（已开则唤回到前台）。
pub fn open_settings(app: &AppHandle) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window("settings") {
        present_document_window(app, &window);
        return Ok(());
    }

    // 走到这里说明配置里声明的窗口没被创建出来。兜底也要保持一致的
    // 外观：不透明（设置页自己画实底，见 global.css 的说明）。
    let window = WebviewWindowBuilder::new(
        app,
        "settings",
        WebviewUrl::App("index.html?view=settings".into()),
    )
    .title("Tacet 设置")
    .inner_size(520.0, 640.0)
    .title_bar_style(tauri::TitleBarStyle::Overlay)
    .build()?;

    present_document_window(app, &window);

    Ok(())
}

/// 打开今日记录窗口（已开则唤回到前台）。
pub fn open_today(app: &AppHandle) -> tauri::Result<()> {
    if let Some(window) = app.get_webview_window("today") {
        present_document_window(app, &window);
        return Ok(());
    }

    let window = WebviewWindowBuilder::new(
        app,
        "today",
        WebviewUrl::App("index.html?view=today".into()),
    )
    .title("今天的记录")
    .inner_size(460.0, 560.0)
    .title_bar_style(tauri::TitleBarStyle::Overlay)
    .build()?;

    present_document_window(app, &window);

    Ok(())
}

/// 发一条系统通知。
///
/// 通知文案由 `state::notification_text` 生成（那里遵守 PRD §5 的语气规范）。
/// 这里只负责把它送出去，并在失败时**静默降级** ——
/// 用户拒绝了通知权限，不该导致任何报错弹窗（原则 7 的精神：
/// 能力缺失不是打扰的理由）。
pub fn send_notification(app: &AppHandle, title: &str, body: &str) -> tauri::Result<()> {
    use tauri_plugin_notification::NotificationExt;

    match app.notification().builder().title(title).body(body).show() {
        Ok(()) => Ok(()),
        Err(err) => {
            // 权限被拒 / 系统不支持：记一条日志就够了。
            // 界面上的「这台电脑上的能力」那一节会如实说明状态。
            crate::logging::warn(&format!("通知发送失败（已静默降级）：{err}"));
            Ok(())
        }
    }
}

/// 广播一条事件给所有界面窗口。
pub fn broadcast(app: &AppHandle, payload: serde_json::Value) {
    let _ = app.emit("tacet:event", payload);
}

/// 广播「用户想收起休息界面」。
///
/// 由幕布窗口（副屏上那层遮挡）在用户点击或按 Esc 时触发，
/// 主窗口收到后按自己的阶段决定该做什么 —— 见 `commands::dismiss_break`。
///
/// 用事件而不是直接调命令：幕布和主窗口是两个独立窗口，
/// 它们之间没有调用关系；而事件通道本来就是为这种「广播给所有界面」
/// 设计的（快照推送走的就是同一条路）。
pub fn broadcast_dismiss(app: &AppHandle) {
    broadcast(app, serde_json::json!({ "type": "dismiss" }));
}

/// 命令层用的状态类型别名。
pub type SharedState = Arc<Mutex<AppState>>;

// ============================================================ 测试

#[cfg(test)]
mod tests {
    use super::*;

    /// 造一块屏。参数就是位置和尺寸，够用了。
    fn screen(x: i32, y: i32, width: u32, height: u32) -> ScreenBox {
        ScreenBox {
            x,
            y,
            width,
            height,
        }
    }

    /// 笔记本自带屏（逻辑 1440×900，原点在 (0,0)）。
    fn builtin() -> ScreenBox {
        screen(0, 0, 1440, 900)
    }

    /// 一台外接显示器，摆在笔记本右边。
    fn external() -> ScreenBox {
        screen(1440, 0, 2560, 1440)
    }

    #[test]
    fn 只有一块屏时不需要幕布() {
        // 单屏用户（大多数）走的就是这条路 —— 不该平白多出窗口。
        let targets = veil_targets(&[builtin()], Some(builtin()));
        assert!(targets.is_empty(), "单屏不该产生幕布，实际：{targets:?}");
    }

    #[test]
    fn 接了两块屏时只盖非主屏那块() {
        let screens = [builtin(), external()];
        let targets = veil_targets(&screens, Some(builtin()));

        assert_eq!(targets, vec![1], "应该只盖外接屏");
    }

    #[test]
    fn 主屏在外接屏右边时也能认出来() {
        // 用户把主屏设成了外接显示器，笔记本在左边。
        // 这个用例是防「默认主屏总在 (0,0)」这种假设 —— 那是错的。
        let screens = [screen(-1440, 0, 1440, 900), builtin()];
        let targets = veil_targets(&screens, Some(builtin()));

        assert_eq!(targets, vec![0], "主屏在列表第二位时要正确跳过它");
    }

    #[test]
    fn 三块屏时盖住另外两块() {
        // 笔记本 + 两台外接屏：合上盖只用外接屏的人不少，
        // 所以这里也把「主屏不在第一项」的情况覆盖了。
        let left = screen(-2560, 0, 2560, 1440);
        let right = screen(1440, 0, 2560, 1440);
        let screens = [left, builtin(), right];

        let targets = veil_targets(&screens, Some(builtin()));

        assert_eq!(targets, vec![0, 2], "三块屏时应该盖住另外两块");
        assert!(
            !targets.contains(&1),
            "主屏不能出现在幕布列表里 —— 那会把倒计时和按钮一起糊掉"
        );
    }

    #[test]
    fn 认不出主屏时全部盖住() {
        // 降级方向必须选对：宁可多盖一块（用户还能在主屏上操作），
        // 也不要漏盖 —— 漏盖意味着休息可以被绕过，这个功能就白做了。
        let screens = [builtin(), external()];
        let targets = veil_targets(&screens, None);

        assert_eq!(targets, vec![0, 1], "认不出主屏时要全盖");
    }

    /// 回归测试：幕布的编号必须**连续**，且与屏幕下标解耦。
    ///
    /// ## 这个 bug 长什么样
    ///
    /// 幕布窗口的 label 在两条路径上被算出来：启动时预建、提醒时显示。
    /// 曾经两边用了不同的编号口径 —— 一边用「屏幕下标」，一边用
    /// 「第几个要盖的屏」。三块屏、主屏在中间时：
    ///
    /// ```text
    ///   预建：下标 0 和 2        → veil-0, veil-2
    ///   显示：序号 0 和 1        → veil-0, veil-1
    /// ```
    ///
    /// 于是 `veil-1` 永远不存在（每次都现建，副屏晚十几秒才蒙住），
    /// 而 `veil-2` 建出来没人用（白占一份 WebView 的内存）。
    /// 这类问题在单屏和双屏机器上**完全看不到**，只有三块屏才暴露。
    #[test]
    fn 幕布编号连续且与屏幕下标无关() {
        // 笔记本在中间，左右各一块外接屏
        let screens = [
            screen(-2560, 0, 2560, 1440),
            builtin(),
            screen(1440, 0, 2560, 1440),
        ];

        let labels = veil_labels(&screens, Some(builtin()));

        assert_eq!(
            labels.iter().map(|(l, _)| l.as_str()).collect::<Vec<_>>(),
            vec!["break-veil-0", "break-veil-1"],
            "编号必须是连续的 0、1 —— 中间不能跳过 1 直接到 2"
        );
        assert_eq!(
            labels.iter().map(|(_, i)| *i).collect::<Vec<_>>(),
            vec![0, 2],
            "编号映射回屏幕下标时，要指到真正该盖的那两块"
        );
    }

    /// 单屏时不该产出任何幕布 —— 预建那条路也走这个判断。
    #[test]
    fn 单屏时没有幕布() {
        let screens = [builtin()];
        assert!(
            veil_labels(&screens, Some(builtin())).is_empty(),
            "只有一块屏时不存在「副屏」，不该建任何幕布窗口"
        );
    }

    #[test]
    fn 同型号的两块外接屏不会被当成同一块() {
        // 这是「不比较 name()」那个决定的原因：两块同型号的屏名字一样，
        // 但位置不同。用位置+尺寸判断才靠得住。
        let a = screen(1440, 0, 2560, 1440);
        let b = screen(4000, 0, 2560, 1440);

        assert_ne!(a, b, "位置不同就是两块不同的屏");
        assert_eq!(
            veil_targets(&[a, b], Some(a)),
            vec![1],
            "同型号的屏不能因为名字一样就被误判成同一块"
        );
    }

    #[test]
    fn 幕布的标签前缀不会和主窗口撞名() {
        // 主窗口的 label 是 "break"。如果幕布也用 "break" 系列的名字，
        // get_webview_window("break") 可能取到幕布，整个休息流程就错乱了。
        assert!(!VEIL_PREFIX.is_empty());
        assert!(
            !"break".starts_with(VEIL_PREFIX),
            "主窗口的 label 不能落在幕布的前缀下"
        );
    }

    // ======================================================== 常规窗口居中

    #[test]
    fn 窗口居中于屏幕() {
        // 笔记本屏 1440×900，窗口 520×640：
        // x = (1440-520)/2 = 460，y = (900-640)/2 = 130
        let (x, y) = centered_origin(&builtin(), 520, 640);

        assert_eq!((x, y), (460, 130));
    }

    #[test]
    fn 屏幕原点不在零零时也居中() {
        // 外接屏摆在笔记本右边，原点 (1440, 0)。窗口必须落在这块屏的
        // 中央，而不是 (0,0) 附近的绝对坐标 —— 这个用例防的是
        // 「忘了加屏幕原点」这种看起来对、实际跑到另一块屏上的错误。
        let external = screen(1440, 0, 2560, 1440);
        let (x, y) = centered_origin(&external, 520, 640);

        assert_eq!(x, 1440 + (2560 - 520) / 2);
        assert_eq!(y, (1440 - 640) / 2);
        assert!(x >= 1440, "窗口必须落在外接屏上，实际 x={x}");
    }

    #[test]
    fn 窗口比屏幕还大时不会跑到屏幕外() {
        // 用户在 13 寸屏上把设置窗口拉宽，然后换到一块更小的屏上。
        // 宽度差为负时若用无符号计算会回绕成一个巨大的正数，
        // 把窗口甩到屏幕外 —— 这里断言它只是稍微超出左/上边缘。
        let small = screen(0, 0, 400, 300);
        let (x, y) = centered_origin(&small, 520, 640);

        assert_eq!((x, y), (-60, -170), "应当允许负值，而不是回绕");
    }

    #[test]
    fn 屏幕原点为负时也居中() {
        // 有些显示器配置会把屏放在负坐标区（例如主屏在右、副屏在左）。
        let left = screen(-1440, 0, 1440, 900);
        let (x, y) = centered_origin(&left, 520, 640);

        assert_eq!(x, -1440 + (1440 - 520) / 2);
        assert!(x < 0, "负原点的屏上，窗口的 x 也可能是负的");
        assert_eq!(y, (900 - 640) / 2);
    }

    // ======================================================== 面板定位
    //
    // 下面这组用例用的是真实机器的几何（开发机上用 CGDisplayBounds 量出来的），
    // 不是随手编的数字。用意是：这台机器恰好是「双屏 + 副屏的顶边比主屏还高」
    // 这种最容易算错的排布 —— 副屏原点的 y 是负的，任何「默认从 0 开始」
    // 的写法都会在这里露馅。

    /// 本机内置屏（物理像素，2 倍缩放）。
    fn builtin_px() -> ScreenBox {
        screen(0, 0, 3456, 2234)
    }

    /// 本机外接屏 —— 摆在右边，而且**顶边比内置屏高 370 像素**，
    /// 所以原点的 y 是负数。这是真实值。
    fn external_px() -> ScreenBox {
        screen(3456, -370, 3840, 2160)
    }

    /// 2 倍屏上的那组间距（就是 `position_panel` 算出来的值）。
    fn insets() -> Insets {
        Insets {
            gap: 12,
            margin: 32,
            fallback_top: 66,
        }
    }

    /// 造一枚托盘图标（物理像素）。
    fn icon(x: i32, y: i32, width: i32, height: i32) -> TrayAnchor {
        TrayAnchor {
            x,
            y,
            width,
            height,
        }
    }

    /// 本机内置屏菜单栏上那枚 Tacet 图标（实测 118×66 物理像素）。
    fn builtin_icon() -> TrayAnchor {
        icon(2060, 0, 118, 66)
    }

    #[test]
    fn 面板贴着图标的下沿并且水平居中对齐() {
        let icon = builtin_icon();
        let (x, y) = panel_origin(&builtin_px(), 752, Some(icon), insets());

        assert_eq!(x, icon.center_x() - 752 / 2, "面板要与图标中心对齐");
        assert_eq!(
            y,
            icon.bottom() + 12,
            "顶边要跟着图标走，而不是靠猜菜单栏多高"
        );
    }

    #[test]
    fn 菜单栏多高不用猜_面板跟着图标走() {
        // 内置屏的菜单栏是 33 逻辑点（66 物理像素），外接屏是 30 逻辑点
        // （60 物理像素）—— 两块屏不一样高。写死任何一个值，在另一块屏上
        // 都会差几个像素。跟着图标下沿走就自动是对的。
        let builtin = panel_origin(&builtin_px(), 752, Some(builtin_icon()), insets());
        let external_icon = icon(5896, -370, 118, 60);
        let external = panel_origin(&external_px(), 752, Some(external_icon), insets());

        assert_eq!(builtin.1, 78, "内置屏：66 + 12");
        assert_eq!(external.1, -298, "外接屏：-370 + 60 + 12");
        assert_eq!(
            external.1 - external_icon.y,
            72,
            "顶边到屏幕顶边的距离 = 菜单栏高度 + 空隙，因屏而异"
        );
    }

    #[test]
    fn 图标贴着屏幕右边时面板会被收回屏幕内() {
        // 托盘图标基本都挤在菜单栏最右侧，而面板比图标宽得多 ——
        // 「与图标居中对齐」算出来的面板常常有一半在屏幕外。
        let screen = builtin_px();
        let icon = icon(screen.width as i32 - 120, 0, 118, 66);
        let (x, _) = panel_origin(&screen, 752, Some(icon), insets());

        assert!(
            x + 752 <= screen.width as i32 - 32,
            "面板右边缘必须留在屏幕内，实际 {}",
            x + 752
        );
        assert_eq!(x, screen.width as i32 - 752 - 32, "应当夹到右边距上");
    }

    #[test]
    fn 屏幕比面板还窄时不会把面板推到屏幕左边之外() {
        // 极端情况：用户把窗口拉得很宽，又接了一块小屏。
        // `clamp` 在 min > max 时会 panic —— 这个用例保证那条路不会崩，
        // 而且退化的方向是「左边对齐」（看得见）而不是「整个飞出去」。
        let tiny = screen(0, 0, 400, 300);
        let (x, y) = panel_origin(&tiny, 752, Some(icon(100, 0, 40, 24)), insets());

        assert_eq!(x, 32, "夹紧后应当贴齐左边缘");
        assert_eq!(y, 36, "24 + 12");
    }

    #[test]
    fn 没有图标位置时面板落在屏幕右上角() {
        // 单实例唤醒、Dock 点击这两条路径拿不到图标矩形。
        let (x, y) = panel_origin(&builtin_px(), 752, None, insets());

        assert_eq!(x, 3456 - 752 - 32, "退化成贴着这块屏的右上角");
        assert_eq!(y, 66, "按估计的菜单栏高度往下让开");
    }

    #[test]
    fn 点落在哪块屏上按左闭右开判定() {
        let screens = [screen(0, 0, 100, 100), screen(100, 0, 100, 100)];

        assert_eq!(
            screen_index_at(&screens, 0, 0),
            Some(0),
            "左上角属于第一块屏"
        );
        assert_eq!(screen_index_at(&screens, 99, 99), Some(0));
        assert_eq!(
            screen_index_at(&screens, 100, 50),
            Some(1),
            "边界上的点归右边那块屏 —— 两块屏紧挨着时既不留缝也不重叠"
        );
        assert_eq!(screen_index_at(&screens, 199, 50), Some(1));
        assert_eq!(screen_index_at(&screens, 200, 50), None, "屏幕之外没有屏");
        assert_eq!(screen_index_at(&screens, 50, -1), None, "屏幕上方也没有");
    }

    #[test]
    fn 副屏菜单栏上的图标会把面板引到副屏() {
        // 这条是 bug 的回归测试。修之前 `position_panel` 写死了主屏：
        // 用户在副屏的菜单栏点图标，面板却出现在笔记本屏幕上
        //（面板写死 1336,33 = 内置屏右上角）。
        let screens = [builtin_px(), external_px()];
        let icon = icon(5896, -370, 118, 60);

        assert_eq!(
            screen_index_at(&screens, icon.center_x(), icon.center_y()),
            Some(1),
            "副屏上的图标必须命中副屏"
        );

        let (x, y) = panel_origin(&screens[1], 752, Some(icon), insets());
        assert!(x >= 3456, "面板必须落在外接屏的横向范围内，实际 x={x}");
        assert!(
            (-370..1790).contains(&y),
            "面板必须落在外接屏的竖向范围内，实际 y={y}"
        );
    }

    #[test]
    fn 托盘矩形能转成物理像素() {
        // macOS 给的是物理像素（tray-icon 内部已经 to_physical 过）。
        let physical = tauri::Rect {
            position: tauri::Position::Physical(tauri::PhysicalPosition::new(5896, -370)),
            size: tauri::Size::Physical(tauri::PhysicalSize::new(118, 60)),
        };
        let anchor = TrayAnchor::from(&physical);

        assert_eq!((anchor.x, anchor.y), (5896, -370));
        assert_eq!((anchor.width, anchor.height), (118, 60));
        assert_eq!(anchor.center_x(), 5955);
        assert_eq!(anchor.bottom(), -310);

        // 逻辑坐标那个分支也要兜住（别的平台可能给逻辑值）。
        let logical = tauri::Rect {
            position: tauri::Position::Logical(tauri::LogicalPosition::new(2948.0_f64, -185.0_f64)),
            size: tauri::Size::Logical(tauri::LogicalSize::new(59.0_f64, 30.0_f64)),
        };
        let anchor = TrayAnchor::from(&logical);

        assert_eq!((anchor.x, anchor.y), (2948, -185));
        assert_eq!((anchor.width, anchor.height), (59, 30));
    }
}
