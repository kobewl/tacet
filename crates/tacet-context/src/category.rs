//! 应用分类 —— 把 Bundle ID 变成「这是一类什么应用」。
//!
//! ## 为什么要在本地做这件事
//!
//! 数据模型原则 3 说「只存摘要，不存原文」。分类就是那个「摘要」：
//! 我们后续要判断「用户在写代码吗」「在开会吗」，
//! 但**不需要知道他在写什么代码、开什么会**。
//!
//! 所以分类规则全部在本地、基于 Bundle ID 做，不上传任何东西。
//!
//! ## 为什么先查表而不是猜
//!
//! Bundle ID 是稳定标识（`com.microsoft.VSCode` 不会变），
//! 而应用显示名会因为界面语言而不同（「访达」/「Finder」）。
//! 所以识别优先看 Bundle ID，显示名只作为兜底的关键词线索。
//!
//! ## 表格是「可长大」的
//!
//! 这张表将来会越来越长（用户装了什么新软件）。它的设计目标是：
//! **加一行数据，不改一行逻辑**。所以识别函数只做两件事：
//! 查精确表，再查关键词表，最后落到 `Other`。

use tacet_core::model::AppCategory;

/// 精确匹配表：`(Bundle ID 前缀, 类别)`。
///
/// 用「前缀」而不是全等，是为了覆盖同一应用的一系列 Bundle ID
/// （例如 `com.google.Chrome` 与 `com.google.Chrome.canary`）。
const EXACT_PREFIXES: &[(&str, AppCategory)] = &[
    // 编辑器与 IDE —— 大概率在心流中
    ("com.microsoft.VSCode", AppCategory::Editor),
    ("com.apple.dt.Xcode", AppCategory::Editor),
    ("com.jetbrains.", AppCategory::Editor),
    ("com.sublimetext.", AppCategory::Editor),
    ("dev.zed.Zed", AppCategory::Editor),
    ("com.neovim", AppCategory::Editor),
    ("org.vim.", AppCategory::Editor),
    ("abnerworks.Typora", AppCategory::Editor),
    ("com.typora.", AppCategory::Editor),
    // 终端
    ("com.apple.Terminal", AppCategory::Terminal),
    ("com.googlecode.iterm2", AppCategory::Terminal),
    ("dev.warp.Warp-Stable", AppCategory::Terminal),
    ("net.kovidgoyal.kitty", AppCategory::Terminal),
    ("com.github.wez.wezterm", AppCategory::Terminal),
    // 会议
    ("us.zoom.xos", AppCategory::Meeting),
    ("com.microsoft.teams", AppCategory::Meeting),
    ("com.apple.FaceTime", AppCategory::Meeting),
    ("com.tinyspeck.slackmacgap", AppCategory::Communication),
    ("com.hnc.Discord", AppCategory::Communication),
    ("com.tencent.meeting", AppCategory::Meeting),
    ("com.alibaba.DingTalkMac", AppCategory::Meeting),
    // 浏览器
    ("com.apple.Safari", AppCategory::Browser),
    ("com.google.Chrome", AppCategory::Browser),
    ("org.mozilla.firefox", AppCategory::Browser),
    ("com.microsoft.edgemac", AppCategory::Browser),
    ("company.thebrowser.Browser", AppCategory::Browser),
    ("com.tencent.xinWeChat", AppCategory::Communication),
    // 影音
    ("com.apple.QuickTimePlayerX", AppCategory::Media),
    ("com.spotify.client", AppCategory::Media),
    ("com.netflix.", AppCategory::Media),
    ("com.tencent.tencentvideo", AppCategory::Media),
];

/// 关键词表：当 Bundle ID 没匹配上时，用显示名里的关键词兜底。
///
/// 这条路径是**不可靠的**（用户可能把应用改名），所以它只做兜底，
/// 并且关键词都写得比较保守 —— 宁可分到 `Other`，也不要分错。
const NAME_KEYWORDS: &[(&str, AppCategory)] = &[
    ("code", AppCategory::Editor),
    ("studio", AppCategory::Editor),
    ("editor", AppCategory::Editor),
    ("idea", AppCategory::Editor),
    ("sublime", AppCategory::Editor),
    ("terminal", AppCategory::Terminal),
    ("iterm", AppCategory::Terminal),
    ("warp", AppCategory::Terminal),
    ("zoom", AppCategory::Meeting),
    ("meet", AppCategory::Meeting),
    ("teams", AppCategory::Meeting),
    ("safari", AppCategory::Browser),
    ("chrome", AppCategory::Browser),
    ("firefox", AppCategory::Browser),
    ("edge", AppCategory::Browser),
    ("browser", AppCategory::Browser),
    ("player", AppCategory::Media),
];

/// 把一个应用归类。
///
/// 依次尝试：Bundle ID 精确前缀 → 显示名关键词 → `Other`。
pub fn classify(bundle_id: &str, name: &str) -> AppCategory {
    if let Some(category) = classify_by_bundle(bundle_id) {
        return category;
    }

    classify_by_name(name)
}

/// 只看 Bundle ID 归类。
fn classify_by_bundle(bundle_id: &str) -> Option<AppCategory> {
    // 大小写不敏感：Bundle ID 理论上大小写敏感，但用户数据里出现过差异，
    // 而误判的代价只是分类不准，不会造成功能故障。
    let lower = bundle_id.to_ascii_lowercase();

    EXACT_PREFIXES
        .iter()
        .find(|(prefix, _)| lower.starts_with(&prefix.to_ascii_lowercase()))
        .map(|(_, category)| *category)
}

