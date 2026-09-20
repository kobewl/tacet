import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

/**
 * Vite 配置。
 *
 * 几个与 Tauri 配合的关键点：
 *
 * 1. **固定端口 5173 且 `strictPort: true`**：Tauri 的开发模式会按
 *    `tauri.conf.json` 里写死的地址去连前端。如果端口被占用后 Vite
 *    默默换到 5174，Tauri 就白屏了 —— 不如直接报错。
 * 2. **`clearScreen: false`**：别把 Rust 的编译输出冲掉，
 *    调试时两边的日志混在一起看更方便。
 * 3. **build.target 用 safari15**：Tauri 在 macOS 上用系统的 WKWebView，
 *    不需要为老浏览器做兼容。
 */
export default defineConfig({
  plugins: [react()],

  // Tauri 需要一个固定端口
  server: {
    port: 5173,
    strictPort: true,
    clearScreen: false,
  },

  build: {
    // 产物交给 Rust 侧打包，输出到 dist/
    outDir: "dist",
    emptyOutDir: true,
    // macOS 上的 WKWebView 支持现代语法
    target: "safari15",
    // 桌面应用不存在「首屏加载慢」的带宽问题，
    // 关掉 chunk 分割能让产物更好调试
    rollupOptions: {
      output: {
        manualChunks: undefined,
      },
    },
  },

  // 与 Rust 那边的 tauri.conf.json 保持一致的固定产物名
  base: "./",
});
