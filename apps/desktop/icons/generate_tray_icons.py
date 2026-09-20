#!/usr/bin/env python3
"""生成菜单栏（托盘）图标 —— 三个设计方向的候选。

## 为什么菜单栏图标必须用代码画，不能用 AI 生图

macOS 菜单栏图标有三个硬性技术要求，AI 生图一个都满足不了：

1. **必须是纯单色 + 透明背景**。系统会把它当作「模板图像」（Template
   Image）自动反色 —— 浅色模式下显示为黑色，深色模式下显示为白色。
   任何彩色或渐变都会让这个机制失效，图标在深色模式下会变成一团糊。
2. **必须在 22pt 下清晰**。这是菜单栏的实际尺寸。这个尺寸下只有
   「形状」是有效的表达手段，细节、纹理、光影全部消失。
3. **笔画必须像素级精确对齐**。AI 生图的边缘总是带抗锯齿噪点，
   在 22pt 下会呈现为一圈灰边，看起来「脏」。

## 三个单位，千万别搞混（我第一版就栽在这里）

菜单栏显示尺寸是 **22pt**。Retina（@2x）屏需要 **44px** 的成品图。
而为了得到平滑的边缘，我们要在 4 倍大的画布上绘制再缩小。

于是存在三个不同的单位：

| 单位 | 含义 | 换算 |
| --- | --- | --- |
| `pt` | 设计尺寸，用户在菜单栏看到的 | 1pt = 2px |
| `px` | 44px 成品图里的像素 | 1px = 4u |
| `u`  | 超采样画布上的单位 | 1u = 1/8 pt |

第一版我把「超采样倍数 4」直接当成了「pt → u 的换算」，结果所有笔画
只有应有粗细的一半 —— 图标在菜单栏里细得像头发丝。所以下面统一用
[`u()`] 从 pt 换算，不再手写乘数。

用法：
    python3 generate_tray_icons.py
"""

from __future__ import annotations

import math
from pathlib import Path

from PIL import Image, ImageDraw

# ── 尺寸体系 ────────────────────────────────────────────────

#: 成品图的像素尺寸（对应 22pt 的菜单栏显示尺寸，@2x）。
ASSET_PX = 44

#: 1pt 等于多少成品像素（Retina 2 倍）。
PX_PER_PT = ASSET_PX // 22

#: 超采样倍数 —— 只影响边缘平滑度，不参与设计尺寸换算。
SUPERSAMPLE = 4

#: 超采样画布的边长。
CANVAS = ASSET_PX * SUPERSAMPLE


def u(pt: float) -> int:
    """把设计尺寸（pt）换算成超采样画布单位。"""
    return int(round(pt * PX_PER_PT * SUPERSAMPLE))


OUT_DIR = Path(__file__).parent

# 绘制用黑色。菜单栏模板图像只用 alpha 通道，RGB 统一为黑即可 ——
# 系统渲染时会忽略颜色，只取形状。
INK = (0, 0, 0, 255)

# ── 设计常量 ────────────────────────────────────────────────
#
# 菜单栏图标的通行尺度：图案本身占 16~18pt，笔画 1.6~2.2pt。
# 细于 1.5pt 在普通屏幕上会开始发虚，粗于 2.5pt 则显得笨重，
# 会在一排系统图标里「跳出来」—— 而我们的产品哲学恰恰是不打扰。

MARK_WIDTH_PT = 18.0  # 图案整体宽度
STROKE_PT = 2.0  # 笔画粗细


def new_canvas() -> tuple[Image.Image, ImageDraw.ImageDraw]:
    """创建一块透明画布（超采样尺寸）。"""
    img = Image.new("RGBA", (CANVAS, CANVAS), (0, 0, 0, 0))
    return img, ImageDraw.Draw(img)


def downsample(img: Image.Image) -> Image.Image:
    """降采样到成品尺寸。

    为什么用 LANCZOS：它在缩小时能更好地保留边缘锐利度。
    超采样的原理是「每个成品像素由 16 个画布像素平均而来」，
    平均的过程天然产生平滑的过渡，比直接开抗锯齿更可控。
    """
    return img.resize((ASSET_PX, ASSET_PX), Image.LANCZOS)


def bar(
    d: ImageDraw.ImageDraw,
    cx: float,
    cy: float,
    width_pt: float,
    stroke_pt: float,
) -> None:
    """在 (cx, cy) 处画一根两端圆头的水平横杠（尺寸都是 pt）。"""
    w = u(width_pt) / 2
    h = u(stroke_pt) / 2
    d.rounded_rectangle(
        [cx - w, cy - h, cx + w, cy + h],
        radius=u(stroke_pt) / 2,
        fill=INK,
    )


