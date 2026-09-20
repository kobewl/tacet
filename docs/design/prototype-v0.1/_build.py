#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""Tacet v0.1 原型 · 第二版设计语言

设计方向：全屏白色毛玻璃（white frosted glass）+ 克制的高级感。
每屏都是 1440×900 的完整桌面场景，内容浮在白色模糊层之上。
所有布局、间距、颜色、字号均为手工指定。
"""
import os

OUT = os.path.dirname(os.path.abspath(__file__))

# ----------------------------------------------------------------- 图标（细线条，1.5 描边，克制）
def svg(d, extra=""):
    # width/height 必须写死：Ardot 转换器不会推断 SVG 固有尺寸，缺省会拉伸变形。
    # 有 .tile svg / .menubar svg 规则的场景由 CSS 覆盖。
    return f'<svg viewBox="0 0 20 20" width="16" height="16" fill="none" {extra}>{d}</svg>'

IC_BRAND = svg('<path d="M10 3.2v13.6M4.4 6.6l11.2 6.8M15.6 6.6 4.4 13.4" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/>')
IC_CUP   = svg('<path d="M3.6 5.4h9.2v6.3a3.3 3.3 0 0 1-3.3 3.3H6.9a3.3 3.3 0 0 1-3.3-3.3V5.4Z" stroke="currentColor" stroke-width="1.4"/><path d="M12.8 7h1.3a1.9 1.9 0 0 1 0 3.8h-1.3" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/><path d="M6.9 2.8v1.4M9.5 2.5v1.7" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/>')
IC_DROP  = svg('<path d="M10 2.8c2.7 3.1 4.4 5.5 4.4 7.5a4.4 4.4 0 0 1-8.8 0c0-2 1.7-4.4 4.4-7.5Z" stroke="currentColor" stroke-width="1.4"/>')
IC_WALK  = svg('<circle cx="11.2" cy="4" r="1.6" stroke="currentColor" stroke-width="1.4"/><path d="M11.6 6.9 9 9.3l1.4 2.4-.9 5M11.6 6.9l2 2.9 2.3.8M10.4 11.7 6.5 12.3" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"/>')
IC_EYE   = svg('<path d="M1.9 10S4.7 5.2 10 5.2 18.1 10 18.1 10 15.3 14.8 10 14.8 1.9 10 1.9 10Z" stroke="currentColor" stroke-width="1.4"/><circle cx="10" cy="10" r="2.3" stroke="currentColor" stroke-width="1.4"/>')
IC_CHECK = svg('<path d="M4.2 10.4 8 14.2l7.8-8.6" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round"/>')
IC_CHEV  = svg('<path d="M8 5l5 5-5 5" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/>')
IC_PAUSE = svg('<path d="M7.6 5.4v9.2M12.4 5.4v9.2" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>')
IC_GEAR  = svg('<circle cx="10" cy="10" r="2.5" stroke="currentColor" stroke-width="1.4"/><path d="M10 2.6v2M10 15.4v2M2.6 10h2M15.4 10h2M4.8 4.8l1.4 1.4M13.8 13.8l1.4 1.4M15.2 4.8l-1.4 1.4M6.2 13.8l-1.4 1.4" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/>')

# macOS 菜单栏右侧小图标
IC_WIFI  = svg('<path d="M3 8.1a10 10 0 0 1 14 0M5.6 10.9a6.3 6.3 0 0 1 8.8 0M8.2 13.7a2.6 2.6 0 0 1 3.6 0" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/><circle cx="10" cy="15.9" r="1" fill="currentColor"/>')
IC_BATT  = '<svg viewBox="0 0 26 14" width="26" height="14" fill="none"><rect x="1" y="3" width="19" height="8" rx="2.6" stroke="currentColor" stroke-width="1.2" opacity=".5"/><rect x="2.6" y="4.6" width="14.4" height="4.8" rx="1.5" fill="currentColor"/><path d="M22 5.6v2.8c1.1-.35 1.6-.6 1.6-.95 0-.4-.5-.7-1.6-1.05Z" fill="currentColor" opacity=".5"/></svg>'
IC_SEARCH= svg('<circle cx="8.8" cy="8.8" r="5.1" stroke="currentColor" stroke-width="1.5"/><path d="M12.6 12.6 16.4 16.4" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>')
IC_CTRL  = svg('<rect x="2.6" y="5" width="14.8" height="10" rx="3" stroke="currentColor" stroke-width="1.3"/><path d="M7.2 10h5.6" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/>')
IC_APPLE = '<svg viewBox="0 0 16 16" width="15" height="15" fill="currentColor"><path d="M11.1 8.4c0-1.6 1.3-2.4 1.4-2.4-.8-1.1-1.9-1.3-2.3-1.3-1-.1-1.9.6-2.4.6-.5 0-1.3-.6-2.1-.6-1.1 0-2.1.6-2.7 1.6-1.1 2-.3 5 .8 6.6.5.8 1.2 1.6 2 1.6.8 0 1.1-.5 2.1-.5s1.2.5 2.1.5c.9 0 1.4-.8 1.9-1.5.6-.9.9-1.7.9-1.8-.1 0-1.7-.7-1.7-2.4ZM9.6 3.6c.4-.5.7-1.2.6-1.9-.6 0-1.4.4-1.8.9-.4.5-.8 1.2-.7 1.9.7.1 1.4-.4 1.9-.9Z"/></svg>'

# ----------------------------------------------------------------- 设计令牌
CSS = '''
*{margin:0;padding:0;box-sizing:border-box}
html,body{width:100%;height:100%;overflow:hidden}
body{
  font-family:-apple-system,BlinkMacSystemFont,"SF Pro Display","SF Pro Text","PingFang SC","Helvetica Neue",Arial,sans-serif;
  color:#0B0D10;-webkit-font-smoothing:antialiased;text-rendering:optimizeLegibility;
  font-size:14px;letter-spacing:-.008em;
}
.num{font-variant-numeric:tabular-nums;font-feature-settings:"tnum" 1}
.scene{position:relative;width:100%;height:100%;overflow:hidden}

/* 桌面底：柔和的多点渐变，作为毛玻璃的光源 */
.desk{position:absolute;inset:-10%;
  background:
    radial-gradient(720px 600px at 13% 4%,   #D3E6EE 0%, rgba(211,230,238,0) 60%),
    radial-gradient(660px 540px at 88% 12%,  #F2E3D6 0%, rgba(242,227,214,0) 62%),
    radial-gradient(820px 700px at 76% 98%,  #CFE4EA 0%, rgba(207,228,234,0) 64%),
    radial-gradient(600px 560px at 24% 90%,  #E4DFF2 0%, rgba(228,223,242,0) 66%),
    radial-gradient(520px 420px at 54% 46%,  #EAF2F5 0%, rgba(234,242,245,0) 70%),
    linear-gradient(158deg,#F2F6F8 0%,#E7EEF3 46%,#F1EDE8 100%);
}
/* 极细颗粒质感：用叠层半透明点阵近似，避免内联 data-URI（Ardot 转换器会误当外链资源下载）*/
.grain{position:absolute;inset:0;opacity:.5;pointer-events:none;
  background-image:
    radial-gradient(rgba(11,13,16,.055) .6px, transparent .6px),
    radial-gradient(rgba(11,13,16,.035) .5px, transparent .5px);
  background-size:3px 3px, 4px 4px;
  background-position:0 0, 1.5px 1.5px}

/* 全屏白色毛玻璃：产品的核心视觉语言 */
.veil{position:absolute;inset:0;background:rgba(255,255,255,.30);
  -webkit-backdrop-filter:saturate(210%) blur(52px);backdrop-filter:saturate(210%) blur(52px)}
/* 顶部来光 + 中心提亮，制造玻璃厚度 */
.veil-2{position:absolute;inset:0;
  background:
    linear-gradient(180deg, rgba(255,255,255,.72) 0%, rgba(255,255,255,.10) 26%, rgba(255,255,255,0) 55%),
    radial-gradient(1000px 680px at 50% 38%, rgba(255,255,255,.62) 0%, rgba(255,255,255,0) 72%)}
/* 四周轻微压暗，让画面收边 */
.veil-3{position:absolute;inset:0;background:radial-gradient(120% 100% at 50% 45%, rgba(255,255,255,0) 58%, rgba(20,32,44,.045) 100%)}

/* 玻璃卡片 */
.glass{background:rgba(255,255,255,.66);
  -webkit-backdrop-filter:saturate(170%) blur(36px);backdrop-filter:saturate(170%) blur(36px);
  border:1px solid rgba(255,255,255,.86);
  box-shadow:0 28px 70px -24px rgba(15,23,32,.20), 0 4px 14px -6px rgba(15,23,32,.08), inset 0 1px 0 rgba(255,255,255,.9)}
.glass-soft{background:rgba(255,255,255,.55);
  -webkit-backdrop-filter:saturate(160%) blur(26px);backdrop-filter:saturate(160%) blur(26px);
  border:1px solid rgba(255,255,255,.8);
  box-shadow:0 18px 46px -22px rgba(15,23,32,.20), inset 0 1px 0 rgba(255,255,255,.85)}

/* 发丝线 */
.hair{height:1px;background:rgba(11,13,16,.07)}
.hair-v{width:1px;background:rgba(11,13,16,.07)}

/* 图标底座 */
.tile{display:flex;align-items:center;justify-content:center;flex:0 0 auto;border-radius:10px}
.tile svg{width:18px;height:18px}
.t-rest{background:rgba(12,124,116,.10);color:#0C7C74}
.t-water{background:rgba(28,110,176,.10);color:#1C6EB0}
.t-move{background:rgba(178,110,36,.11);color:#A9681F}
.t-eye{background:rgba(107,101,151,.11);color:#6B6597}
.t-ok{background:rgba(52,133,92,.11);color:#34855C}

/* 按钮 */
.btn{display:inline-flex;align-items:center;justify-content:center;gap:7px;border:none;font-family:inherit;
  border-radius:999px;font-size:14px;font-weight:500;letter-spacing:-.005em;cursor:default;white-space:nowrap}
.btn-primary{background:linear-gradient(180deg,#127F76 0%,#0C6E66 100%);color:#fff;
  box-shadow:0 1px 0 rgba(255,255,255,.18) inset, 0 10px 24px -10px rgba(12,110,102,.55), 0 2px 6px -2px rgba(12,110,102,.3)}
.btn-ghost{background:rgba(255,255,255,.62);color:#20262D;border:1px solid rgba(11,13,16,.09);
  box-shadow:0 1px 2px rgba(15,23,32,.045), inset 0 1px 0 rgba(255,255,255,.9);
  -webkit-backdrop-filter:blur(12px);backdrop-filter:blur(12px)}
.btn-quiet{background:rgba(255,255,255,.5);color:#4B525A;border:1px solid rgba(11,13,16,.07)}
.btn-text{background:transparent;color:#8C939B;font-size:13px}

/* 标签 */
.kicker{font-size:10.5px;font-weight:600;letter-spacing:.30em;color:#B9BFC6;text-transform:uppercase}
.sub{font-size:12.5px;color:#8C939B;letter-spacing:-.004em}
.ink2{color:#4B525A}

/* 开关 */
.sw{width:40px;height:23px;border-radius:999px;background:rgba(11,13,16,.14);position:relative;flex:0 0 auto}
.sw i{position:absolute;top:2px;left:2px;width:19px;height:19px;border-radius:50%;background:#fff;
  box-shadow:0 1px 3px rgba(15,23,32,.22), 0 0 0 .5px rgba(15,23,32,.04)}
.sw.on{background:linear-gradient(180deg,#12867C,#0C6E66)}
.sw.on i{left:19px}

/* 顶栏（macOS 菜单栏） */
.menubar{position:absolute;top:0;left:0;right:0;height:28px;display:flex;align-items:center;justify-content:space-between;
  padding:0 14px;background:rgba(255,255,255,.72);
  -webkit-backdrop-filter:saturate(180%) blur(30px);backdrop-filter:saturate(180%) blur(30px);
  border-bottom:1px solid rgba(11,13,16,.055);font-size:13px;color:#1E242B;z-index:5}
.menubar .l{display:flex;align-items:center;gap:16px}
.menubar .l b{font-weight:600}
.menubar .r{display:flex;align-items:center;gap:14px;color:#2C333A}
.menubar .r svg{width:17px;height:17px;opacity:.82}

/* 行 */
.row{display:flex;align-items:center;gap:12px;padding:13px 16px;min-height:56px}
.row .lab{flex:1;font-size:13.5px;color:#20262D;letter-spacing:-.006em}
.ctl{display:flex;align-items:center;gap:14px}
.pill{display:flex;align-items:center;gap:5px;height:30px;padding:0 10px;border-radius:9px;
  background:rgba(255,255,255,.7);border:1px solid rgba(11,13,16,.08);
  font-size:13px;color:#20262D;box-shadow:inset 0 1px 0 rgba(255,255,255,.9)}
.pill span{font-size:11.5px;color:#8C939B}

/* ---------------------------------------------------------------
   两个环，各司其职，互不干扰：

   1) 进度环 .prog —— 信息型。外圈随剩余时间匀速收缩，只动 arc 长度，
      位置和缩放都不动。这是「还剩多久」的唯一视觉表达。
   2) 呼吸脉动 .pulse —— 引导型。内圈做 10 秒一周期的极小幅缩放
      （4 秒吸 / 6 秒呼 ＝ 6 次/分）。这是本版唯一的持续动效。

   为什么不做「动态背景」：屏幕运动本身对眼睛没有益处（AAO 明确列出
   数字眼疲劳的成因是眨眼减少、干燥、眩光、亮度失配、姿势，不含画面运动），
   而后台运动反而会自动捕获周边视觉注意力 —— 对一个主张「不抢注意力」的
   产品是反向的，且可能诱发前庭不适（WCAG 2.3.3）。
   真正有生理依据的是「慢呼吸」与「看远处」，所以动效服务于这两件事。
   --------------------------------------------------------------- */
.ring{position:absolute;display:flex;align-items:center;justify-content:center}
.ring svg{position:absolute;inset:0;width:100%;height:100%;overflow:visible}

/* 进度环：只过渡 arc 长度，不参与缩放 */
.prog{transition:stroke-dashoffset 1s linear}

/* 呼吸脉动：独立图层，10 秒一个周期（4 秒吸 / 6 秒呼 ＝ 6 次/分）。
   幅度 5%：上一版用 3.8%，实际在 27 寸屏上几乎看不出来，起不到引导作用；
   5% 是「余光能明确跟上、但不会觉得画面在晃」的平衡点。 */
.pulse{transform-origin:50% 50%;animation:breathe 10s cubic-bezier(.42,0,.58,1) infinite}
@keyframes breathe{
  0%  {transform:scale(1);   opacity:.45}
  40% {transform:scale(1.05);opacity:1}
  100%{transform:scale(1);   opacity:.45}
}

/* 倒计时：整串更新，不做逐位拆分。
   原因：Ardot 的 HTML 转换器会在页面加载数秒后抓取 DOM，若数字被拆成
   外层文本 + 内层 <span>（逐位滚动那种写法），它只会读到外层文本节点，
   导入的原型就会出现「04:2」这样缺一位的情况。
   改成一整个文本节点后，任何时刻被抓到的都是完整可读的数值。
   高级感由「秒数做极轻的呼吸式明暗 + 整块极小幅位移」来体现。*/
.cd{transition:opacity .38s ease}
.cd.tick{animation:cdPulse .5s cubic-bezier(.22,.61,.36,1)}
@keyframes cdPulse{
  0%  {opacity:.62; transform:translateY(.045em)}
  100%{opacity:1;   transform:translateY(0)}
}

/* ---------------------------------------------------------------
   真实输入态（03 屏）：
   原型里的输入框如果只是个静态方块，评审时没法判断「敲字是不是舒服」。
   所以这里做了三件在真机上才看得出来的事：
   1) 真光标 —— 用 1px 竖线 + steps 动画做闪烁，节奏与 macOS 一致（约 1s）；
      用 steps 而非 ease，因为系统光标是硬切换，不是渐隐。
   2) 聚焦环呼吸 —— 极慢的 4 秒明暗变化，暗示「这里在等你说话」，
      不抢戏但让静止页面有生命感。
   3) 已输入文字 + 字符计数 + 清除按钮的完整状态。
   --------------------------------------------------------------- */
.field{position:relative;display:flex;align-items:center;gap:12px;
  padding:15px 16px;border-radius:13px;background:rgba(255,255,255,.82);
  border:1px solid rgba(12,110,102,.34);
  box-shadow:0 0 0 4px rgba(12,110,102,.10), inset 0 1px 0 rgba(255,255,255,.95);
  animation:ringBreath 4.2s ease-in-out infinite}
@keyframes ringBreath{
  0%,100%{box-shadow:0 0 0 4px rgba(12,110,102,.10), inset 0 1px 0 rgba(255,255,255,.95)}
  50%    {box-shadow:0 0 0 6px rgba(12,110,102,.055), inset 0 1px 0 rgba(255,255,255,.95)}
}
.caret{display:inline-block;width:1.5px;height:1.05em;background:#0C6E66;vertical-align:-.18em;
  margin-left:1px;animation:blink 1.06s steps(1,end) infinite}
@keyframes blink{0%,49%{opacity:1}50%,100%{opacity:0}}
.field-clear{display:flex;align-items:center;justify-content:center;width:16px;height:16px;
  border-radius:50%;background:rgba(11,13,16,.16);color:#fff;flex:0 0 auto}

/* 快捷键提示（03 屏用）：让「怎么提交」这件事可见，不用猜 */
.kbd{display:inline-flex;align-items:center;justify-content:center;min-width:19px;height:19px;
  padding:0 5px;border-radius:5px;background:rgba(255,255,255,.72);
  border:1px solid rgba(11,13,16,.10);box-shadow:0 1px 0 rgba(11,13,16,.05);
  font-size:10.5px;color:#8C939B;letter-spacing:.02em}

/* 鸡汤：休息时的一个「可看可不看」，字号与对比度都压到最低 */
.quote{width:460px;text-align:center;min-height:44px}
.quote-t{font-size:13.5px;line-height:1.66;color:#6B737B;letter-spacing:.004em;
  transition:opacity .5s ease}
.quote-f{font-size:11.5px;color:#AEB5BC;margin-top:9px;letter-spacing:.02em;
  transition:opacity .5s ease}
.quote-f:empty{display:none}
.quote.fade .quote-t,.quote.fade .quote-f{opacity:0}

/* 尊重系统「减少动态效果」：动效全部关闭（WCAG 2.3.3） */
@media (prefers-reduced-motion: reduce){
  .pulse{animation:none;opacity:.6}
  .cd,.cd.tick{animation:none;transition:none}
  .prog{transition:none}
  .quote-t,.quote-f{transition:none}
  .caret{animation:none}
  .field{animation:none}
}
'''

# ----------------------------------------------------------------- 场景零件
DESK = '<div class="desk"></div><div class="grain"></div>'

# 休息时那句鸡汤的共享逻辑（01 / 02 屏都用）。
#
# 为什么以本地池为主、API 为辅：
# 实测所有免费名言 API（一言 / vvhan / 52vmy / oick / tenapi / xygeng）
# 里，没有一个能稳定提供「励志」内容 —— 一言的分类是诗词、文学、哲学，
# 拉出来的是「苟利国家生死以」「不要说我一无所有，我们要做天下的主人」，
# 甚至「梦想还是要有的，万一见鬼了呢」这种抖机灵的。
# 这些出现在「劝你休息」的场景里是错位的。
#
# 所以：本地精选项保证「一定是对的话」，API 负责「每次不一样」的惊喜感，
# 且必须通过励志关键词白名单 + 负面词黑名单才会被采用；
# 拿不到合适的就退回本地池，轮换照常进行，绝不会露出不合适的句子。
QUOTE_JS = '''
// —— 鸡汤 ——
(function(){
  var wrap = document.getElementById('quote');
  var tEl = document.getElementById('quoteText');
  var fEl = document.getElementById('quoteFrom');
  if(!wrap || !tEl) return;

  // 本地精选项：质量确定，保证任何情况下都有得说。
  // 池子做到 60 条 —— 一次休息只显示一句，且每次进入都从随机位置开始，
  // 「每次都不一样」这个诉求由池子本身就满足了，不必依赖 API 的命中率。
  var POOL = [
    '你今天的努力，是明天的伏笔。',
    '慢慢来，比较快。',
    '所有的伟大，都源于一个勇敢的开始。',
    '你不必走得很快，但要走得很久。',
    '每一次坚持，都在悄悄改变你。',
    '认真生活的人，运气都不会太差。',
    '别急，你想要的，岁月都会给你。',
    '把每一件简单的事做好，就是不简单。',
    '路虽远，行则将至；事虽难，做则必成。',
    '越努力，越幸运。',
    '心之所向，素履以往。',
    '保持热爱，奔赴山海。',
    '你只管努力，剩下的交给时间。',
    '每天进步一点点，坚持带来大改变。',
    '不是因为看到希望才坚持，而是因为坚持才看到希望。',
    '你要悄悄拔尖，然后惊艳所有人。',
    '与其抱怨黑暗，不如点亮一支蜡烛。',
    '现在走的每一步，都算数。',
    '你的努力，时光看得见。',
    '愿你所行皆坦途，所遇皆温柔。',
    '把日子过成自己喜欢的样子。',
    '今天也要好好吃饭，好好休息。',
    '你现在付出的，都会以另一种方式回来。',
    '不必太用力，日子是过给自己看的。',
    '把今天过好，明天自然会来。',
    '哪怕只前进一小步，也比站在原地强。',
    '你比想象中更有力量。',
    '慢慢变好，是给自己最好的礼物。',
    '不要着急，最好的总在最不经意的时候出现。',
    '所有的失去，都会以另一种方式归来。',
    '愿你走出半生，归来仍是少年。',
    '生活明朗，万物可爱。',
    '人间值得，你更值得。',
    '星光不问赶路人，时光不负有心人。',
    '你现在的气质里，藏着你走过的路。',
    '把每一个平凡的日子，过成诗。',
    '愿你有前程可奔赴，也有岁月可回头。',
    '每一个不曾起舞的日子，都是对生命的辜负。',
    '不乱于心，不困于情，不畏将来。',
    '心里有光，哪里都是春天。',
    '愿你所有的努力都不被辜负。',
    '做一朵向阳的花，安静地生长。',
    '你的坚持，终将美好。',
    '只要方向对了，就不怕路远。',
    '每一天都是一个新的开始。',
    '相信自己，你比想象中优秀。',
    '努力的意义，是让未来的自己有选择。',
    '别让明天的你，讨厌今天的自己。',
    '你值得这世间所有的美好。',
    '愿你所求皆如愿，所行化坦途。',
    '把心放宽，把事看淡，把日子过好。',
    '岁月漫长，然而值得等待。',
    '愿你有好运气，如果没有，愿你在不幸中学会慈悲。',
    '愿你被这世界温柔以待。',
    '所有的美好，都在路上。',
    '你尽管精彩，老天自有安排。',
    '愿你不饶点滴，不舍昼夜。',
    '用心生活，用力向上。',
    '将来的你，一定会感谢现在拼命的自己。',
    '把每一天当作礼物，好好珍惜。',
    '慢慢来，你会成为你想成为的人。'
  ];

  var idx = Math.floor(Math.random() * POOL.length);

  // 只有正向词才接受，且必须不含负面意象
  var GOOD = /(努力|坚持|梦想|希望|成长|勇敢|热爱|相信|未来|时光|岁月|光芒|温柔|美好|慢慢|认真|值得|开始|改变|成为|做好|进步|幸运|远方|山海|坦途|绽放|出发|向前)/;
  var BAD  = /(死|杀|血|战|敌人|革命|恨|怒|毁灭|深渊|地狱|苦|痛|泪|错|失败|输|可怜|孤独|绝望|无奈|笑话|见鬼|放弃|算了)/;
  // 一言里混着日文和英文条目（实测出现过「希望とシェイクハンドして。」），
  // 中文界面里冒出外语很出戏，直接按字符集排除。
  var NON_ZH = /[\\u3040-\\u30ff\\u0400-\\u04ff]/;   // 平假名/片假名/西里尔
  var LATIN  = /^[\\x00-\\x7f\\s]+$/;                 // 纯 ASCII（英文句）

  function acceptable(s){
    if(!s) return false;
    if(s.length < 8 || s.length > 26) return false;   // 太长在休息时读不完
    if(BAD.test(s)) return false;
    if(NON_ZH.test(s) || LATIN.test(s)) return false;
    return GOOD.test(s);
  }

  function reveal(text, from){
    wrap.classList.add('fade');
    setTimeout(function(){
      tEl.textContent = text;
      // 作者只在真的有意义时才显示 —— 「— 网络」这种反而掉价
      fEl.textContent = from ? '— ' + from : '';
      wrap.classList.remove('fade');
    }, 520);
  }

  function fromPool(){
    idx = (idx + 1) % POOL.length;
    reveal(POOL[idx], '');
  }

  var API = 'https://v1.hitokoto.cn/?encode=json&charset=utf-8';

  // 关于 API 的实际定位（这是实测后调低的，不是偷懒）：
  // 一言的库以诗词、文学、网易云评论为主，励志内容极少，加上语种和气质
  // 过滤后命中率只有 2% 上下（60 条过 1 条）。如果为了凑够句子去循环拉取，
  // 等于对一个免费 API 发几百次请求 —— 既不礼貌也不可靠。
  //
  // 所以：本地池负责「稳定输出」，API 只做低频点缀（每次休息最多探 3 次），
  // 命中就用、不命中就用池子。用户感知到的「每次都不一样」由 60 条池子保证。
  var cache = [];
  var fetching = false;
  var MAX_PROBE = 3;

  function refill(){
    if(fetching) return;
    fetching = true;
    var jobs = [];
    for(var i = 0; i < MAX_PROBE; i++){
      jobs.push(
        fetch(API + '&_=' + Date.now() + i, { cache: 'no-store' })
          .then(function(r){ return r.json(); })
          .then(function(d){
            var who = d.from_who || '';
            if(acceptable(d.hitokoto) && who && who !== '网络'){
              cache.push({ text: d.hitokoto, from: who });
            }
          })
          .catch(function(){})
      );
    }
    Promise.all(jobs).then(function(){ fetching = false; });
  }

  function next(){
    // API 里有货就偶尔用一条（每 4 次换句掺 1 条），其余走本地池
    if(cache.length && Math.random() < 0.25){
      var q = cache.shift();
      reveal(q.text, q.from);
    } else {
      fromPool();
    }
    if(cache.length === 0) refill();
  }

  refill();       // 进页面就开始囤货
  // 换句节奏随场景不同（由 data-delay 指定）：
  //   01 屏 8 秒 —— 这是「询问」界面，用户可能在犹豫，先让页面安静一会儿
  //   02 屏 10 秒 —— 休息刚开始，先让他看完行动清单，鸡汤是之后的事
  // 这样安排比「一进来就换」更像一个懂分寸的同事：先办事，再闲聊。
  var delay = parseInt(wrap.dataset.delay || '8000', 10);
  setTimeout(next, delay);
  setInterval(next, 60000);   // 之后每分钟一句，比一次休息还长，不打断
})();
'''

def menubar(tacet_right, tacet_style=""):
    return f'''<div class="menubar">
  <div class="l">
    <span style="display:flex;width:15px;opacity:.9">{IC_APPLE}</span>
    <b>Finder</b><span>文件</span><span>编辑</span><span>显示</span><span>前往</span><span>窗口</span><span>帮助</span>
  </div>
  <div class="r">
    <span style="display:flex">{IC_CTRL}</span>
    <span style="display:flex">{IC_WIFI}</span>
    <span style="display:flex;width:26px">{IC_BATT}</span>
    <span style="display:flex">{IC_SEARCH}</span>
    <span style="width:1px;height:14px;background:rgba(11,13,16,.10)"></span>
    <span style="display:flex;align-items:center;gap:7px;{tacet_style}">{tacet_right}</span>
  </div>
</div>'''

def scene(inner, w=1440, h=900, veil=True, bar=""):
    """把内容放进 1440×900 的桌面场景，可选白毛玻璃层。"""
    veil_html = '<div class="veil"></div><div class="veil-2"></div><div class="veil-3"></div>' if veil else '<div class="veil-3"></div>'
    return f'''<div class="scene" style="width:{w}px;height:{h}px">
  {DESK}
  {bar}
  {veil_html}
  <div style="position:absolute;inset:0">{inner}</div>
</div>'''

def doc(title, body, w=1440, h=900, script=""):
    """页面骨架。script 里是页面级 JS（实时倒计时、名言拉取等）。
    注意：Ardot 的 HTML 转换器不执行 JS，所以所有会变的内容都必须在
    HTML 里先写死一份可读的静态值，再由 JS 接管。"""
    js = f'<script>{script}</script>' if script else ""
    return f'''<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="utf-8">
<title>{title}</title>
<style>{CSS}</style>
</head>
<body>
<div style="width:{w}px;height:{h}px">{body}</div>
{js}
</body>
</html>
'''

# ================================================================ 01 全屏休息提醒
def s01():
    reasons = [("已连续工作 78 分钟"), ("未检测到会议"), ("上次休息是 2 小时前")]
    rhtml = "".join(f'''<div style="display:flex;align-items:center;gap:12px">
      <span style="display:flex;width:16px;height:16px;color:#0C7C74;opacity:.85">{IC_CHECK}</span>
      <span style="font-size:15px;color:#3A424A;letter-spacing:-.006em">{t}</span>
    </div>''' for t in reasons)
    inner = f'''
  <!-- 倒计时背后的呼吸光晕：比 02 屏更淡，暗示「准备停下来」而不是抓住注意力 -->
  <div style="position:absolute;inset:0;display:flex;flex-direction:column;align-items:center;justify-content:center">
    <!-- 品牌 -->
    <div style="display:flex;align-items:center;gap:11px;margin-bottom:52px">
      <span style="display:flex;width:15px;color:#C2C8CE">{IC_BRAND}</span>
      <span class="kicker">Tacet</span>
    </div>

    <!-- 主标题 -->
    <div style="font-size:30px;font-weight:500;letter-spacing:-.022em;color:#0B0D10;margin-bottom:30px">建议休息一下</div>

    <!-- 倒计时：数字逐位滚动，位移极小 -->
    <div class="num cd" id="cd" style="font-size:112px;font-weight:200;line-height:1;letter-spacing:-.035em;color:#0B0D10;margin-bottom:46px"
         data-sec="300">05:00</div>

    <!-- 理由 -->
    <div style="width:360px">
      <div class="hair" style="margin-bottom:26px"></div>
      <div class="kicker" style="font-size:10px;letter-spacing:.26em;color:#AEB5BC;margin-bottom:20px;text-align:center">为什么现在提醒我</div>
      <div style="display:flex;flex-direction:column;gap:15px">{rhtml}</div>
      <div class="hair" style="margin-top:26px"></div>
    </div>

    <!-- 操作 -->
    <div style="display:flex;align-items:center;gap:10px;margin-top:40px">
      <button class="btn btn-primary" style="height:46px;padding:0 34px;font-size:15px">现在休息</button>
      <button class="btn btn-ghost" style="height:46px;padding:0 20px">延后 1 分钟</button>
      <button class="btn btn-ghost" style="height:46px;padding:0 20px">3 分钟</button>
      <button class="btn btn-ghost" style="height:46px;padding:0 20px">5 分钟</button>
    </div>

    <button class="btn btn-text" style="margin-top:22px">跳过这次提醒</button>

    <!-- 等待你决定时的一句话，位置在页脚之上，绝不干扰主决策 -->
    <div class="quote" id="quote" data-delay="8000" style="position:absolute;bottom:88px">
      <div class="quote-t" id="quoteText">你今天的努力，是明天的伏笔。</div>
      <div class="quote-f" id="quoteFrom"></div>
    </div>

    <!-- 页脚约束说明 -->
    <div class="sub" style="position:absolute;bottom:44px;font-size:11.5px;letter-spacing:.02em;color:#AEB5BC">
      永不锁屏 · 随时可以关闭 · 不阻塞 ⌘Tab 与输入法切换
    </div>
  </div>'''

    script = '''
// 倒计时数字：只给变化的那一位做位移淡入。位移量 .15em —— 快到几乎察觉不到，
// 但比硬切换「活」。
// 注：01 屏刻意不加进度环 —— 这一屏要回答的是「为什么现在提醒我」，
// 页面已有标题 / 倒计时 / 理由 / 操作四层信息，再加环只会打架。
// 进度环留给 02 屏，那里用户已经接受休息，需要的是「还剩多久」。
(function(){
  var el = document.getElementById('cd');
  if(!el) return;
  var sec = parseInt(el.dataset.sec || '300', 10);
  var prev = el.textContent;

  function pad(n){ return n < 10 ? '0' + n : '' + n; }
  function fmt(s){
    var m = Math.floor(s / 60), r = s % 60;
    return pad(m) + ':' + pad(r);
  }
  function tick(){
    if(sec > 0) sec--;
    var next = fmt(sec);
    if(next === prev) return;
    // 整串替换，不做逐位拆分 —— 见 CSS 里 .cd 的说明
    el.textContent = next;
    el.classList.remove('tick');
    void el.offsetWidth;              // 强制重排，让动画能重复触发
    el.classList.add('tick');
    prev = next;
  }
  // 延迟 2 秒起步：页面加载时不跳字。
  setTimeout(function(){
    tick();
    setInterval(tick, 1000);
  }, 2000);
})();
'''

    return doc("01 全屏休息提醒", scene(inner), script=script + QUOTE_JS)

# ================================================================ 02 休息中
def s02():
    rows = [
        ("t-eye", IC_EYE, "看向 6 米外，保持 20 秒", "让睫状肌松一下"),
        ("t-water", IC_DROP, "喝几口水", "顺手补一次水"),
        ("t-move", IC_WALK, "站起来活动 2~3 分钟", "走动或拉伸都可以"),
    ]
    rh = ""
    for i, (cls, ic, t, d) in enumerate(rows):
        rh += f'''<div style="display:flex;align-items:center;gap:14px;padding:16px 18px">
        <div class="tile {cls}" style="width:36px;height:36px;border-radius:11px">{ic}</div>
        <div style="line-height:1.4">
          <div style="font-size:14px;font-weight:500;color:#20262D;letter-spacing:-.008em">{t}</div>
          <div class="sub" style="margin-top:2px">{d}</div>
        </div>
      </div>'''
        if i < 2:
            rh += '<div class="hair" style="margin-left:68px"></div>'

    # 两个环分层：外圈进度（信息）+ 内圈脉动（引导），互不干扰
    # 环心对准「倒计时 + 呼吸提示」这一组，而不是整个视口中心
    ring = f'''<div class="ring" style="width:340px;height:340px;position:absolute;left:50%;top:280px;margin-left:-170px;margin-top:-170px">
      <!-- 外圈：进度，只变 arc 长度 -->
      <svg viewBox="0 0 340 340" width="340" height="340" fill="none">
        <circle cx="170" cy="170" r="150" stroke="rgba(11,13,16,.05)" stroke-width="1.5"/>
        <!-- 周长 2πr = 2×π×150 ≈ 942.5 -->
        <circle class="prog" id="prog" cx="170" cy="170" r="150"
                stroke="rgba(12,110,102,.26)" stroke-width="1.5"
                stroke-dasharray="942.5" stroke-dashoffset="0"
                transform="rotate(-90 170 170)" stroke-linecap="round"/>
      </svg>
      <!-- 内圈：呼吸脉动，独立图层，不受进度环影响 -->
      <div class="pulse" style="width:248px;height:248px;position:relative">
        <svg viewBox="0 0 248 248" width="248" height="248" fill="none">
          <circle cx="124" cy="124" r="123" stroke="rgba(12,110,102,.16)" stroke-width="1"/>
        </svg>
      </div>
    </div>'''

    inner = f'''
  {ring}
  <div style="position:absolute;inset:0;display:flex;flex-direction:column;align-items:center;justify-content:center">
    <div class="kicker" style="margin-bottom:16px">休息中</div>
    <div class="num cd" id="cd" style="font-size:88px;font-weight:200;line-height:1;letter-spacing:-.03em;color:#0C6E66;margin-bottom:18px"
         data-sec="277">04:37</div>

    <!-- 呼吸引导：唯一持续动效 -->
    <div class="sub" id="breatheHint" style="font-size:12px;color:#8FA6A2;margin-bottom:34px;height:17px">吸气　4 秒</div>

    <div class="glass" style="width:440px;border-radius:20px;overflow:hidden">{rh}</div>

    <!-- 休息时的一句话：可看可不看，字号对比度都压到最低 -->
    <div class="quote" id="quote" data-delay="10000" style="margin-top:34px">
      <div class="quote-t" id="quoteText">你今天的努力，是明天的伏笔。</div>
      <div class="quote-f" id="quoteFrom"></div>
    </div>

    <button class="btn btn-ghost" style="margin-top:30px;height:44px;padding:0 26px">提前结束休息</button>
  </div>'''

    script = '''
// 倒计时：只给「变化了的那一位」加滚动动画，且位移极小
(function(){
  var el = document.getElementById('cd');
  if(!el) return;
  var sec = parseInt(el.dataset.sec || '277', 10);
  var total = sec;              // 用于换算外圈进度
  var prev = el.textContent;
  var prog = document.getElementById('prog');
  var CIRC = 942.5;             // 2πr, r=150

  function pad(n){ return n < 10 ? '0' + n : '' + n; }
  function fmt(s){
    var m = Math.floor(s / 60), r = s % 60;
    return pad(m) + ':' + pad(r);
  }
  function tick(){
    if(sec > 0) sec--;
    if(prog){
      // 剩余比例映射到描边偏移：走完一圈正好休息结束
      prog.setAttribute('stroke-dashoffset', String(CIRC * (1 - sec / total)));
    }
    var next = fmt(sec);
    if(next === prev) return;
    // 整串替换：保证任何时候 DOM 里都是完整的数值（见 CSS 里的说明）
    el.textContent = next;
    el.classList.remove('tick');
    void el.offsetWidth;              // 强制重排以重启动画
    el.classList.add('tick');
    prev = next;
  }
  // 延迟 2 秒起步：既让页面加载时不跳字，也让 Ardot 的 HTML 转换器
  // 抓到的是完整的初始值（否则会截到位移动画的中间帧，缺一位数字）。
  setTimeout(function(){
    tick();
    setInterval(tick, 1000);
  }, 2000);
})();

// 呼吸引导文案：与 10 秒 CSS 动画同一节奏（吸 4s / 呼 6s）
(function(){
  var el = document.getElementById('breatheHint');
  if(!el) return;
  var inhale = true;
  function step(){
    el.style.opacity = '0';
    setTimeout(function(){
      el.textContent = inhale ? '吸气　4 秒' : '呼气　6 秒';
      el.style.opacity = '1';
    }, 220);
    inhale = !inhale;
    setTimeout(step, inhale ? 6000 : 4000);
  }
  el.style.transition = 'opacity .22s ease';
  setTimeout(step, 4000);
})();

'''

    return doc("02 休息中", scene(inner), script=script + QUOTE_JS)

# ================================================================ 03 记录 Intent
def s03():
    inner = f'''
  <div style="position:absolute;inset:0;display:flex;align-items:center;justify-content:center">
    <div class="glass" style="width:560px;border-radius:22px;padding:36px 38px 30px">
      <div style="font-size:21px;font-weight:600;letter-spacing:-.018em;color:#0B0D10">休息前，记一下接下来要做什么？</div>
      <div class="sub" style="margin-top:9px">一句话就够，省得回来时想不起来。</div>

      <!-- 真实输入态：光标在闪、聚焦环在呼吸、字符计数、可清除 -->
      <div class="field" style="margin-top:28px">
        <div style="flex:1;font-size:15px;color:#0B0D10;letter-spacing:-.006em;display:flex;align-items:baseline">
          <span>完成 Auth 模块测试</span><span class="caret"></span>
        </div>
        <div class="num" style="font-size:11.5px;color:#AEB5BC;flex:0 0 auto">12/100</div>
        <div class="field-clear"><svg viewBox="0 0 20 20" width="9" height="9" fill="none">
          <path d="M5.6 5.6l8.8 8.8M14.4 5.6l-8.8 8.8" stroke="currentColor" stroke-width="2.6" stroke-linecap="round"/></svg></div>
      </div>

      <div style="display:flex;align-items:center;gap:8px;margin-top:14px">
        <span class="sub" style="font-size:12px">休息结束时会原样还给你</span>
        <span style="flex:1"></span>
        <span class="kbd">↵</span>
        <span class="sub" style="font-size:11.5px;color:#AEB5BC">保存</span>
      </div>

      <div style="display:flex;align-items:center;justify-content:flex-end;gap:12px;margin-top:30px">
        <button class="btn btn-text">跳过</button>
        <button class="btn btn-primary" style="height:42px;padding:0 26px">保存并休息</button>
      </div>
    </div>
  </div>'''
    return doc("03 记录 Intent", scene(inner))

# ================================================================ 04 休息结束
def s04():
    inner = f'''
  <div style="position:absolute;inset:0;display:flex;flex-direction:column;align-items:center;justify-content:center">
    <div style="display:flex;align-items:center;gap:11px;margin-bottom:30px">
      <span style="display:flex;width:22px;height:22px;border-radius:50%;background:rgba(52,133,92,.12);color:#34855C;align-items:center;justify-content:center">{IC_CHECK}</span>
      <span style="font-size:24px;font-weight:600;letter-spacing:-.02em;color:#0B0D10">欢迎回来</span>
    </div>

    <div class="glass-soft" style="width:470px;border-radius:20px;padding:24px 26px">
      <div class="kicker" style="font-size:10px;letter-spacing:.24em;color:#9AA3AB;margin-bottom:12px">休息前你准备继续</div>
      <div style="font-size:20px;font-weight:600;letter-spacing:-.016em;color:#0B0D10;line-height:1.4">完成 Auth 模块测试</div>
    </div>

    <div style="display:flex;align-items:center;gap:9px;margin-top:24px">
      <span class="sub num">本次休息 5 分钟</span>
      <span style="width:3px;height:3px;border-radius:50%;background:#C8CED4"></span>
      <span class="sub">活动打卡已完成</span>
    </div>

    <button class="btn btn-primary" style="margin-top:32px;height:46px;padding:0 62px;font-size:15px">开始</button>
  </div>'''
    return doc("04 休息结束", scene(inner))

# ================================================================ 05 菜单栏主面板
def s05():
    def card(cls, ic, name, status, ok=False):
        bg = "rgba(52,133,92,.055)" if ok else "rgba(255,255,255,.62)"
        bd = "rgba(52,133,92,.16)" if ok else "rgba(255,255,255,.8)"
        st = "color:#34855C" if ok else "color:#8C939B"
        return f'''<div style="display:flex;align-items:center;gap:11px;padding:13px 14px;border-radius:14px;
                    background:{bg};border:1px solid {bd};box-shadow:inset 0 1px 0 rgba(255,255,255,.7)">
        <div class="tile {cls}" style="width:32px;height:32px;border-radius:10px">{ic}</div>
        <div style="line-height:1.35">
          <div style="font-size:13.5px;font-weight:500;color:#20262D">{name}</div>
          <div class="num sub" style="margin-top:1px;{st}">{status}</div>
        </div>
      </div>'''

    panel = f'''
      <!-- 状态头 -->
      <div style="padding:18px 20px 15px;display:flex;align-items:center">
        <div style="flex:1;display:flex;align-items:center;gap:10px">
          <div class="tile t-rest" style="width:30px;height:30px;border-radius:10px">{IC_BRAND}</div>
          <div style="font-size:15px;font-weight:600;letter-spacing:-.01em">Tacet</div>
        </div>
        <div style="display:flex;flex-direction:column;align-items:flex-end;line-height:1.15">
          <div style="font-size:10px;font-weight:600;color:#AEB5BC">连续工作</div>
          <div class="num" style="font-size:21px;font-weight:600;letter-spacing:-.02em;color:#0C6E66;margin-top:3px">1h 32m</div>
        </div>
      </div>
      <div class="hair"></div>

      <!-- 四类健康 -->
      <div style="padding:14px;display:grid;grid-template-columns:1fr 1fr;gap:10px">
        {card("t-rest", IC_CUP, "休息", "距提醒 18 分钟")}
        {card("t-water", IC_DROP, "喝水", "距提醒 12 分钟")}
        {card("t-move", IC_WALK, "活动", "距提醒 42 分钟")}
        {card("t-ok", IC_EYE, "护眼", "今日已达标", ok=True)}
      </div>

      <!-- 操作 -->
      <div style="padding:4px 14px 16px;display:flex;flex-direction:column;gap:9px">
        <button class="btn btn-primary" style="width:100%;height:42px;font-size:14.5px">现在休息</button>
        <div style="display:flex;gap:9px">
          <button class="btn btn-ghost" style="flex:1;height:38px">+1 杯水</button>
          <button class="btn btn-ghost" style="flex:1;height:38px">活动打卡</button>
        </div>
      </div>

      <div class="hair"></div>
      <div style="padding:12px 20px 14px;display:flex;align-items:center;gap:11px;font-size:12.5px;color:#8C939B">
        <span>设置</span><span style="color:#D2D8DD">·</span>
        <span>暂停计时</span><span style="color:#D2D8DD">·</span>
        <span>退出</span>
      </div>'''

    inner = f'''
  {menubar(f'<span style="display:flex;width:16px;color:#0C6E66">{IC_BRAND}</span><span class="num" style="font-weight:600;color:#0C6E66">1h 32m</span>')}
  <div style="position:absolute;top:28px;right:16px;width:376px">
    <div class="glass" style="border-radius:20px;border-top-right-radius:8px;overflow:hidden">{panel}</div>
  </div>
  <div class="sub" style="position:absolute;left:48px;bottom:56px;max-width:300px;line-height:1.7">
    菜单栏下拉主面板<br>四类健康状态一眼可见，常用操作一步可达
  </div>'''
    return doc("05 菜单栏主面板", scene(inner, veil=False))

# ================================================================ 06 菜单栏状态
def s06():
    states = [
        ("静默", f'<span style="display:flex;width:16px;color:#9AA3AB">{IC_BRAND}</span>', "", "需求低或刚休息完，只留一个安静的小图标"),
        ("工作中", f'<span style="display:flex;width:16px;color:#0C6E66">{IC_BRAND}</span><span class="num" style="font-weight:600;color:#0C6E66">1h 32m</span>', "color:#0C6E66", "显示连续工作计时，不发声、不弹窗"),
        ("喝水临近", f'<span style="display:flex;width:16px;color:#1C6EB0">{IC_DROP}</span><span class="num" style="font-weight:600;color:#1C6EB0">12m</span>', "color:#1C6EB0", "把「快要提醒了」提前放出来"),
        ("勿扰", f'<span style="display:flex;width:16px;color:#C8CED4">{IC_BRAND}</span><span style="color:#C8CED4">勿扰</span>', "color:#C8CED4", "用户主动开启，期间不做任何提示"),
    ]
    rows = ""
    for i, (label, right, style, note) in enumerate(states):
        rows += f'''<div>
        <div style="display:flex;align-items:center;margin-bottom:7px">
          <span style="font-size:12.5px;font-weight:600;color:#20262D">{label}</span>
          <span class="hair" style="flex:1;margin-left:12px"></span>
        </div>
        <div style="display:flex;justify-content:flex-end">
          <div style="display:flex;align-items:center;gap:16px;padding:5px 12px;border-radius:8px;
                      background:rgba(255,255,255,.72);border:1px solid rgba(11,13,16,.06)">
            <span style="display:flex;gap:12px;opacity:.75">{IC_CTRL}{IC_WIFI}</span>
            <span style="display:flex;width:24px;opacity:.75">{IC_BATT}</span>
            <span style="width:1px;height:13px;background:rgba(11,13,16,.10)"></span>
            <span style="display:flex;align-items:center;gap:7px;font-size:12.5px;{style}">{right}</span>
          </div>
        </div>
        <div class="sub" style="margin-top:6px;font-size:11.5px">{note}</div>
      </div>'''

    inner = f'''
  {menubar(f'<span style="display:flex;width:16px;color:#0C6E66">{IC_BRAND}</span><span class="num" style="font-weight:600;color:#0C6E66">1h 32m</span>')}
  <div style="position:absolute;top:96px;left:50%;transform:translateX(-50%);width:452px">
    <div class="glass" style="border-radius:20px;padding:26px 28px;display:flex;flex-direction:column;gap:22px">{rows}</div>
  </div>
  <div class="sub" style="position:absolute;left:48px;bottom:56px;max-width:300px;line-height:1.7">
    菜单栏常驻状态<br>Level 1 环境级提示：图标本身即是提醒
  </div>'''
    return doc("06 菜单栏状态", scene(inner, veil=False))

# ================================================================ 07 系统通知
def s07():
    def notif(cls, ic, title, body, acts, primary_first=True):
        btns = ""
        for i, t in enumerate(acts):
            if i == 0:
                btns += f'<button class="btn btn-primary" style="flex:1;height:34px;font-size:13.5px;border-radius:10px;box-shadow:0 4px 12px -6px rgba(12,110,102,.5)">{t}</button>'
            else:
                btns += f'<button class="btn btn-quiet" style="flex:1;height:34px;font-size:13.5px;border-radius:10px">{t}</button>'
        return f'''<div class="glass" style="width:410px;border-radius:20px;padding:16px 17px 15px">
        <div style="display:flex;align-items:center;gap:11px;margin-bottom:12px">
          <div class="tile {cls}" style="width:31px;height:31px;border-radius:9px">{ic}</div>
          <div style="font-size:12.5px;font-weight:600;color:#5A626B;letter-spacing:.01em">Tacet</div>
          <div style="flex:1"></div>
          <div class="sub" style="font-size:11.5px">现在</div>
        </div>
        <div style="font-size:15px;font-weight:600;letter-spacing:-.012em;color:#0B0D10">{title}</div>
        <div style="font-size:13.5px;color:#5A626B;line-height:1.55;margin-top:5px;margin-bottom:14px">{body}</div>
        <div style="display:flex;gap:9px">{btns}</div>
      </div>'''

    inner = f'''
  <div style="position:absolute;top:64px;right:56px;width:410px;display:flex;flex-direction:column;gap:20px">
    <div>
      <div class="kicker" style="font-size:10px;letter-spacing:.22em;margin-bottom:11px;color:#AEB5BC">休息场景</div>
      {notif("t-rest", IC_CUP, "建议休息一下", "你已经连续工作 78 分钟了。", ["现在休息", "3 分钟后"])}
    </div>
    <div>
      <div class="kicker" style="font-size:10px;letter-spacing:.22em;margin-bottom:11px;color:#AEB5BC">喝水场景</div>
      {notif("t-water", IC_DROP, "如果方便，记得喝点水", "距离上次喝水已经 45 分钟了。", ["已喝水", "稍后"])}
    </div>
  </div>

  <div style="position:absolute;left:64px;bottom:112px;width:380px">
    <div style="font-size:26px;font-weight:600;letter-spacing:-.022em;line-height:1.35;color:#0B0D10">
      通知里永远有一个<br>「稍后」的出口
    </div>
    <div class="sub" style="margin-top:14px;line-height:1.75">
      Level 2 用于不宜强打断的时刻：会议、全屏、心流。<br>
      提醒可以不看，但绝不逼你立刻放下手上的事。
    </div>
  </div>'''
    return doc("07 系统通知", scene(inner, veil=False))

# ================================================================ 08 设置页
def s08():
    def rem(cls, ic, label, interval):
        return f'''<div class="row">
        <div class="tile {cls}" style="width:30px;height:30px;border-radius:9px">{ic}</div>
        <div class="lab">{label}</div>
        <div class="ctl">
          <div class="sw on"><i></i></div>
          <div class="pill num">{interval}<span>分钟</span></div>
        </div>
      </div>'''

    def tog(label, on, note=""):
        n = f'<span class="sub" style="font-size:12px;margin-left:4px">{note}</span>' if note else ""
        c = "sw on" if on else "sw"
        return f'''<div class="row">
        <div class="lab">{label}{n}</div>
        <div class="ctl"><div class="{c}"><i></i></div></div>
      </div>'''

    group = lambda title, body: f'''<div>
      <div style="font-size:11.5px;font-weight:600;letter-spacing:.04em;color:#9AA3AB;margin:0 0 9px 4px">{title}</div>
      <div style="border-radius:14px;overflow:hidden;background:rgba(255,255,255,.66);
                  border:1px solid rgba(255,255,255,.8);box-shadow:inset 0 1px 0 rgba(255,255,255,.8)">{body}</div>
    </div>'''

    rows_rem = (rem("t-rest", IC_CUP, "休息提醒", "50") + '<div class="hair" style="margin-left:58px"></div>' +
                rem("t-water", IC_DROP, "喝水提醒", "45") + '<div class="hair" style="margin-left:58px"></div>' +
                rem("t-move", IC_WALK, "活动提醒", "60") + '<div class="hair" style="margin-left:58px"></div>' +
                rem("t-eye", IC_EYE, "护眼提醒", "40"))
    rows_dnd = tog("勿扰模式", False) + '<div class="hair" style="margin-left:16px"></div>' + tog("仅在工作时间提醒", False, "v0.2 提供")

    about = f'''<div class="row">
        <div class="lab">关于 Tacet</div>
        <div class="ctl"><span class="sub num">v0.1.0</span><span style="display:flex;width:15px;color:#C2C8CE">{IC_CHEV}</span></div>
      </div>'''
    rows_gen = tog("开机自动启动", True) + '<div class="hair" style="margin-left:16px"></div>' + about

    ai = f'''<div class="row" style="padding:15px 16px">
        <div style="flex:1;line-height:1.45">
          <div style="font-size:13.5px;color:#3A424A">AI 助手（可选）</div>
          <div class="sub" style="font-size:12px;margin-top:3px">不配置也能完整使用 Tacet</div>
        </div>
        <div class="ctl"><span style="display:flex;width:15px;color:#C2C8CE">{IC_CHEV}</span></div>
      </div>'''

    win = f'''<div class="glass" style="width:700px;border-radius:22px;overflow:hidden">
      <div style="padding:16px 20px;display:flex;align-items:center;border-bottom:1px solid rgba(11,13,16,.055)">
        <div style="display:flex;gap:8px;width:56px"><i style="width:12px;height:12px;border-radius:50%;background:#FF5F57;display:block"></i><i style="width:12px;height:12px;border-radius:50%;background:#FEBC2E;display:block"></i><i style="width:12px;height:12px;border-radius:50%;background:#28C840;display:block"></i></div>
        <div style="flex:1;display:flex;justify-content:center;font-size:13.5px;font-weight:600">设置</div>
        <div style="width:56px"></div>
      </div>
      <div style="padding:24px 26px 26px;display:flex;flex-direction:column;gap:22px">
        {group("提醒", rows_rem)}
        {group("勿扰", rows_dnd)}
        {group("通用", rows_gen)}
        {group("AI 助手", ai)}
      </div>
    </div>'''

    inner = f'''
  <div style="position:absolute;inset:0;display:flex;align-items:center;justify-content:center">
    <div style="display:flex;flex-direction:column;align-items:center">
      {win}
      <div class="sub" style="margin-top:22px;font-size:12px">AI 入口静默存在，无红点、无弹窗、不阻塞任何流程</div>
    </div>
  </div>'''
    return doc("08 设置页", scene(inner))

# ================================================================ 09 连续跳过 → 主动收手
#
# 为什么这一屏不能做成「全屏宣告」：
# 用户已经连跳三次，说明他此刻有明显更重要的事。这时候弹全屏告诉他
# 「我要收手了」，本身就是第四次打扰 —— 用一个打扰去承诺不再打扰，
# 逻辑上就错了。
# 所以收手只用一条系统通知（Level 2），说完就彻底安静。
# 这一屏同时把「不会发生的事」逐条列出来 ——「不惩罚」这件事，
# 只有说清楚才可信。
def s09():
    WONT = [
        ("不扣分", "没有健康分、没有连续天数，跳过不产生任何记录"),
        ("不降权重", "下次该提醒还是照常提醒，不因为跳过而变更间隔"),
        ("不追问", "不会问你「为什么又跳过」，也不做跳过的原因归因"),
    ]
    wh = ""
    for t, d in WONT:
        # 用 flex + gap 做间隔，不用全角空格 —— Ardot 转换器不认全角空格，
        # 会把「不扣分」和后面的说明挤成一整行。
        wh += f'''<div style="display:flex;align-items:flex-start;gap:11px">
        <span style="display:flex;width:15px;height:15px;margin-top:2px;color:#8FA6A2;flex:0 0 auto">{IC_CHECK}</span>
        <div style="display:flex;align-items:baseline;gap:14px;line-height:1.5">
          <span style="font-size:13px;color:#3A424A;font-weight:500;flex:0 0 auto;min-width:54px">{t}</span>
          <span class="sub" style="font-size:12.5px">{d}</span>
        </div>
      </div>'''

    notif = f'''<div class="glass" style="width:430px;border-radius:20px;padding:16px 17px 15px">
        <div style="display:flex;align-items:center;gap:11px;margin-bottom:12px">
          <div class="tile t-rest" style="width:31px;height:31px;border-radius:9px">{IC_BRAND}</div>
          <div style="font-size:12.5px;font-weight:600;color:#5A626B;letter-spacing:.01em">Tacet</div>
          <div style="flex:1"></div>
          <div class="sub" style="font-size:11.5px">现在</div>
        </div>
        <div style="font-size:15px;font-weight:600;letter-spacing:-.012em;color:#0B0D10">接下来 2 小时不再提醒休息</div>
        <div style="font-size:13.5px;color:#5A626B;line-height:1.55;margin-top:5px;margin-bottom:14px">
          你连着跳过了 3 次，我先不打扰了。<br>忙完随时可以手动开始。
        </div>
        <div style="display:flex;gap:9px">
          <button class="btn btn-quiet" style="flex:1;height:34px;font-size:13.5px;border-radius:10px">知道了</button>
          <button class="btn btn-quiet" style="flex:1;height:34px;font-size:13.5px;border-radius:10px">恢复提醒</button>
        </div>
      </div>'''

    inner = f'''
  <!-- 刻意留大片空白 —— 这一屏要传达的是「安静」，用密度去填满会适得其反。 -->
  <div style="position:absolute;left:96px;top:208px;width:520px">
    <div class="kicker" style="font-size:10px;letter-spacing:.26em;color:#AEB5BC;margin-bottom:20px">原则 10 · 不惩罚</div>
    <div style="font-size:27px;font-weight:600;letter-spacing:-.02em;color:#0B0D10;line-height:1.42;margin-bottom:16px">
      连续跳过三次<br>它主动收手
    </div>
    <div class="sub" style="font-size:13.5px;line-height:1.78;color:#8C939B">
      跳过是用户的正当权利，不是需要被纠正的行为。<br>
      所以 Tacet 的回应是退开一步，而不是加强提醒。
    </div>
  </div>

  <div style="position:absolute;left:96px;top:496px;width:520px">
    <div style="display:flex;align-items:center;gap:10px;margin-bottom:18px">
      <span class="kicker" style="font-size:10px;letter-spacing:.24em;color:#B9BFC6">收手期间，它不会</span>
      <span class="hair" style="flex:1;margin-left:12px"></span>
    </div>
    <div style="display:flex;flex-direction:column;gap:15px">{wh}</div>
  </div>

  <div style="position:absolute;left:96px;top:722px;width:520px">
    <div style="display:flex;align-items:center;gap:10px;margin-bottom:13px">
      <span class="kicker" style="font-size:10px;letter-spacing:.22em;color:#C8CED4">记录仅用于决定何时收手</span>
      <span class="hair" style="flex:1;margin-left:12px"></span>
    </div>
    <div class="sub" style="font-size:12px;line-height:1.7;color:#AEB5BC">
      跳过 3 次 · 分别发生在工作 62 / 78 / 95 分钟时 · 不进入任何统计
    </div>
  </div>

  <div style="position:absolute;top:176px;right:96px;width:410px">
    <div class="sub" style="font-size:11px;letter-spacing:.06em;color:#B9BFC6;margin-bottom:11px;text-align:right">收手通知 · 整个收手期只发这一次</div>
    {notif}
    <div class="sub" style="font-size:11.5px;margin-top:15px;color:#AEB5BC;line-height:1.72;text-align:right">
      为什么不弹全屏：你已经连跳三次，说明此刻有更要紧的事。<br>
      用一个打扰去承诺不再打扰，是自相矛盾的。
    </div>
  </div>'''
    return doc("09 连续跳过 · 主动收手", scene(inner))


# ================================================================ 10 AI 缺席对照
#
# 这一屏是给评审看的「原则证明页」。
#
# 背景：直觉上容易认为「AI 没配置」算异常态、该给用户提示。但产品原则 7
# 的硬约束恰恰相反 —— 未配置 AI 时，那个入口必须**静默存在**：
# 无红点、无提示、不阻塞任何流程。
# 所以这里刻意不做「降级提示」，而是把两种状态并排放出来，让人一眼看到
# 提醒体验完全一致，唯一差别只是设置页里一行状态文字。
# （做成了对照页，而不是提示弹窗 —— 后者会自相矛盾。）
def s10():
    def ai_row(configured):
        if configured:
            right = f'''<span class="sub num" style="font-size:12px;color:#8C939B">已连接 · 本地模型</span><span style="display:flex;width:15px;color:#C2C8CE">{IC_CHEV}</span>'''
        else:
            right = f'''<span style="display:flex;width:15px;color:#C2C8CE">{IC_CHEV}</span>'''
        return f'''<div class="row" style="padding:15px 16px">
        <div style="flex:1;line-height:1.45">
          <div style="font-size:13.5px;color:#3A424A">AI 助手（可选）</div>
          <div class="sub" style="font-size:12px;margin-top:3px">不配置也能完整使用 Tacet</div>
        </div>
        <div class="ctl">{right}</div>
      </div>'''

    def col(configured, label, tag_color, note):
        return f'''<div style="flex:1">
        <div style="display:flex;align-items:center;gap:9px;margin-bottom:12px">
          <span style="font-size:12.5px;font-weight:600;color:{tag_color}">{label}</span>
          <span class="hair" style="flex:1"></span>
        </div>
        <div style="border-radius:14px;overflow:hidden;background:rgba(255,255,255,.66);
                    border:1px solid rgba(255,255,255,.8);box-shadow:inset 0 1px 0 rgba(255,255,255,.8)">
          {ai_row(configured)}
        </div>
        <div class="sub" style="font-size:12px;margin-top:11px;line-height:1.6">{note}</div>
      </div>'''

    same = [
        "四类提醒的触发、间隔、文案",
        "休息全屏、Intent 记录与恢复",
        "菜单栏状态、系统通知",
        "设置页的其余每一项",
    ]
    sh = "".join(f'''<div style="display:flex;align-items:center;gap:11px">
      <span style="display:flex;width:15px;height:15px;color:#34855C;flex:0 0 auto">{IC_CHECK}</span>
      <span style="font-size:13px;color:#3A424A">{t}　<span class="sub" style="font-size:12px">完全一致</span></span>
    </div>''' for t in same)

    inner = f'''
  <div style="position:absolute;inset:0;display:flex;flex-direction:column;align-items:center;justify-content:center">
    <div class="kicker" style="font-size:10px;letter-spacing:.26em;color:#AEB5BC;margin-bottom:16px">原则 7 · AI 完全可选</div>
    <div style="font-size:26px;font-weight:600;letter-spacing:-.02em;color:#0B0D10;margin-bottom:10px">AI 是否配置，不影响任何提醒</div>
    <div class="sub" style="font-size:13.5px;margin-bottom:38px">所以「未配置」不是异常态，也不该有任何提示</div>

    <div style="display:flex;gap:22px;width:720px">
      {col(False, "未配置 AI", "#8C939B", "没有状态文字，也没有红点或提示")}
      {col(True,  "已配置 AI", "#0C6E66", "只多一行状态文字 —— 仅此而已")}
    </div>

    <div style="width:720px;margin-top:34px">
      <div style="display:flex;align-items:center;gap:10px;margin-bottom:18px">
        <span class="kicker" style="font-size:10px;letter-spacing:.24em;color:#B9BFC6">两条路径完全相同</span>
        <span class="hair" style="flex:1"></span>
      </div>
      <div style="display:grid;grid-template-columns:1fr 1fr;gap:14px 26px">{sh}</div>
    </div>
  </div>

  <div class="sub" style="position:absolute;left:50%;transform:translateX(-50%);bottom:52px;font-size:11.5px;color:#AEB5BC;letter-spacing:.02em;text-align:center">
    v0.1 不含 AI 功能 · 此页用于验证「没有 AI 也完整」这条硬约束
  </div>'''
    return doc("10 AI 缺席对照", scene(inner))


# ================================================================ 00 设计封面 · 五层干预体系
def s00():
    LEVELS = [
        ("0", "Silent", "什么都不做", "需求低、刚休息完、正在开会", True),
        ("1", "Ambient", "菜单栏图标自己变", "常驻显示，不发声、不弹窗", True),
        ("2", "Notification", "一条系统通知", "带「稍后」，随时可以不看", True),
        ("3", "Floating Card", "浮在窗口边的小卡片", "比通知更近，但依然不挡操作", False),
        ("4", "Full Screen", "全屏毛玻璃覆盖", "只在真正该休息时，永不锁屏", True),
        ("5", "Escalated", "升级提醒", "多次忽略后的更强提示（需用户开启）", False),
    ]
    rows = ""
    for i, (lv, name, what, when, delivered) in enumerate(LEVELS):
        badge = (
            '<span style="display:inline-flex;align-items:center;height:20px;padding:0 8px;border-radius:6px;'
            'background:rgba(12,110,102,.09);color:#0C6E66;font-size:10.5px;font-weight:600;letter-spacing:.02em">v0.1</span>'
            if delivered else
            '<span style="display:inline-flex;align-items:center;height:20px;padding:0 8px;border-radius:6px;'
            'background:rgba(11,13,16,.05);color:#AEB5BC;font-size:10.5px;font-weight:600;letter-spacing:.02em">v0.2</span>'
        )
        dim = "" if delivered else "opacity:.48"
        rows += f'''<div style="display:flex;align-items:center;gap:16px;padding:14px 18px;{dim}">
        <span class="num" style="flex:0 0 auto;width:20px;font-size:15px;font-weight:600;color:#0C6E66">{lv}</span>
        <span style="flex:0 0 auto;width:120px;font-size:13.5px;font-weight:600;color:#20262D">{name}</span>
        <span style="flex:1;font-size:13px;color:#4B525A">{what}</span>
        <span class="sub" style="flex:0 0 auto;width:238px;font-size:12px">{when}</span>
        <span style="flex:0 0 auto">{badge}</span>
      </div>'''
        if i < len(LEVELS) - 1:
            rows += '<div class="hair" style="margin-left:54px"></div>'

    inner = f'''
  <div style="position:absolute;inset:0;display:flex;flex-direction:column;justify-content:center;padding:0 104px">
    <div style="display:flex;align-items:flex-end;gap:20px;margin-bottom:54px">
      <div style="display:flex;align-items:center;gap:14px">
        <span style="display:flex;width:30px;color:#0C6E66">{IC_BRAND}</span>
        <span style="font-size:34px;font-weight:600;letter-spacing:-.02em;color:#0B0D10">Tacet</span>
      </div>
      <span class="sub" style="font-size:15px;color:#8C939B;padding-bottom:6px">此刻，我知道该我安静了</span>
      <span style="flex:1"></span>
      <span class="sub" style="font-size:12.5px;color:#AEB5BC;padding-bottom:7px">v0.1 界面原型 · 1440×900</span>
    </div>

    <div style="display:flex;align-items:center;gap:10px;margin-bottom:14px">
      <span class="kicker" style="font-size:10px;letter-spacing:.26em;color:#B9BFC6">五层干预体系</span>
      <span class="hair" style="flex:1"></span>
      <span class="sub" style="font-size:11.5px;color:#B9BFC6">健康需求 × 可打扰度 → 干预决策</span>
    </div>

    <div class="glass" style="border-radius:18px;overflow:hidden;padding:6px 0">{rows}</div>

    <div class="sub" style="margin-top:26px;font-size:12px;color:#AEB5BC;line-height:1.7">
      层级越往下，打扰越强 —— 所以只有上一层解决不了问题时，才会动用下一层。<br>
      任何一层都可跳过或延后；永远不锁屏，永远不惩罚。
    </div>
  </div>'''
    return doc("00 设计封面 · 五层干预体系", scene(inner))


PAGES = [
    ("00 设计封面", s00()),
    ("01 全屏休息提醒", s01()),
    ("02 休息中", s02()),
    ("03 记录 Intent", s03()),
    ("04 休息结束", s04()),
    ("05 菜单栏主面板", s05()),
    ("06 菜单栏状态", s06()),
    ("07 系统通知", s07()),
    ("08 设置页", s08()),
    ("09 连续跳过 · 主动收手", s09()),
    ("10 AI 缺席对照", s10()),
]

if __name__ == "__main__":
    for name, html in PAGES:
        with open(os.path.join(OUT, name + ".html"), "w", encoding="utf-8") as fh:
            fh.write(html)
    # 总览
    cards = "".join(
        f'<div class="c"><div class="t">{n}</div>'
        f'<iframe src="{n}.html" width="1440" height="900" style="transform:scale(.30);transform-origin:0 0;'
        f'width:1440px;height:900px;border:none;display:block"></iframe></div>'
        for n, _ in PAGES)
    index = f'''<!DOCTYPE html><html lang="zh-CN"><head><meta charset="utf-8"><title>Tacet v0.1 · 设计总览</title>
<style>body{{margin:0;padding:26px;background:#DDE1E5;font-family:-apple-system,"PingFang SC",sans-serif}}
h1{{font-size:16px;margin:0 0 4px}}p{{font-size:12.5px;color:#5A626B;margin:0 0 20px}}
.grid{{display:grid;grid-template-columns:repeat(4,max-content);gap:18px}}
.c{{background:#fff;border-radius:12px;overflow:hidden;box-shadow:0 6px 20px rgba(0,0,0,.14)}}
.c .t{{font-size:11.5px;font-weight:600;color:#3A424A;padding:8px 11px;border-bottom:1px solid #EEF1F3}}
.c div{{position:relative}}</style></head><body>
<h1>Tacet（休止）v0.1 · 界面原型</h1>
<p>全屏白色毛玻璃设计语言 · 11 屏 · 1440×900</p>
<div class="grid">{cards}</div></body></html>'''
    with open(os.path.join(OUT, "index.html"), "w", encoding="utf-8") as fh:
        fh.write(index)
    print(f"已生成 {len(PAGES)} 屏 + 总览：")
    for n, h in PAGES:
        print(f"  {n}.html  {len(h):>6,} bytes")
