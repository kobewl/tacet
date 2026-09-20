// ESLint 配置（Flat Config，ESLint 9 的写法）
//
// 研发规范 §7 要求「前端 lint 必查」，并且 §3.3 里有几条约定
// 值得用规则**强制**住，而不是靠代码评审时的记忆力：
//
// - 禁止 `any`（§3.2 类型约定）→ @typescript-eslint/no-explicit-any
// - 业务真值必须在 Rust 侧（§3.2 状态约定）→ 见下方 no-restricted-syntax
// - React Hooks 规则 → eslint-plugin-react-hooks
//
// 我们刻意**不**引入完整版 Airbnb 风格规则集：那是几百条风格偏好，
// 会把真正重要的错误淹没在格式化噪音里。只留能拦住真 bug 的规则。

import js from "@eslint/js";
import globals from "globals";
import reactHooks from "eslint-plugin-react-hooks";
import reactRefresh from "eslint-plugin-react-refresh";
import tseslint from "typescript-eslint";

export default tseslint.config(
  // 构建产物不检查
  { ignores: ["dist/**", "node_modules/**"] },

  {
    files: ["**/*.{ts,tsx}"],

    extends: [
      js.configs.recommended,
      // recommendedTypeChecked 会做类型感知的检查（需要 tsconfig）。
      // 它比纯语法检查多抓一类真 bug：比如 await 一个非 Promise、
      // 或者把 string 传给要求 number 的函数。
      ...tseslint.configs.recommendedTypeChecked,
    ],

    languageOptions: {
      ecmaVersion: 2022,
      globals: globals.browser,
      parserOptions: {
        project: ["./tsconfig.json"],
        tsconfigRootDir: import.meta.dirname,
      },
    },

    plugins: {
      "react-hooks": reactHooks,
      "react-refresh": reactRefresh,
    },

    rules: {
      ...reactHooks.configs.recommended.rules,
      "react-refresh/only-export-components": [
        "warn",
        { allowConstantExport: true },
      ],

      // ── 研发规范 §3.2：禁止 any ──
      //
      // `any` 会让 TypeScript 的类型检查在这条路径上完全失效。
      // 需要「未知类型」时用 `unknown` + 类型收窄 —— 后者会强制
      // 你在使用前先证明它是什么，这才是类型安全的意义。
      "@typescript-eslint/no-explicit-any": "error",

      // ── 前端不得持有业务真值（规范 §3.2）──
      //
      // 业务真值（当前状态、需求分数、统计数字）全部由 Rust 侧持有，
      // 前端只持有 UI 状态（弹窗开没开、表单草稿）。
      //
      // 这条规则拦住最常见的越界写法：在前端用 localStorage 存业务数据。
      // 那会导致「后端重启后数据不一致」这类非常难查的 bug。
      "no-restricted-syntax": [
        "error",
        {
          selector:
            "MemberExpression[object.name='localStorage'][property.name='setItem']",
          message:
            "业务真值必须由 Rust 侧持有（研发规范 §3.2）。前端只用 localStorage 存纯 UI 偏好，不要存业务数据。",
        },
      ],

      // 未使用的变量：允许以 _ 开头显式忽略
      "@typescript-eslint/no-unused-vars": [
        "error",
        {
          argsIgnorePattern: "^_",
          varsIgnorePattern: "^_",
          caughtErrorsIgnorePattern: "^_",
        },
      ],

      // 空函数允许（事件处理器占位很常见）
      "@typescript-eslint/no-empty-function": "off",
    },
  },

  // 测试与配置文件放宽
  {
    files: ["**/*.config.{ts,js}", "vite.config.ts"],
    ...tseslint.configs.disableTypeChecked,
  },
);