def draw_broken_line() -> Image.Image:
    """方向 A：一条被断口切开的横线。

    概念：一个人的工作流是一条连续的线，中间那个断口就是
    「该停下来休息」的时刻。断口偏右放置 —— 居中的断口看起来
    像是「加载中」，偏一点才像是被设计的。
    """
    img, d = new_canvas()

    total_w = u(MARK_WIDTH_PT)
    # 断线方向天生比环形/实心矩形「轻」，所以单独加重一点，
    # 让三个候选在菜单栏里的视觉体重接近 —— 否则它会显得凭空消失了。
    stroke = u(STROKE_PT + 0.4)
    gap = u(2.2)  # 断口宽度

    cx, cy = CANVAS / 2, CANVAS / 2
    left = cx - total_w / 2
    right = cx + total_w / 2

    # 断口中心放在整体宽度的 62% 处 —— 偏一点才像是被设计的
    gap_cx = left + total_w * 0.62

    half_h = stroke / 2
    d.rounded_rectangle(
        [left, cy - half_h, gap_cx - gap / 2, cy + half_h],
        radius=half_h,
        fill=INK,
    )
    d.rounded_rectangle(
        [gap_cx + gap / 2, cy - half_h, right, cy + half_h],
        radius=half_h,
        fill=INK,
    )

    return downsample(img)


def draw_open_ring() -> Image.Image:
    """方向 B：一个留了缺口的圆环。

    概念：工作与休息的循环，缺口就是那次呼吸。

    ## 一个很容易踩的坑

    `ImageDraw.arc(start, end)` 的参数是**要画出来的那一段角度**，
    不是「留出缺口的角度」。我第一版写成 `start=-20, end=20`，
    本意是「-20° 到 20° 之间留空」，实际却只画出了右边那 40° 的
    一小条弧，其余 320° 全丢了。

    正确写法是反过来：`start=20, end=340`，画出 320° 的环，
    缺口自然留在右边。

    另一个约定：Pillow 的角度**从 3 点钟方向起算、顺时针为正**
    （因为屏幕坐标系 y 轴朝下），所以算端点坐标时 sin 是加不是减。
    """
    img, d = new_canvas()

    diameter = u(MARK_WIDTH_PT)
    stroke = u(STROKE_PT)
    margin = (CANVAS - diameter) / 2

    # Pillow 的 arc 是【向内】画宽度的，所以包围盒就是外径
    box = [margin, margin, margin + diameter, margin + diameter]

    # 缺口：跨过 3 点钟方向、上下各 18°，共 36°
    gap_half = 18
    d.arc(box, start=gap_half, end=360 - gap_half, fill=INK, width=stroke)

    # arc 两端是平口，补圆头让它柔和、也显得更厚实
    cx = cy = margin + diameter / 2
    r = diameter / 2 - stroke / 2  # 笔画中心线的半径
    for angle_deg in (gap_half, 360 - gap_half):
        a = math.radians(angle_deg)
        ex = cx + r * math.cos(a)
        ey = cy + r * math.sin(a)
        rr = stroke / 2
        d.ellipse([ex - rr, ey - rr, ex + rr, ey + rr], fill=INK)

    return downsample(img)


def draw_rest_sign() -> Image.Image:
    """方向 C：乐谱的休止符号 —— 长杠在上、短杠在下。

    概念：这就是 "tacet" 的字面含义。产品名、产品哲学、以及
    「该停下来了」这个动作，在这个符号里是同一件事。
    """
    img, d = new_canvas()

    stroke = u(2.2)
    gap = u(2.0)

    cx = CANVAS / 2
    cy = CANVAS / 2

    # 两杠相对整体中心上下对称
    offset = (stroke + gap) / 2
    bar(d, cx, cy - offset, 18.0, 2.2)  # 长杠
    bar(d, cx, cy + offset, 12.0, 2.2)  # 短杠，居中

    return downsample(img)


DIRECTIONS = {
    "a-broken-line": draw_broken_line,
    "b-open-ring": draw_open_ring,
    "c-rest-sign": draw_rest_sign,
}


def main() -> None:
    for name, draw in DIRECTIONS.items():
        icon = draw()
        out = OUT_DIR / f"tray-{name}.png"
        icon.save(out)
        print(f"已生成 {out.name}  ({icon.width}×{icon.height}px = 22pt @2x)")


if __name__ == "__main__":
    main()
