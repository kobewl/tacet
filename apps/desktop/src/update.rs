//! 应用内自动更新 —— 检查 GitHub Release，下载带签名的更新包并安装。
//!
//! ## 为什么这件事能成立（以及不能做到什么）
//!
//! Tacet 是**未做 Apple 代码签名与公证**的个人测试包，走 GitHub Releases 分发。
//! 这里用的是 Tauri 更新器自己的 **minisign** 链路，与 Apple 签名是两条独立的事：
//!
//! | | Apple 代码签名 / 公证 | Tauri 更新签名（本模块） |
//! | --- | --- | --- |
//! | 验证什么 | 应用的身份 | 更新包有没有被篡改 |
//! | 需要什么 | 付费开发者账号 | 一对本地生成的密钥 |
//! | 本轮是否具备 | ❌ 没有 | ✅ 有 |
//!
//! 所以这一套能保证「下到的包确实是发布者签的」，但**不能**消除第一次
//! 打开未签名应用时的 Gatekeeper 提示，也不能保证 TCC 权限跨更新保留。
//! 那两件事只有稳定的 Apple Developer ID 能解决 —— 见
//! `docs/release/自动更新.md`。
//!
//! ## 密钥的摆放位置（一个容易搞混的地方）
//!
//! - **公钥**：写在 `tauri.conf.json` 的 `plugins.updater.pubkey`，随应用分发；
//! - **私钥**：只存在于 GitHub Secret `TAURI_SIGNING_PRIVATE_KEY` 与开发机的
//!   `~/.tauri/tacet-updater.key`，**永远不进仓库**。
//!
//! 校验在下载后、安装前发生：签名不匹配的包会被插件直接拒绝，
//! 连解压都不会发生。
//!
//! ## 为什么「检查」和「安装」是两个命令
//!
//! 检查要快（用户点了按钮立刻要有反应），安装要慢（下载几十兆、可能失败）。
//! 拆开后，界面上「发现新版本 → 用户看清版本号和说明 → 才下载」这条路径
//! 才走得通；合成一个命令的话，用户会在不知道要装什么的情况下被下载。
//!
//! ## 与「单实例」插件的关系（这里曾经差点出错）
//!
//! 更新完要重启应用，而 Tacet 装了 `tauri-plugin-single-instance`：
//! 新进程启动时会去连一个 Unix socket，**如果旧进程还在，新进程会直接退出**，
//! 表现为「点完更新，应用消失了」。
//!
//! 好在 Tauri 的重启顺序是对的：`RunEvent::Exit` 会先派发给所有插件
//! （单实例插件在这里删掉 socket），之后才真正拉起新进程。
//! 这条顺序是 `App::restart()` 的实现细节，不是文档承诺 ——
//! 所以升级 Tauri 大版本时，**这一条要重新验证**。

use tauri::{AppHandle, Emitter};
use tauri_plugin_updater::UpdaterExt;

use crate::commands::CmdResult;

/// 检查更新时，把新版本信息交给界面的载荷。
///
/// 字段用 camelCase 序列化 —— 前端类型契约见 `packages/ui/src/types.ts`
/// 的 `UpdateInfo`，两边必须同步改。
#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    /// 当前运行的版本。
    pub current_version: String,
    /// 远端最新版本。
    pub version: String,
    /// 发布说明（Release Notes）；没有时为空字符串。
    pub notes: String,
    /// 发布说明页地址，供「手动下载」用。
    pub release_url: String,
}

/// 更新包下载进度事件名。
///
/// 用 `update:progress` 而不是 `tacet:event`：后者是**业务状态**的推送通道
///（状态机、快照），混进一个 UI 进度事件会让那条通道的语义变模糊。
const PROGRESS_EVENT: &str = "update:progress";

/// 检查是否有新版本。
///
/// 返回 `None` 表示已经是最新的 —— 这是一个**成功**的结果，不是错误。
/// 「没有更新」和「检查失败」在界面上的样子完全不同，所以必须分开表达。
///
/// ## 网络失败的处理
///
/// 最典型的失败是 `latest.json` 不存在（发了 Release 但没附更新清单，
/// 或仓库还没发过任何版本）。这时插件报
/// `Could not fetch a valid release JSON from the remote`，
/// 听起来像故障，其实是一个可解释的状态。这里把它翻译成能指导用户
/// 下一步动作的话。
#[tauri::command]
pub async fn check_update(app: AppHandle) -> CmdResult<Option<UpdateInfo>> {
    let updater = app
        .updater()
        .map_err(|err| format!("更新器初始化失败：{err}"))?;

    match updater.check().await {
        Ok(Some(update)) => Ok(Some(UpdateInfo {
            current_version: update.current_version.clone(),
            release_url: format!(
                "https://github.com/kobewl/tacet/releases/tag/v{}",
                update.version
            ),
            version: update.version.clone(),
            notes: update.body.clone().unwrap_or_default(),
        })),
        Ok(None) => Ok(None),
        Err(err) => {
            crate::logging::warn(&format!("检查更新失败：{err}"));
            Err(translate_check_error(&err.to_string()).into())
        }
    }
}