/// 只看显示名归类。
fn classify_by_name(name: &str) -> AppCategory {
    let lower = name.to_ascii_lowercase();

    NAME_KEYWORDS
        .iter()
        .find(|(keyword, _)| lower.contains(keyword))
        .map(|(_, category)| *category)
        .unwrap_or(AppCategory::Other)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 常见编辑器被正确识别() {
        assert_eq!(
            classify("com.microsoft.VSCode", "Visual Studio Code"),
            AppCategory::Editor
        );
        assert_eq!(classify("com.apple.dt.Xcode", "Xcode"), AppCategory::Editor);
        assert_eq!(
            classify("com.jetbrains.intellij", "IntelliJ IDEA"),
            AppCategory::Editor
        );
    }

    #[test]
    fn 终端被识别且算作专注场景() {
        let category = classify("com.googlecode.iterm2", "iTerm2");
        assert_eq!(category, AppCategory::Terminal);
        assert!(category.implies_focus());
    }

    #[test]
    fn 会议与通讯应用被区分开() {
        // 这两个类别在 v0.2 会有不同的处理：会议要降级干预，
        // 通讯只是「可能不宜强打断」。现在分清楚，将来才不用改数据。
        assert_eq!(classify("us.zoom.xos", "zoom.us"), AppCategory::Meeting);
        assert_eq!(
            classify("com.tinyspeck.slackmacgap", "Slack"),
            AppCategory::Communication
        );
    }

    #[test]
    fn 浏览器与影音被识别() {
        assert_eq!(
            classify("com.google.Chrome", "Google Chrome"),
            AppCategory::Browser
        );
        assert_eq!(
            classify("com.spotify.client", "Spotify"),
            AppCategory::Media
        );
    }

    #[test]
    fn 未知应用落到其它() {
        assert_eq!(
            classify("com.example.unknown", "某个没见过的应用"),
            AppCategory::Other
        );
        assert_eq!(classify("", ""), AppCategory::Other);
    }

    #[test]
    fn 大写形式的bundle_id也能识别() {
        assert_eq!(
            classify("COM.MICROSOFT.VSCODE", "Visual Studio Code"),
            AppCategory::Editor
        );
    }

    #[test]
    fn bundle_id没命中时用显示名兜底() {
        // 用户装了一个自己打包的编辑器，Bundle ID 是自定义的，
        // 但显示名里带 "Studio" —— 应当能兜住。
        assert_eq!(
            classify("com.custom.thing", "My Awesome Editor"),
            AppCategory::Editor
        );
        assert_eq!(
            classify("com.custom.browser", "SuperBrowser"),
            AppCategory::Browser
        );
    }

    #[test]
    fn bundle_id的判定优先于显示名() {
        // 显示名里带 "meet"（可能被误判成会议），但 Bundle ID 明确是浏览器。
        // 精确识别的结果必须胜出。
        assert_eq!(
            classify("com.google.Chrome", "Meet Helper Extension"),
            AppCategory::Browser
        );
    }

    #[test]
    fn 中文应用名不会被误判() {
        // 中文名里不可能含英文关键词，所以会落到 Other —— 这是可接受的：
        // 真正重要的应用都有稳定的 Bundle ID，走的是精确路径。
        assert_eq!(classify("com.unknown.app", "访达"), AppCategory::Other);
    }

    #[test]
    fn 分类表里没有重复的bundle_id前缀() {
        // 重复前缀会让「先匹配到哪个」取决于数组顺序，是个隐蔽的坑。
        let mut seen: Vec<String> = Vec::new();

        for (prefix, _) in EXACT_PREFIXES {
            let lower = prefix.to_ascii_lowercase();
            assert!(!seen.contains(&lower), "Bundle ID 前缀 {prefix} 重复了");
            seen.push(lower);
        }
    }

    #[test]
    fn 关键词表里没有重复关键词() {
        let mut seen: Vec<String> = Vec::new();

        for (keyword, _) in NAME_KEYWORDS {
            let lower = keyword.to_ascii_lowercase();
            assert!(!seen.contains(&lower), "关键词 {keyword} 重复了");
            seen.push(lower);
        }
    }

    #[test]
    fn 分类结果覆盖所有类别() {
        // 确认这张表确实能产出多种类别，而不是「写了很多但都指向 Editor」。
        let samples = [
            ("com.microsoft.VSCode", "Visual Studio Code"),
            ("com.apple.Safari", "Safari"),
            ("us.zoom.xos", "zoom.us"),
            ("com.tinyspeck.slackmacgap", "Slack"),
            ("com.spotify.client", "Spotify"),
            ("com.apple.Terminal", "Terminal"),
        ];

        let categories: Vec<AppCategory> = samples
            .iter()
            .map(|(bundle, name)| classify(bundle, name))
            .collect();

        assert!(categories.contains(&AppCategory::Editor));
        assert!(categories.contains(&AppCategory::Browser));
        assert!(categories.contains(&AppCategory::Meeting));
        assert!(categories.contains(&AppCategory::Communication));
        assert!(categories.contains(&AppCategory::Media));
        assert!(categories.contains(&AppCategory::Terminal));
    }
}
