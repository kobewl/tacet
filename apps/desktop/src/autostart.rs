//! 开机自启 —— 设置页里的那个开关。
//!
//! ## 为什么实现在壳层，而不是 `platform-macos`
//!
//! 注册登录项需要一个**有 bundle 身份**的进程：系统要知道该登记谁。
//! `platform-macos` 是纯库 crate，它没有 bundle。这与通知能力是同一类问题：
//! **库层如实报告「这里不提供」，实现在壳层**
//!（见 `platform_macos` 里 `MacStartupService` 与 `MacNotificationService`）。
//!
//! ## 状态不存数据库（这一点是刻意的）
//!
//! 真正的开关是系统里的那个登录项文件（`~/Library/LaunchAgents/*.plist`）。
//! 数据库里再存一份就有**两个真相来源**：
//!
//! - 用户在「系统设置 → 通用 → 登录项」里手动改掉，数据库不知道；
//! - 系统清理了那个 plist，数据库还以为开着。
//!
//! 两边对不上时界面显示的就是假状态，用户会以为自己刚才的操作没生效。
//! 所以这里每次都**直接问系统**，显示的一定是实际生效的状态。
//!
//! ## 关于「写 LaunchAgents plist」这个实现方式
//!
//! 底层用 `tauri-plugin-autostart`，它在 macOS 上是往
//! `~/Library/LaunchAgents/` 写一个 plist。要如实说明它的**缺点**：
//!
//! | 方式 | 优点 | 缺点 |
//! | --- | --- | --- |
//! | 写 LaunchAgents plist（当前） | 对**未签名**应用也能用 | 应用被删掉后 plist 会残留 |
//! | `SMAppService`（macOS 13+） | 系统代管，卸载时自动清理 | 对未签名应用有兼容风险 |
//!
//! Tacet 是**没有 Apple 证书的未签名个人测试包**（见 `docs/release/自动更新.md`），
//! 所以选了前者。用户关掉开关时插件会删掉 plist，问题只出在
//! 「装了但没关自启就直接删应用」这一种情况 —— 残留的 plist 指向一个不存在的
//! 应用，macOS 会安静地忽略它，不会造成故障。
//!
//! 将来若购买了 Apple 开发者账号并完成签名，**这一处应该升级到 `SMAppService`**
//!（`crates/platform-macos/src/startup.rs` 里已有版本判断，就是为此留的）。
//!
//! ## 开发版禁止设置
//!
//! 开发版（`tauri dev` 编出来的 debug 二进制）要连着 Vite 开发服务器才能显示界面。
//! 把它注册成登录项，开机后它会启动 —— 但连不上 devUrl，界面上什么都没有，
//! 只在菜单栏留一个点开是空白的图标。哪怕就是开发者本人，也很难第一时间
//! 反应过来那是几天前自己随手打开的一个开关造成的。
//!
//! 所以开发版里**允许查询、禁止修改**，并把原因原样告诉用户。
//!
//! ## 为什么还要关掉系统的「登录时恢复窗口」
//!
//! 这是个容易漏掉的**第二条自启路径**。macOS 有一个独立于登录项的机制：
//! 关机时还在运行的应用，下次登录会被系统恢复（前提是用户在关机对话框里
//! 勾了「重新打开窗口」）。Tacet 是常驻菜单栏的应用，关机时多半开着 ——
//! 也就是说它会**从这条路径自己回来**，跟用户有没有打开我们那个开关无关。
//!
//! 后果是「开机自启」这个开关看起来是坏的：用户关掉了它，下次登录 Tacet
//! 照样出现。所以必须显式关掉系统恢复这条路径，让登录项成为唯一入口 ——
//! 见 [`disable_relaunch_on_login`]。

use tauri::AppHandle;
use tauri_plugin_autostart::ManagerExt;

use crate::commands::CmdResult;
use crate::logging;

/// 当前构建是否禁止改动开机自启设置。
///
/// `debug_assertions` 为真即「开发版」——`tauri dev` 与 `cargo test` 都落在
/// 这一侧，正式的 `tauri build` 落在另一侧。
///
/// 单独抽成函数是为了能被测试覆盖：判断本身简单，但它决定了用户能不能改
/// 系统状态，值得有一个明确的名字和测试。
pub fn is_autostart_locked() -> bool {
    cfg!(debug_assertions)
}

