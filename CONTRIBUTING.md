# 贡献指南

创建于 2026-03-22

感谢你关注 **清衡 · System Balance**。欢迎通过 Issue、讨论和 Pull Request 参与改进。

## 开始之前

1. 先搜索 [Issues](https://github.com/AndroidSix/mac-space/issues)，确认问题或想法尚未被提出。
2. 较大改动请先开 Issue 说明动机与方案，避免方向不一致。
3. 安全相关漏洞请按 [SECURITY.md](SECURITY.md) 私下报告，不要公开 Issue。

## 开发环境

需要：

- Node.js 22.12+
- Rust stable
- macOS：Xcode Command Line Tools
- Windows：MSVC C++ 构建工具与 WebView2

```sh
npm ci
npm run tauri -- dev
npm run check
npm test
npm run test:native -- --release
```

当前仓库的 Rust 工具链可放在 `.tools/`；`npm` 包装脚本会自动选用，不改系统 PATH。其他机器使用本机已安装的 Rust 即可。

## 建议的改动方式

- **Bug**：尽量写清复现步骤、系统版本、期望与实际结果；能附最小复现更好。
- **功能**：说明使用场景与边界；本工具默认不自动扫描全盘、不常驻服务、不强制结束进程，请保持这一产品边界。
- **文档**：README / API.md / 注释与界面文案保持中英可用时一并更新。
- **测试**：前端改动跑 `npm test` 与 `npm run check`；涉及原生逻辑再跑 `npm run test:native`。原生测试只操作临时目录与测试进程，不要依赖真实缓存或改动机器电源模式。

## Pull Request

1. 从最新 `main` 拉分支。
2. 保持改动聚焦；无关格式化、重构请拆开。
3. 提交说明写清「改了什么、为什么」。
4. 确保本地检查通过：`npm run check`、`npm test`；触及 Rust 时再跑 Clippy / 原生测试。
5. PR 描述里关联相关 Issue，并注明是否影响 macOS、Windows 或双端。

## 代码风格

- 前端：TypeScript + Svelte 5；可用仓库内 Prettier 配置格式化。
- 后端：Rust（Tauri 2）；优先清晰命名，少写注释堆砌。
- 不要提交 `node_modules/`、`dist/`、`.tools/`、`src-tauri/target/`、`release/` 或本机扫描数据。

## 行为约定

请保持友善、就事论事。辱骂、骚扰或恶意利用本项目讨论渠道的行为不可接受。
