#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""手写 Tacet v0.1 原型（8 个界面），输出独立 HTML 页面 + 预览 + 导入用 payload。

每一页都是手工排版：位置、间距、颜色、图标全部显式写出，不含任何生成式内容。
"""
import json
import os

OUT = os.path.dirname(os.path.abspath(__file__))

# ---------------------------------------------------------------- 图标（内联 SVG）
IC_NOTE = '<svg viewBox="0 0 18 18" fill="none"><path d="M7.1 12.4V3.6l6.1-1.2v8.5" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/><circle cx="5.3" cy="12.7" r="1.9" stroke="currentColor" stroke-width="1.6"/><circle cx="11.4" cy="11" r="1.9" stroke="currentColor" stroke-width="1.6"/></svg>'
IC_CUP = '<svg viewBox="0 0 18 18" fill="none"><path d="M3.1 4.9h8.6v6a3.2 3.2 0 0 1-3.2 3.2H6.3A3.2 3.2 0 0 1 3.1 10.9V4.9Z" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round"/><path d="M11.7 6.3h1.3a1.9 1.9 0 0 1 0 3.8h-1.3" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/><path d="M6.2 2.4v1.2M8.6 2.1v1.5" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/></svg>'
IC_DROP = '<svg viewBox="0 0 18 18" fill="none"><path d="M9 2.4c2.5 2.9 4.1 5.1 4.1 7a4.1 4.1 0 1 1-8.2 0c0-1.9 1.6-4.1 4.1-7Z" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round"/></svg>'
IC_WALK = '<svg viewBox="0 0 18 18" fill="none"><circle cx="10.3" cy="3.5" r="1.6" stroke="currentColor" stroke-width="1.5"/><path d="M10.7 6.2 8.1 8.4l1.3 2.2-.8 4.5M10.7 6.2l1.9 2.6 2.1.7M9.4 10.6 5.9 11.1M12.6 8.8l.9 6.1" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/></svg>'
IC_EYE = '<svg viewBox="0 0 18 18" fill="none"><path d="M1.9 9S4.5 4.4 9 4.4 16.1 9 16.1 9 13.5 13.6 9 13.6 1.9 9 1.9 9Z" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round"/><circle cx="9" cy="9" r="2.1" stroke="currentColor" stroke-width="1.5"/></svg>'
IC_CHECK = '<svg viewBox="0 0 18 18" fill="none"><path d="M3.6 9.3l3.3 3.3 6.1-7.2" stroke="currentColor" stroke-width="1.9" stroke-linecap="round" stroke-linejoin="round"/></svg>'
IC_CHEV = '<svg viewBox="0 0 18 18" fill="none"><path d="M7 4.5 11.5 9 7 13.5" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"/></svg>'
IC_WIFI = '<svg viewBox="0 0 18 18" fill="none"><path d="M2.5 6.6a9.6 9.6 0 0 1 13 0M5 9.3a6 6 0 0 1 8 0M7.4 11.9a2.4 2.4 0 0 1 3.2 0" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/><circle cx="9" cy="14" r=".9" fill="currentColor"/></svg>'
IC_BATT = '<svg viewBox="0 0 24 18" fill="none"><rect x="2.2" y="5.2" width="17" height="7.6" rx="2.4" stroke="currentColor" stroke-width="1.4" opacity=".55"/><rect x="3.9" y="6.9" width="12.4" height="4.2" rx="1.3" fill="currentColor"/><path d="M21 8.2v1.6c.9-.3 1.4-.5 1.4-.8s-.5-.5-1.4-.8Z" fill="currentColor" opacity=".55"/></svg>'
IC_SEARCH = '<svg viewBox="0 0 18 18" fill="none"><circle cx="8.1" cy="8.1" r="4.9" stroke="currentColor" stroke-width="1.6"/><path d="M11.8 11.8 15.4 15.4" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/></svg>'
IC_CTRL = '<svg viewBox="0 0 18 18" fill="none"><rect x="2.2" y="4.4" width="13.6" height="9.2" rx="2.6" stroke="currentColor" stroke-width="1.4"/><path d="M6.6 9h4.8" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/></svg>'
IC_APPLE = '<svg viewBox="0 0 16 16" fill="currentColor"><path d="M11.1 8.4c0-1.6 1.3-2.4 1.4-2.4-.8-1.1-1.9-1.3-2.3-1.3-1-.1-1.9.6-2.4.6-.5 0-1.3-.6-2.1-.6-1.1 0-2.1.6-2.7 1.6-1.1 2-.3 5 .8 6.6.5.8 1.2 1.6 2 1.6.8 0 1.1-.5 2.1-.5s1.2.5 2.1.5c.9 0 1.4-.8 1.9-1.5.6-.9.9-1.7.9-1.8-.1 0-1.7-.7-1.7-2.4ZM9.6 3.6c.4-.5.7-1.2.6-1.9-.6 0-1.4.4-1.8.9-.4.5-.8 1.2-.7 1.9.7.1 1.4-.4 1.9-.9Z"/></svg>'


def shell(title, cap_no, cap_name, body, w=1280, h=800, extra_css="", full_bleed=False, ver="v0.1 Foundation"):
    """把一页内容包装成独立文档。"""
    cap = f'''
  <div class="cap">
    <div class="t"><em>{cap_no}</em>{cap_name}</div>
    <div class="v">{ver}</div>
  </div>''' if not full_bleed else ''
    stage_cls = "stage-bleed" if full_bleed else "stage"
    return f'''<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<title>{title}</title>
<style>
{BASE_CSS}
{extra_css}
</style>
</head>
<body>
<div class="page" style="width:{w}px;height:{h}px">
{cap}
  <div class="{stage_cls}">
{body}
  </div>
</div>
</body>
</html>
'''


BASE_CSS = '''
*{margin:0;padding:0;box-sizing:border-box}
body{background:#F5F6F7;font-family:-apple-system,BlinkMacSystemFont,"SF Pro Text","PingFang SC","Helvetica Neue",Arial,sans-serif;color:#1F2329;-webkit-font-smoothing:antialiased;font-size:14px}
.page{position:relative;display:flex;flex-direction:column;gap:20px;padding:34px 44px 30px;overflow:hidden}
.cap{display:flex;align-items:baseline;justify-content:space-between;flex:0 0 auto}
.cap .t{font-size:14px;font-weight:600;color:#3C444C;letter-spacing:.01em}
.cap .t em{font-style:normal;color:#A8AEB5;font-weight:600;margin-right:9px;font-variant-numeric:tabular-nums}
.cap .v{font-size:11.5px;color:#A8AEB5;letter-spacing:.06em}
.stage{flex:1;display:flex;align-items:center;justify-content:center;gap:26px}
.stage-col{flex:1;display:flex;flex-direction:column;align-items:center;justify-content:center;gap:18px}
.stage-bleed{flex:1;display:flex;align-items:center;justify-content:center;margin:-34px -44px -30px;border-radius:0}
.num{font-variant-numeric:tabular-nums;font-feature-settings:"tnum" 1}

/* ---------- 通用细节 ---------- */
.tile{width:34px;height:34px;border-radius:10px;display:flex;align-items:center;justify-content:center;flex:0 0 auto}
.tile svg{width:18px;height:18px}
.t-teal{background:#E7F5F3;color:#2FA8A0}
.t-water{background:#E9F2FB;color:#4A9FE0}
.t-move{background:#FDF1E6;color:#DE8B41}
.t-eye{background:#F0EDFB;color:#8B7BD8}
.t-ok{background:#E9F6EF;color:#4CAF7D}
.t-gray{background:#EFF1F3;color:#7C858E}
.sub{font-size:12px;color:#98A0A8}
.ok{color:#4CAF7D}

/* 开关 */
.sw{width:40px;height:24px;border-radius:12px;background:#D5DADF;position:relative;flex:0 0 auto;transition:.2s}
.sw i{position:absolute;top:2px;left:2px;width:20px;height:20px;border-radius:50%;background:#fff;box-shadow:0 1px 3px rgba(0,0,0,.18);transition:.2s}
.sw.on{background:#2FA8A0}
.sw.on i{left:18px}

/* 按钮 */
.btn{border:none;border-radius:10px;font-size:13.5px;font-weight:500;font-family:inherit;cursor:default;display:inline-flex;align-items:center;justify-content:center;gap:6px}
.btn-primary{background:#2FA8A0;color:#fff;height:38px;padding:0 18px;box-shadow:0 2px 8px rgba(47,168,160,.28)}
.btn-ghost{background:#fff;color:#3C444C;height:38px;padding:0 16px;border:1px solid #E1E5E9}
.btn-quiet{background:#F2F4F5;color:#5A636C;height:34px;padding:0 14px}
.btn-plain{background:transparent;color:#8B939B;font-size:13px;height:30px;padding:0 6px}

/* 窗口 / 面板 */
.win{background:#fff;border-radius:16px;box-shadow:0 22px 50px rgba(16,24,32,.14),0 2px 8px rgba(16,24,32,.06);border:1px solid rgba(20,28,36,.05);overflow:hidden;position:relative}
.tl{display:flex;gap:8px;align-items:center}
.tl i{width:12px;height:12px;border-radius:50%;display:block}
.tl .r{background:#FF5F57}.tl .y{background:#FEBC2E}.tl .g{background:#28C840}

/* 分组卡片 */
.group-title{font-size:12.5px;font-weight:600;color:#8B939B;margin:0 0 8px 4px}
.card{background:#fff;border-radius:12px;border:1px solid #ECEEF0;overflow:hidden}
.row{display:flex;align-items:center;gap:12px;padding:13px 16px;min-height:56px}
.row + .row{border-top:1px solid #F0F2F4}
.row .lab{font-size:13.5px;color:#262C33;flex:1}
.row .ctl{display:flex;align-items:center;gap:14px}
.pill{display:flex;align-items:center;gap:4px;background:#F4F6F7;border:1px solid #E7EAED;border-radius:8px;height:30px;padding:0 9px;font-size:13px;color:#3C444C;font-variant-numeric:tabular-nums}
.pill span{color:#98A0A8;font-size:12px;margin-left:1px}
'''

# ============================================================ 01 菜单栏下拉主面板
P1 = shell("01 菜单栏下拉主面板", "01", "菜单栏下拉主面板", '''
    <div style="display:flex;flex-direction:column;align-items:center;gap:0">
      <div style="width:18px;height:9px;position:relative">
        <div style="position:absolute;inset:0;background:#fff;clip-path:polygon(50% 0,100% 100%,0 100%);filter:drop-shadow(0 -1px 1px rgba(16,24,32,.05))"></div>
      </div>
      <div class="win" style="width:364px;border-radius:0 0 16px 16px;border-radius:14px">
        <!-- 状态头 -->
        <div style="padding:16px 18px 14px;display:flex;align-items:center;justify-content:space-between">
          <div style="display:flex;align-items:center;gap:9px">
            <div class="tile t-teal" style="width:28px;height:28px;border-radius:9px">''' + IC_NOTE + '''</div>
            <div style="font-size:14.5px;font-weight:600;letter-spacing:.01em">Tacet</div>
          </div>
          <div style="text-align:right;line-height:1.15">
            <div style="font-size:10.5px;color:#A8AEB5;letter-spacing:.04em">连续工作</div>
            <div class="num" style="font-size:19px;font-weight:600;color:#2FA8A0;letter-spacing:.01em">1h 32m</div>
          </div>
        </div>
        <div style="height:1px;background:#F0F2F4"></div>

        <!-- 四类健康卡 -->
        <div style="padding:14px;display:grid;grid-template-columns:1fr 1fr;gap:10px">
          <div style="background:#FAFBFB;border:1px solid #EEF1F2;border-radius:12px;padding:11px 12px;display:flex;align-items:center;gap:9px">
            <div class="tile t-teal" style="width:30px;height:30px;border-radius:9px">''' + IC_CUP + '''</div>
            <div style="line-height:1.3;min-width:0">
              <div style="font-size:13px;font-weight:500">休息</div>
              <div class="sub num">距提醒 18 分钟</div>
            </div>
          </div>
          <div style="background:#FAFBFB;border:1px solid #EEF1F2;border-radius:12px;padding:11px 12px;display:flex;align-items:center;gap:9px">
            <div class="tile t-water" style="width:30px;height:30px;border-radius:9px">''' + IC_DROP + '''</div>
            <div style="line-height:1.3;min-width:0">
              <div style="font-size:13px;font-weight:500">喝水</div>
              <div class="sub num">距提醒 12 分钟</div>
            </div>
          </div>
          <div style="background:#FAFBFB;border:1px solid #EEF1F2;border-radius:12px;padding:11px 12px;display:flex;align-items:center;gap:9px">
            <div class="tile t-move" style="width:30px;height:30px;border-radius:9px">''' + IC_WALK + '''</div>
            <div style="line-height:1.3;min-width:0">
              <div style="font-size:13px;font-weight:500">活动</div>
              <div class="sub num">距提醒 42 分钟</div>
            </div>
          </div>
          <div style="background:#F7FBF9;border:1px solid #E6F2EB;border-radius:12px;padding:11px 12px;display:flex;align-items:center;gap:9px">
            <div class="tile t-ok" style="width:30px;height:30px;border-radius:9px">''' + IC_EYE + '''</div>
            <div style="line-height:1.3;min-width:0">
              <div style="font-size:13px;font-weight:500">护眼</div>
              <div class="sub num ok">今日已达标</div>
            </div>
          </div>
        </div>

        <!-- 快捷操作 -->
        <div style="padding:2px 14px 14px;display:flex;flex-direction:column;gap:9px">
          <button class="btn btn-primary" style="width:100%;height:40px;font-size:14px">现在休息</button>
          <div style="display:flex;gap:9px">
            <button class="btn btn-ghost" style="flex:1">+1 杯水</button>
            <button class="btn btn-ghost" style="flex:1">活动打卡</button>
          </div>
        </div>

        <div style="height:1px;background:#F0F2F4"></div>
        <div style="padding:11px 18px 14px;display:flex;align-items:center;gap:10px;font-size:12.5px;color:#8B939B">
          <span>设置</span><span style="color:#D8DDE1">·</span>
          <span>暂停计时</span><span style="color:#D8DDE1">·</span>
          <span>退出</span>
        </div>
      </div>
    </div>
''', w=520, h=540)

# ============================================================ 02 菜单栏状态变体
def mbar(tacet_inner, tacet_style="", extra=""):
    return f'''<div style="width:1000px;height:30px;background:rgba(255,255,255,.86);backdrop-filter:blur(20px);border:1px solid rgba(20,28,36,.06);border-radius:8px;display:flex;align-items:center;justify-content:space-between;padding:0 12px;box-shadow:0 2px 10px rgba(16,24,32,.05)">
      <div style="display:flex;align-items:center;gap:14px;font-size:13px;color:#2A2F35">
        <span style="display:flex;opacity:.85">{IC_APPLE}</span>
        <b style="font-weight:600">Finder</b>
        <span style="color:#3C444C">文件</span><span style="color:#3C444C">编辑</span><span style="color:#3C444C">显示</span><span style="color:#3C444C">前往</span><span style="color:#3C444C">窗口</span><span style="color:#3C444C">帮助</span>
      </div>
      <div style="display:flex;align-items:center;gap:13px;color:#3C444C">
        <span style="display:flex;opacity:.8">{IC_CTRL}</span>
        <span style="display:flex;opacity:.8">{IC_WIFI}</span>
        <span style="display:flex;opacity:.8;width:24px">{IC_BATT}</span>
        <span style="display:flex;opacity:.8">{IC_SEARCH}</span>
        <span style="width:1px;height:14px;background:#DFE3E6"></span>
        <span style="display:flex;align-items:center;gap:6px;font-size:12.5px;{tacet_style}">{tacet_inner}</span>
      </div>
    </div>{extra}'''

P2 = shell("02 菜单栏状态", "02", "菜单栏状态（Level 1 · 环境级提示）", f'''
    <div style="display:flex;flex-direction:column;gap:13px">
      {mbar('<span style="display:flex;width:15px;color:#8B939B">' + IC_NOTE + '</span>', '',
            '<div class="sub" style="margin:7px 2px 0;font-size:11.5px">静默 —— 需求低或刚休息完，只保留一个安静的小图标</div>')}
      {mbar('<span style="display:flex;width:15px;color:#2FA8A0">' + IC_NOTE + '</span><span class="num" style="font-weight:600">1h 32m</span>', 'color:#2FA8A0',
            '<div class="sub" style="margin:7px 2px 0;font-size:11.5px">工作中 —— 显示连续工作计时，不发声、不弹窗</div>')}
      {mbar('<span style="display:flex;width:15px;color:#4A9FE0">' + IC_DROP + '</span><span class="num" style="font-weight:600">12m</span>', 'color:#4A9FE0',
            '<div class="sub" style="margin:7px 2px 0;font-size:11.5px">喝水临近 —— 用蓝色水滴把「快要提醒了」这件事提前放出来</div>')}
      {mbar('<span style="display:flex;width:15px;color:#B9BFC5">' + IC_NOTE + '</span><span style="color:#B9BFC5">勿扰</span>', 'color:#B9BFC5',
            '<div class="sub" style="margin:7px 2px 0;font-size:11.5px">勿扰 —— 用户主动开启，图标整体变淡，期间不做任何提示</div>')}
    </div>
''', w=1120, h=440)

# ============================================================ 03 系统通知
def notif(tile_cls, icon, title, body, acts):
    btn = ""
    for i, (txt, primary) in enumerate(acts):
        if primary:
            btn += f'<button class="btn btn-primary" style="height:32px;font-size:13px;flex:1;box-shadow:none">{txt}</button>'
        else:
            btn += f'<button class="btn btn-quiet" style="height:32px;font-size:13px;flex:1">{txt}</button>'
    return f'''<div style="width:404px;background:rgba(255,255,255,.9);backdrop-filter:blur(24px);border:1px solid rgba(20,28,36,.07);border-radius:18px;box-shadow:0 16px 36px rgba(16,24,32,.13);padding:15px 16px 14px">
      <div style="display:flex;align-items:center;gap:10px;margin-bottom:11px">
        <div class="tile {tile_cls}" style="width:30px;height:30px;border-radius:8px">{icon}</div>
        <div style="font-size:12.5px;font-weight:600;color:#5A636C;letter-spacing:.02em">Tacet</div>
        <div style="flex:1"></div>
        <div class="sub" style="font-size:11.5px">现在</div>
      </div>
      <div style="font-size:14.5px;font-weight:600;margin-bottom:4px">{title}</div>
      <div style="font-size:13px;color:#6B737B;line-height:1.5;margin-bottom:13px">{body}</div>
      <div style="display:flex;gap:9px">{btn}</div>
    </div>'''

P3 = shell("03 系统通知", "03", "系统通知（Level 2 · 不宜强打断时）", f'''
    <div style="display:flex;flex-direction:column;align-items:center;gap:26px">
      <div style="display:flex;flex-direction:column;align-items:center;gap:12px">
        <div class="sub" style="font-size:11.5px;letter-spacing:.04em">休息场景 · 通知内直接给出两个出口</div>
        {notif("t-teal", IC_CUP, "建议休息一下", "你已经连续工作 78 分钟了。", [("现在休息", True), ("3 分钟后", False)])}
      </div>
      <div style="display:flex;flex-direction:column;align-items:center;gap:12px">
        <div class="sub" style="font-size:11.5px;letter-spacing:.04em">喝水场景 · 避免「必须现在做」的压迫感</div>
        {notif("t-water", IC_DROP, "如果方便，记得喝点水", "距离上次喝水已经 45 分钟了。", [("已喝水", True), ("稍后", False)])}
      </div>
    </div>
''', w=560, h=600)

# ============================================================ 04 全屏休息提醒
P4 = shell("04 全屏休息提醒", "04", "全屏休息提醒（Level 4 · 可打断度高时）", '''
    <div style="width:100%;height:100%;background:radial-gradient(1100px 620px at 50% 30%,#26343D 0%,#131C23 58%,#0C1217 100%);display:flex;flex-direction:column;align-items:center;justify-content:center;position:relative;overflow:hidden">
      <!-- 背景层次 -->
      <div style="position:absolute;inset:0;background:radial-gradient(700px 400px at 22% 78%,rgba(47,168,160,.13),transparent 70%)"></div>
      <div style="position:absolute;inset:0;background:radial-gradient(600px 340px at 82% 18%,rgba(74,159,224,.10),transparent 70%)"></div>

      <div style="position:relative;display:flex;flex-direction:column;align-items:center">
        <div style="display:flex;align-items:center;gap:9px;margin-bottom:26px">
          <span style="display:flex;width:16px;color:rgba(255,255,255,.55)">''' + IC_NOTE + '''</span>
          <span style="font-size:12.5px;letter-spacing:.22em;color:rgba(255,255,255,.5)">TACET</span>
        </div>

        <div style="font-size:27px;font-weight:500;color:#F2F6F8;letter-spacing:.02em;margin-bottom:22px">建议休息一下</div>

        <div class="num" style="font-size:88px;font-weight:200;color:#fff;letter-spacing:.01em;line-height:1;margin-bottom:34px">05:00</div>

        <div style="width:520px;height:1px;background:linear-gradient(90deg,transparent,rgba(255,255,255,.22),transparent);margin-bottom:28px"></div>

        <div style="width:430px">
          <div style="font-size:12px;letter-spacing:.1em;color:rgba(255,255,255,.42);margin-bottom:15px;text-align:center">为什么现在提醒我</div>
          <div style="display:flex;flex-direction:column;gap:12px">
            <div style="display:flex;align-items:center;gap:11px;font-size:14px;color:#D5DDE3">
              <span style="width:19px;height:19px;border-radius:50%;background:rgba(47,168,160,.22);color:#5ED3C9;display:flex;align-items:center;justify-content:center;flex:0 0 auto">''' + IC_CHECK + '''</span>
              已连续工作 78 分钟
            </div>
            <div style="display:flex;align-items:center;gap:11px;font-size:14px;color:#D5DDE3">
              <span style="width:19px;height:19px;border-radius:50%;background:rgba(47,168,160,.22);color:#5ED3C9;display:flex;align-items:center;justify-content:center;flex:0 0 auto">''' + IC_CHECK + '''</span>
              未检测到会议
            </div>
            <div style="display:flex;align-items:center;gap:11px;font-size:14px;color:#D5DDE3">
              <span style="width:19px;height:19px;border-radius:50%;background:rgba(47,168,160,.22);color:#5ED3C9;display:flex;align-items:center;justify-content:center;flex:0 0 auto">''' + IC_CHECK + '''</span>
              上次休息是 2 小时前
            </div>
          </div>
        </div>

        <div style="display:flex;align-items:center;gap:11px;margin-top:38px">
          <button class="btn" style="height:42px;padding:0 30px;font-size:14.5px;background:#2FA8A0;color:#fff;box-shadow:0 6px 20px rgba(47,168,160,.35)">现在休息</button>
          <button class="btn" style="height:42px;padding:0 20px;font-size:13.5px;background:rgba(255,255,255,.09);color:#DDE4E9;border:1px solid rgba(255,255,255,.16)">延后 1 分钟</button>
          <button class="btn" style="height:42px;padding:0 20px;font-size:13.5px;background:rgba(255,255,255,.09);color:#DDE4E9;border:1px solid rgba(255,255,255,.16)">3 分钟</button>
          <button class="btn" style="height:42px;padding:0 20px;font-size:13.5px;background:rgba(255,255,255,.09);color:#DDE4E9;border:1px solid rgba(255,255,255,.16)">5 分钟</button>
        </div>

        <div style="margin-top:20px;font-size:12.5px;color:rgba(255,255,255,.38)">跳过这次提醒</div>
        <div style="margin-top:30px;font-size:11.5px;color:rgba(255,255,255,.22);letter-spacing:.04em;white-space:nowrap">永不锁屏 · 随时可以关闭 · 不阻塞 ⌘Tab 与输入法切换</div>
      </div>
    </div>
''', w=1280, h=800, full_bleed=True)

# ============================================================ 05 记录 Intent
P5 = shell("05 记录 Intent", "05", "记录 Intent（休息前）", '''
    <div class="win" style="width:500px">
      <div style="padding:15px 18px;border-bottom:1px solid #F0F2F4;display:flex;align-items:center;gap:10px">
        <div class="tl"><i class="r"></i><i class="y"></i><i class="g"></i></div>
        <div style="flex:1"></div>
        <div style="font-size:12.5px;color:#98A0A8">Tacet</div>
      </div>
      <div style="padding:26px 26px 22px">
        <div style="font-size:16.5px;font-weight:600;margin-bottom:6px">休息前，记一下接下来要做什么？</div>
        <div class="sub" style="margin-bottom:18px">一句话就够，省得回来时想不起来。</div>

        <div style="border:1.5px solid #2FA8A0;border-radius:10px;background:#FBFDFD;padding:12px 13px;display:flex;align-items:center;gap:10px;box-shadow:0 0 0 3px rgba(47,168,160,.08)">
          <div style="flex:1;font-size:14px;color:#1F2329">完成 Auth 模块测试</div>
          <div class="num" style="font-size:11.5px;color:#A8AEB5">12/100</div>
        </div>

        <div class="sub" style="margin-top:12px;font-size:12px">休息结束时会原样还给你</div>

        <div style="display:flex;justify-content:flex-end;align-items:center;gap:10px;margin-top:24px">
          <button class="btn btn-plain">跳过</button>
          <button class="btn btn-primary" style="height:38px;padding:0 22px">保存并休息</button>
        </div>
      </div>
    </div>
''', w=640, h=480)

# ============================================================ 06 休息中
def guide(tile_cls, icon, title, desc):
    return f'''<div style="display:flex;align-items:center;gap:12px;background:#FAFBFB;border:1px solid #EEF1F2;border-radius:12px;padding:12px 14px">
      <div class="tile {tile_cls}" style="width:34px;height:34px;border-radius:9px">{icon}</div>
      <div style="line-height:1.35">
        <div style="font-size:13.5px;font-weight:500">{title}</div>
        <div class="sub">{desc}</div>
      </div>
    </div>'''

P6 = shell("06 休息中", "06", "休息中（倒计时与引导）", f'''
    <div class="win" style="width:480px">
      <div style="padding:15px 18px;border-bottom:1px solid #F0F2F4;display:flex;align-items:center;gap:10px">
        <div class="tl"><i class="r"></i><i class="y"></i><i class="g"></i></div>
        <div style="flex:1"></div>
        <div style="font-size:12.5px;color:#98A0A8">休息模式</div>
      </div>
      <div style="padding:28px 26px 24px;display:flex;flex-direction:column;align-items:center">
        <div style="font-size:13px;color:#8B939B;letter-spacing:.16em;margin-bottom:14px">休 息 中</div>
        <div class="num" style="font-size:64px;font-weight:200;color:#2FA8A0;line-height:1;margin-bottom:26px;letter-spacing:.01em">04:37</div>
        <div style="width:100%;display:flex;flex-direction:column;gap:10px">
          {guide("t-eye", IC_EYE, "看向 6 米外，保持 20 秒", "让睫状肌松一下")}
          {guide("t-water", IC_DROP, "喝几口水", "顺手补一次水")}
          {guide("t-move", IC_WALK, "站起来活动 2~3 分钟", "走动或拉伸都可以")}
        </div>
        <button class="btn btn-ghost" style="margin-top:22px;width:100%">提前结束休息</button>
      </div>
    </div>
''', w=620, h=660)

# ============================================================ 07 休息结束
P7 = shell("07 休息结束 · Intent 恢复", "07", "休息结束 · 恢复 Intent", '''
    <div class="win" style="width:500px">
      <div style="padding:15px 18px;border-bottom:1px solid #F0F2F4;display:flex;align-items:center;gap:10px">
        <div class="tl"><i class="r"></i><i class="y"></i><i class="g"></i></div>
        <div style="flex:1"></div>
        <div style="font-size:12.5px;color:#98A0A8">Tacet</div>
      </div>
      <div style="padding:28px 26px 24px;display:flex;flex-direction:column;align-items:center">
        <div style="display:flex;align-items:center;gap:9px;margin-bottom:20px">
          <span style="width:22px;height:22px;border-radius:50%;background:#E9F6EF;color:#4CAF7D;display:flex;align-items:center;justify-content:center">''' + IC_CHECK + '''</span>
          <span style="font-size:17px;font-weight:600">欢迎回来</span>
        </div>

        <div style="width:100%;background:#F7FBF9;border:1px solid #E6F2EB;border-radius:12px;padding:15px 16px">
          <div style="font-size:11.5px;color:#7FA894;letter-spacing:.03em;margin-bottom:7px">休息前你准备继续</div>
          <div style="font-size:15.5px;font-weight:600;color:#1F2329">完成 Auth 模块测试</div>
        </div>

        <div class="sub num" style="margin-top:16px;font-size:12.5px">本次休息 5 分钟 · 活动打卡已完成 ✓</div>

        <button class="btn btn-primary" style="margin-top:22px;width:100%;height:40px;font-size:14.5px">开始</button>
      </div>
    </div>
''', w=640, h=520)

# ============================================================ 08 设置页
def setting_row(tile_cls, icon, label, note=None):
    note_html = f'<span class="sub" style="font-size:11.5px;margin-left:2px">{note}</span>' if note else ''
    return f'''<div class="row">
        <div class="tile {tile_cls}" style="width:28px;height:28px;border-radius:8px">{icon}</div>
        <div class="lab">{label}{note_html}</div>
      </div>'''

def reminder_row(tile_cls, icon, label, interval):
    return f'''<div class="row">
        <div class="tile {tile_cls}" style="width:28px;height:28px;border-radius:8px">{icon}</div>
        <div class="lab">{label}</div>
        <div class="ctl">
          <div class="sw on"><i></i></div>
          <div class="pill num">{interval}<span>分钟</span></div>
        </div>
      </div>'''

def toggle_row(label, on, note=None):
    note_html = f'<span class="sub" style="font-size:11.5px;margin-left:2px">{note}</span>' if note else ''
    cls = "sw on" if on else "sw"
    return f'''<div class="row">
        <div class="lab">{label}{note_html}</div>
        <div class="ctl"><div class="{cls}"><i></i></div></div>
      </div>'''

P8 = shell("08 设置页", "08", "设置页", f'''
    <div class="win" style="width:640px">
      <div style="padding:14px 18px;border-bottom:1px solid #EDEFF2;display:flex;align-items:center;gap:10px;background:#FCFCFD">
        <div class="tl"><i class="r"></i><i class="y"></i><i class="g"></i></div>
        <div style="flex:1;text-align:center;font-size:13.5px;font-weight:600;margin-left:-46px">设置</div>
      </div>
      <div style="padding:20px 22px 18px;background:#F7F8F9;display:flex;flex-direction:column;gap:20px">
        <div>
          <div class="group-title">提醒</div>
          <div class="card">
            {reminder_row("t-teal", IC_CUP, "休息提醒", "50")}
            {reminder_row("t-water", IC_DROP, "喝水提醒", "45")}
            {reminder_row("t-move", IC_WALK, "活动提醒", "60")}
            {reminder_row("t-eye", IC_EYE, "护眼提醒", "40")}
          </div>
        </div>

        <div>
          <div class="group-title">勿扰</div>
          <div class="card">
            {toggle_row("勿扰模式", False)}
            {toggle_row("仅在工作时间提醒", False, "（v0.2 提供）")}
          </div>
        </div>

        <div>
          <div class="group-title">通用</div>
          <div class="card">
            {toggle_row("开机自动启动", True)}
            <div class="row">
              <div class="lab">关于 Tacet</div>
              <div class="ctl">
                <span class="sub num">v0.1.0</span>
                <span style="display:flex;width:15px;color:#C4CAD0">''' + IC_CHEV + '''</span>
              </div>
            </div>
          </div>
        </div>

        <div>
          <div class="group-title">AI 助手</div>
          <div class="card">
            <div class="row" style="padding:15px 16px">
              <div style="flex:1;line-height:1.45">
                <div style="font-size:13.5px;color:#3C444C">AI 助手（可选）</div>
                <div class="sub" style="font-size:12px;margin-top:2px">不配置也能完整使用 Tacet</div>
              </div>
              <div class="ctl">
                <span style="display:flex;width:15px;color:#C4CAD0">''' + IC_CHEV + '''</span>
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
''', w=780, h=920)

PAGES = [
    ("01 菜单栏下拉主面板", P1, 520, 540),
    ("02 菜单栏状态（Level 1）", P2, 1120, 440),
    ("03 系统通知（Level 2）", P3, 560, 600),
    ("04 全屏休息提醒（Level 4）", P4, 1280, 800),
    ("05 记录 Intent", P5, 640, 480),
    ("06 休息中", P6, 620, 660),
    ("07 休息结束 · 恢复 Intent", P7, 640, 520),
    ("08 设置页", P8, 780, 920),
]

# ---------------------------------------------------------------- 输出
for i, (name, html, _w, _h) in enumerate(PAGES, 1):
    with open(os.path.join(OUT, f"page-{i:02d}.html"), "w", encoding="utf-8") as fh:
        fh.write(html)

# 预览：整页纵向排列，便于一次性检查
preview = ['<!DOCTYPE html><html lang="zh-CN"><head><meta charset="utf-8"><style>',
           'body{margin:0;background:#DFE3E6;display:flex;flex-direction:column;gap:20px;padding:20px;align-items:flex-start}',
           'iframe{border:none;background:#F5F6F7;box-shadow:0 4px 18px rgba(0,0,0,.12);display:block}',
           '</style></head><body>']
for i, (name, html, w, h) in enumerate(PAGES, 1):
    preview.append(f'<iframe src="page-{i:02d}.html" width="{w}" height="{h}"></iframe>')
preview.append('</body></html>')
with open(os.path.join(OUT, "preview.html"), "w", encoding="utf-8") as fh:
    fh.write("\n".join(preview))

# 缩略图拼版：2 列 × 4 行，45% 缩放，用于快速整体检查
thumbs = ['<!DOCTYPE html><html lang="zh-CN"><head><meta charset="utf-8"><style>',
          'body{margin:0;background:#C9CED3;padding:24px;font-family:-apple-system,"PingFang SC",sans-serif}',
          '.grid{display:grid;grid-template-columns:repeat(2,1fr);gap:20px}',
          '.cell{background:#fff;border-radius:12px;overflow:hidden;box-shadow:0 6px 20px rgba(0,0,0,.16)}',
          '.cell .capp{font-size:12px;color:#6B737B;padding:8px 12px;font-weight:600}',
          '.wrap{position:relative;overflow:hidden}',
          '.wrap iframe{border:none;transform-origin:0 0;display:block}',
          '</style></head><body><div class="grid">']
for i, (name, html, w, h) in enumerate(PAGES, 1):
    s = min(0.46, 560 / w)
    thumbs.append(f'<div class="cell"><div class="capp">{name}</div>'
                  f'<div class="wrap" style="width:{int(w*s)}px;height:{int(h*s)}px">'
                  f'<iframe src="page-{i:02d}.html" width="{w}" height="{h}" style="transform:scale({s});width:{w}px;height:{h}px"></iframe>'
                  f'</div></div>')
thumbs.append('</div></body></html>')
with open(os.path.join(OUT, "thumbnails.html"), "w", encoding="utf-8") as fh:
    fh.write("\n".join(thumbs))

# 墨刀导入 payload
payload = {"html_list": [{"name": name, "html": html} for name, html, _w, _h in PAGES],
           "page_name": "Tacet v0.1 原型",
           "client": "ZCode"}
with open(os.path.join(OUT, "payload.json"), "w", encoding="utf-8") as fh:
    json.dump(payload, fh, ensure_ascii=False)

print("生成完成：")
for i, (name, _h_, _w_, _hh_) in enumerate(PAGES, 1):
    p = os.path.join(OUT, f"page-{i:02d}.html")
    print(f"  page-{i:02d}.html  {os.path.getsize(p):>7,} bytes  {name}")
print(f"  preview.html / thumbnails.html / payload.json ({os.path.getsize(os.path.join(OUT,'payload.json')):,} bytes)")
