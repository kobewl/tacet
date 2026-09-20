#!/usr/bin/env python3
"""检查 plist 文件的 XML 注释里有没有连续减号（`--`）。

## 为什么需要这个检查

XML 规范**不允许注释内容包含 `--`**，而 Apple 的签名工具 AMFI 严格照此执行。
真实踩过的坑：`entitlements.plist` 的注释里用 Markdown 表格写过
`| --- | --- |`，结果是「本地构建一直成功，一签名就报 AMFIUnserializeXML」。

更阴险的是 `plutil -lint` 会说这个文件 OK（它比较宽容），
所以本地怎么查都查不出来。只能靠这条专门的检查。

## 为什么是一个独立文件而不是内联在 CI 里

内联版本是 `python3 -c "..."` 写在 workflow 的 `run: |` 块里的。
那有个隐蔽的陷阱：**YAML 的块标量要求所有行都有缩进**，而 Python 的
多行代码一旦顶格写，YAML 就会把它当成新的顶层键，整个 workflow
在解析阶段就失败 —— 表现为「CI 0 秒失败、日志为空」，非常难查。

抽成独立文件后这个问题从根上消失：文本文件不受 YAML 缩进规则约束。

## 用法

    python3 check-plist-comments.py path/to/file.plist

退出码 0 表示通过，非 0 表示有问题（错误信息打到 stderr）。
"""

import re
import sys

# 匹配 XML 注释块。`re.S` 让 `.` 能跨行 —— plist 的注释经常是多行的。
COMMENT_PATTERN = re.compile(r"<!--.*?-->", re.S)


def find_bad_comments(text: str) -> list[str]:
    """返回所有不合格的注释片段（去掉 `<!--` 和 `-->` 之后仍含 `--` 的）。"""
    bad = []
    for match in COMMENT_PATTERN.finditer(text):
        inner = match.group(0)[4:-3]  # 剥掉 <!-- 和 -->
        if "--" in inner:
            bad.append(match.group(0)[:60])
    return bad


def main() -> int:
    if len(sys.argv) != 2:
        print(f"用法：{sys.argv[0]} <plist 文件>", file=sys.stderr)
        return 2

    path = sys.argv[1]
    try:
        text = open(path, encoding="utf-8").read()
    except OSError as err:
        print(f"读取失败：{err}", file=sys.stderr)
        return 2

    bad = find_bad_comments(text)
    if not bad:
        return 0

    print(f"❌ {path} 的 XML 注释里有连续减号（'--'）：", file=sys.stderr)
    for snippet in bad:
        print(f"     {snippet!r}", file=sys.stderr)
    print("", file=sys.stderr)
    print(
        "   XML 规范禁止注释内容包含 '--'。签名时会报 AMFIUnserializeXML 错误。\n"
        "   如果注释里要画分隔线，用 '—'（em dash）或改成文字描述。",
        file=sys.stderr,
    )
    return 1


if __name__ == "__main__":
    sys.exit(main())