/// 读取当前是否已设置为开机自启。
///
/// 直接问系统。读不到时返回错误而不是 `false` ——「没开」和「我读不到」
/// 对用户是完全不同的信息：前者不用管，后者说明有东西坏了。
#[tauri::command]
pub fn get_autostart_enabled(app: AppHandle) -> CmdResult<bool> {
    app.autolaunch()
        .is_enabled()
        .map_err(|err| format!("读取开机自启状态失败：{err}").into())
}

/// 打开或关闭开机自启，立即生效（不经过设置页的「保存」）。
///
/// ## 为什么立即生效
///
/// 它改的是**系统里的登录项**，不是本应用的配置：没有「草稿」这个概念，
/// 也就没有可保存的东西。如果做成「改一下、点保存才生效」，用户点了保存
/// 却看到开关没动，反而会怀疑到底生效没有。
///
/// 失败时前端会把开关**弹回原值** —— 让控件停在用户点的位置上，
/// 他会以为已经设好了。
#[tauri::command]
pub fn set_autostart_enabled(app: AppHandle, enabled: bool) -> CmdResult<()> {
    if is_autostart_locked() {
        return Err(
            "开发版不能设置开机自启：它需要本地的开发服务器才能显示界面，\
                    注册成登录项后开机只会留下一个点开是空白的菜单栏图标。\
                    用正式打包的版本再开这个开关。"
                .to_string()
                .into(),
        );
    }

    let manager = app.autolaunch();
    let result = if enabled {
        manager.enable()
    } else {
        manager.disable()
    };

    // 成败都记日志：否则「我明明打开了，怎么没生效」这种问题
    // 事后完全查不到线索。措辞里带上动作方向，不然只看到一句系统错误
    // 判断不出是打开失败还是关闭失败。
    let action = if enabled { "打开" } else { "关闭" };

    match result {
        Ok(()) => {
            logging::info(&format!("开机自启已{action}"));
            Ok(())
        }
        Err(err) => {
            logging::error(&format!("{action}开机自启失败：{err}"));
            Err(format!("{action}开机自启失败：{err}").into())
        }
    }
}

/// 告诉 macOS：登录时**不要**用「重新打开窗口」把本应用恢复回来。
///
/// 理由见模块头部「为什么还要关掉系统的登录时恢复窗口」。一句话：
/// 不关掉它，设置页那个自启开关就是坏的 —— 关掉了也照样自启。
///
/// Apple 对这个方法的说明正好就是我们的场景：
///
/// > 如果应用因为通过其它机制启动（例如 launchd）而不应被重新启动，
/// > 推荐调用一次 `disableRelaunchOnLogin`，并且**永远不要**配对调用 enable。
///
/// 唯一约束：必须主线程调用（`NSApplication` 带 `MainThreadOnly` 约束），
/// 所以调用点在 `RunEvent::Ready`。
#[cfg(target_os = "macos")]
pub fn disable_relaunch_on_login() {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSApplication;

    let Some(mtm) = MainThreadMarker::new() else {
        // 只可能是调用点写错了（放到了非主线程）。退化成「多一条自启路径」，
        // 而不是让进程崩溃 —— 应用本身照常工作，只是关掉开关后仍可能被系统恢复。
        logging::warn("disableRelaunchOnLogin 必须在主线程调用，已跳过");
        return;
    };

    NSApplication::sharedApplication(mtm).disableRelaunchOnLogin();
}

/// 非 macOS 上的空实现（保持 workspace 可跨平台编译）。
#[cfg(not(target_os = "macos"))]
pub fn disable_relaunch_on_login() {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 开发版锁定自启设置() {
        // 测试构建必然是 debug，所以这里断言的是「开发环境下锁定」。
        // 正式构建的那一侧由 cfg 保证，测试里不需要（也无法）覆盖。
        assert!(
            is_autostart_locked(),
            "测试构建属于开发版，应当锁定自启设置"
        );
    }
}
