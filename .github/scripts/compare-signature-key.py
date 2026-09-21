#!/usr/bin/env python3
"""校验更新包的签名与应用内置公钥是同一对密钥。

## 为什么需要这个检查

发布流程里最容易犯、也最难在事后发现的错，是 **Secret 里配错了私钥**
（比如把别的项目的密钥填进来了）。那种情况下：

- 构建完全正常；
- Release 发得出去；
- 附件也会有 `.sig` 和 `latest.json`。

一切看起来都好，只有当用户点「立即更新」时才会报「签名不匹配」。
而那时已经**没法悄悄补救了** —— 客户端只认最初发布时内置的那把公钥，
轮换公钥会让所有旧版本无法验证任何新版本。

所以这个检查的唯一目的，就是在发布的那一刻把这种错拦住。

## 比对原理

minisign 的签名和公钥里都带有同一个 8 字节 `key_id`
（用于验签时快速排除不匹配的密钥）。两者一致就说明签名确实出自
配置里那把公钥的配对私钥。

  签名材料：算法(2) + key_id(8) + 签名(64)
  公钥材料：算法(2) + key_id(8) + 公钥(32)

## 为什么是独立文件而不是内联在 workflow 里

与 `check-plist-comments.py` 同样的理由：YAML 的块标量要求所有行都有缩进，
而 Python 多行代码一旦顶格写，YAML 会把它当成新的顶层键，
整个 workflow 在**解析阶段**就失败 —— 表现是「CI 0 秒失败、日志空白」，
非常难查。这个坑本项目已经踩过一次。

## 用法

    python3 compare-signature-key.py <tauri.conf.json> <签名文件.sig> [更多 .sig...]

可以传多个 `.sig`（通用包会为每个架构各产一份），全部都会校验。
退出码 0 表示全部配对，1 表示有不配对的或文件有问题。
"""

import base64
import json
import sys


def minisign_keyid_from_signature(path: str) -> str:
    """从 .sig 文件里取出 key_id。

    .sig 文件**本身**是外层 base64（内容是完整的 minisign 文本格式），
    所以要解两次：外层 base64 → 文本 → 第二行 base64 → 二进制材料。
    """
    outer = open(path, encoding="utf-8").read().strip()

    try:
        inner = base64.b64decode(outer).decode("utf-8")
    except Exception as exc:
        raise ValueError(f"{path} 不是合法的 base64：{exc}") from exc

    # minisign 文本格式：注释行、签名行、trusted comment 行…
    lines = [line for line in inner.split("\n") if line]
    if len(lines) < 2:
        raise ValueError(f"{path} 的内容不是预期的 minisign 格式（行数不足）")

    material = base64.b64decode(lines[1])
    if len(material) < 10:
        raise ValueError(f"{path} 的签名材料过短（{len(material)} 字节）")

    return material[2:10].hex().upper()


def minisign_keyid_from_pubkey(config_path: str) -> str:
    """从 tauri.conf.json 的 plugins.updater.pubkey 里取出 key_id。"""
    config = json.load(open(config_path, encoding="utf-8"))

    try:
        pubkey = config["plugins"]["updater"]["pubkey"]
    except KeyError as exc:
        raise ValueError(
            f"{config_path} 里找不到 plugins.updater.pubkey：{exc}"
        ) from exc

    # 公钥同样是外层 base64（内容是 .pub 文件的文本），解两次
    inner = base64.b64decode(pubkey.strip()).decode("utf-8")
    lines = [line for line in inner.split("\n") if line]
    if len(lines) < 2:
        raise ValueError("公钥内容不是预期的 minisign 格式（行数不足）")

    material = base64.b64decode(lines[1])
    if len(material) < 10:
        raise ValueError(f"公钥材料过短（{len(material)} 字节）")

    return material[2:10].hex().upper()


def main() -> int:
    if len(sys.argv) < 3:
        print(f"用法: {sys.argv[0]} <tauri.conf.json> <签名文件.sig> [更多 .sig...]")
        return 2

    config_path, sig_paths = sys.argv[1], sys.argv[2:]

    try:
        pub_keyid = minisign_keyid_from_pubkey(config_path)
    except (ValueError, OSError) as exc:
        print(f"❌ {exc}")
        return 1

    print(f"应用内置公钥 key_id: {pub_keyid}")

    # 每一个签名都要校验：通用包会为两个架构各产一份 .sig，
    # 只查第一个的话，第二个有问题就漏过去了。
    bad: list[str] = []
    for path in sig_paths:
        try:
            sig_keyid = minisign_keyid_from_signature(path)
        except (ValueError, OSError) as exc:
            print(f"❌ {exc}")
            bad.append(path)
            continue

        name = path.rsplit("/", 1)[-1]
        if sig_keyid == pub_keyid:
            print(f"  ✅ {name}（key_id {sig_keyid}）")
        else:
            print(f"  ❌ {name}：key_id {sig_keyid} 与公钥不符")
            bad.append(path)

    if bad:
        print()
        print("❌ 有签名与应用内置公钥不配对 —— 这些更新包会因验签失败而装不上")
        print()
        print("   最可能的原因：GitHub Secret TAURI_SIGNING_PRIVATE_KEY 里配的")
        print("   不是本项目的私钥。请改用开发机上的 ~/.tauri/tacet-updater.key，")
        print("   然后重新发布一个补丁版本。")
        print()
        print("   注意：**不要轮换应用里的公钥**来迁就现有私钥 ——")
        print("   已经装了旧版的用户会因此无法验证任何新版本。")
        return 1

    print(f"✅ {len(sig_paths)} 个签名全部与内置公钥配对")
    return 0


if __name__ == "__main__":
    sys.exit(main())
