#!/usr/bin/env bash
# 切换应用图标 —— 三个候选之间一条命令的事。
#
# 用法：
#     ./switch_icon.sh midnight     # 深夜琥珀（当前默认）
#     ./switch_icon.sh lens         # 玻璃球
#     ./switch_icon.sh aurora       # 极光环
#
# ## 为什么需要这个脚本
#
# 三个候选各有取舍，而**真正的评判标准只有一个：装进 Dock 里好不好看**。
# 在图片浏览器里对比是没用的 —— 图标是在 Dock、Launchpad、
# 切换器（Cmd+Tab）这些地方被人看到的。
#
# ## 三个候选的实测差异（这是选型时最重要的依据）
#
# | 尺寸 | lens 玻璃球 | aurora 极光环 | midnight 深夜琥珀 |
# | --- | --- | --- | --- |
# | 128px（Dock） | 好看 | 好看 | 好看 |
# | 64px | 好 | 好 | 好 |
# | 16px | **变成「禁止」符号** | 变成 C / 转圈 | **糊成一团** |
#
# - `lens` 在 16px 下是「圆圈 + 横杠」，而这是国际通行的**禁止符号**，
#   对健康产品来说是**语义错误**，所以它不能做默认。
# - `midnight` 在 16px 下会糊（深色底 + 细小发光元素缩小后糊在一起），
#   但 16px 只在 Finder 列表视图和「显示简介」里出现，影响面小。
# - `aurora` 缩放性最好，但小尺寸下容易被认成「加载中」。
#
# 当前默认选 `midnight`：它的概念最强（就是乐谱的休止符，
# 与产品名同源），而它退化的那个尺寸恰恰是用户最少看到的那个。

set -euo pipefail

DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
NAME="${1:-}"

if [[ -z "$NAME" ]]; then
  echo "用法：$0 <midnight|lens|aurora>"
  echo
  echo "可用候选："
  for f in "$DIR"/icon-*.icns; do
    echo "  - $(basename "$f" .icns | sed 's/^icon-//')"
  done
  exit 1
fi

SRC="$DIR/icon-${NAME}.icns"
if [[ ! -f "$SRC" ]]; then
  echo "❌ 找不到候选：$NAME" >&2
  exit 1
fi

# .icns 直接覆盖；icon.png 从同一份源图重新导出，
# 保证两处图标永远来自同一个设计，不会出现「.app 是新图标、
# 关于页是旧图标」这种不一致。
cp "$SRC" "$DIR/icon.icns"

python3 - "$DIR" "$SRC" <<'PY'
import sys
from pathlib import Path
from PIL import Image

icon_dir = Path(sys.argv[1])
src = sys.argv[2]

im = Image.open(src).convert("RGBA")
im.resize((512, 512), Image.LANCZOS).save(icon_dir / "icon.png")
print(f"已切换为 {Path(src).stem}：icon.icns + icon.png (512px)")
PY

echo
echo "重新构建后才生效："
echo "  cd apps/desktop && ./node_modules/.bin/tauri build --bundles app"