/// 用系统默认浏览器打开发布页（更新失败时的手动下载兜底）。
#[tauri::command]
pub fn open_release_page(app: AppHandle, url: String) -> CmdResult<()> {
    // 只允许打开本项目 Release 页下的地址。
    // 前端传来的字符串是不可信输入 —— 虽然这个窗口加载的是我们自己的页面，
    // 但「能被任意 URL 驱动的 open」是一个不该留的口子。
    const ALLOWED_PREFIX: &str = "https://github.com/kobewl/tacet/releases";

    if !url.starts_with(ALLOWED_PREFIX) {
        return Err(format!("不允许打开的地址：{url}").into());
    }

    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_url(&url, None::<&str>)
        .map_err(|err| format!("打不开浏览器：{err}").into())
}

/// 把插件的错误信息翻译成「用户能据此行动」的说法。
///
/// 原文照搬没有意义：用户看不懂 `Could not fetch a valid release JSON
/// from the remote`，而这句话真正想说的是「发布渠道还没就绪」。
fn translate_check_error(raw: &str) -> String {
    if raw.contains("Could not fetch a valid release JSON") {
        return "还查不到发布信息 —— 可能是仓库还没发布过版本，或者刚发布的 \
                版本缺少更新清单（latest.json）。可以先到 Releases 页面手动下载。"
            .to_string();
    }
    if raw.contains("the platform") || raw.contains("None of the fallback platforms") {
        return "最新版本里没有适用于这台电脑（Apple 芯片）的安装包。".to_string();
    }
    format!("检查更新失败：{raw}")
}

/// 下载并安装更新，完成后自动重启。
///
/// 走的是插件的 `download_and_install`：下载 → **验签** → 解压 → 替换 .app。
/// 任何一步失败都不会破坏现有安装（替换是「先备份旧的，再放新的」）。
///
/// 进度通过 [`PROGRESS_EVENT`] 推给界面；重启前不发「已完成」——
/// 因为那一刻进程已经在退出路上了，界面没机会消费它。
#[tauri::command]
pub async fn install_update(app: AppHandle) -> CmdResult<()> {
    let updater = app
        .updater()
        .map_err(|err| format!("更新器初始化失败：{err}"))?;

    let update = updater
        .check()
        .await
        .map_err(|err| format!("获取更新包失败：{err}"))?
        .ok_or_else(|| "已经是最新版本，没有可安装的更新".to_string())?;

    let version = update.version.clone();
    crate::logging::info(&format!("开始下载更新 v{version}"));

    let mut downloaded: u64 = 0;
    let progress_app = app.clone();

    let result = update
        .download_and_install(
            move |chunk, total| {
                downloaded += chunk as u64;

                // total 由服务器给；缺 Content-Length 时拿不到，这时不猜，
                // 让界面显示「正在下载」而不是一个假的百分比。
                if let Some(total) = total {
                    if total > 0 {
                        let percent = ((downloaded as f64 / total as f64) * 100.0).min(100.0);
                        let _ = progress_app.emit(PROGRESS_EVENT, percent as u32);
                    }
                }
            },
            || {},
        )
        .await;

    match result {
        Ok(()) => {
            crate::logging::info(&format!("更新 v{version} 安装完成，即将重启"));
            // 重启：新进程起来时会连上单实例的 socket 检查 ——
            // 旧进程的 socket 由插件在 RunEvent::Exit 时清掉，顺序见模块头部说明。
            app.restart();
        }
        Err(err) => {
            crate::logging::error(&format!("安装更新失败：{err}"));
            Err(format!("安装更新失败：{err}").into())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 「还没发过 Release」是最常见的一次性状态，文案要能指导下一步动作。
    ///
    /// 这个测试守住的是一件容易被改坏的事：插件原文是一句英文技术话
    /// （`Could not fetch a valid release JSON from the remote`），
    /// 用户看不懂。如果将来有人「顺手」删掉这层翻译，这里会红。
    #[test]
    fn 发布渠道未就绪时给出可行动的说法() {
        let raw = "Could not fetch a valid release JSON from the remote";
        let msg = translate_check_error(raw);

        assert!(
            msg.contains("还查不到发布信息"),
            "应当翻译成人话，而不是原样回显；实际是：{msg}"
        );
        // 必须指出下一步该干什么 —— 只说「失败了」等于没说
        assert!(
            msg.contains("Releases"),
            "应当指向手动下载的出口；实际是：{msg}"
        );
        // 不该把插件原文甩给用户
        assert!(
            !msg.contains("Could not fetch"),
            "不应回显英文原文；实际是：{msg}"
        );
    }

    /// 找不到当前平台时，要说清是「这台机器没有对应包」，而不是笼统的网络错误。
    #[test]
    fn 缺少对应平台时说清原因() {
        let msg =
            translate_check_error("the platform `darwin-aarch64` was not found in the response");
        assert!(
            msg.contains("Apple 芯片"),
            "应指出平台不匹配；实际是：{msg}"
        );
    }

    /// 认不出的错误要保留原文 —— 那是排查时唯一的线索，不能吞掉。
    #[test]
    fn 未知错误保留原文() {
        let msg = translate_check_error("some brand new failure");
        assert!(
            msg.contains("some brand new failure"),
            "未知错误不应被吞；实际是：{msg}"
        );
    }
}
