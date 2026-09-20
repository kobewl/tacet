//! Tauri 的构建脚本。
//!
//! 它的主要工作是读取 `tauri.conf.json`、生成权限与上下文代码。
//! 我们额外加了一件事：**把版本号一致性检查塞进构建期**。
//!
//! 研发规范 §3.3 红线 3 要求「不得手写多份版本号，CI 校验 Cargo /
//! tauri.conf / package.json 一致」。放在构建脚本里意味着：
//! 一旦不一致，**本地编译就会失败** —— 而不是等到 CI 上才发现。

fn main() {
    check_version_consistency();
    tauri_build::build();
}

/// 校验三处版本号是否一致。
///
/// 为什么必须一致：关于页显示的版本来自 `package.json`，
/// 安装包名来自 `tauri.conf.json`，而代码里的版本来自 `Cargo.toml`。
/// 三者不一致时会出现「安装包叫 0.1.0，关于页写着 0.0.9」这种事，
/// 排查线上问题时极其误导。
fn check_version_consistency() {
    // Cargo.toml 的版本由 CARGO_PKG_VERSION 提供
    let cargo_version = std::env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION 应当存在");

    // tauri.conf.json
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR 应当存在");
    let conf_path = std::path::Path::new(&manifest_dir).join("tauri.conf.json");

    println!("cargo:rerun-if-changed={}", conf_path.display());

    let conf_text = match std::fs::read_to_string(&conf_path) {
        Ok(text) => text,
        Err(err) => {
            panic!("读不到 {}：{err}", conf_path.display());
        }
    };

    let conf_version =
        extract_json_string(&conf_text, "version").expect("tauri.conf.json 里应当有 version 字段");

    if conf_version != cargo_version {
        panic!(
            "版本号不一致：Cargo.toml 是 {cargo_version}，\
             tauri.conf.json 是 {conf_version}。\
             请同步后再构建（研发规范 §3.3 红线 3）。"
        );
    }

    // package.json（前端）
    let ui_pkg = std::path::Path::new(&manifest_dir).join("../../packages/ui/package.json");

    println!("cargo:rerun-if-changed={}", ui_pkg.display());

    if let Ok(text) = std::fs::read_to_string(&ui_pkg) {
        let ui_version =
            extract_json_string(&text, "version").expect("package.json 里应当有 version 字段");

        if ui_version != cargo_version {
            panic!(
                "版本号不一致：Cargo.toml 是 {cargo_version}，\
                 packages/ui/package.json 是 {ui_version}。\
                 请同步后再构建（研发规范 §3.3 红线 3）。"
            );
        }
    } else {
        // 前端目录不存在时不阻断（比如只想编译 Rust 部分的场景）
        println!("cargo:warning=找不到 packages/ui/package.json，跳过该处版本校验");
    }
}

/// 从 JSON 文本里粗取出一个顶层字符串字段的值。
///
/// ## 为什么不用 serde_json 来解析
///
/// 构建脚本的依赖越少越好 —— 每加一个 build-dependency，
/// 都会让整个项目的编译时间变长一点，而且一旦它的版本冲突，
/// 排查起来很麻烦。我们只需要取一个字符串，十几行代码就够了。
fn extract_json_string(text: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let start = text.find(&needle)?;

    // 从 key 之后找第一个冒号，再找第一个引号对
    let after_key = &text[start + needle.len()..];
    let colon = after_key.find(':')?;
    let after_colon = &after_key[colon + 1..];

    let quote_start = after_colon.find('"')?;
    let rest = &after_colon[quote_start + 1..];
    let quote_end = rest.find('"')?;

    Some(rest[..quote_end].to_string())
}
