# 清衡 · System Balance

创建于 2026-11-11 · 更新于 2026-03-22

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![GitHub release](https://img.shields.io/github/v/release/AndroidSix/mac-space)](https://github.com/AndroidSix/mac-space/releases)
[![Platform](https://img.shields.io/badge/platform-macOS%20%7C%20Windows-blue)](https://github.com/AndroidSix/mac-space)

面向 macOS 与 Windows 的本机磁盘、负载与电源管理工具，使用 Tauri 2、Rust 和 Svelte 5。当前版本 **0.4.0**，原名 MacSpace。默认跟随系统语言，支持简体中文与 English，包含原生菜单和确认弹窗。

本仓库以 **MIT** 许可证开源。欢迎报告问题、提交改进，详见 [CONTRIBUTING.md](CONTRIBUTING.md)。

## 安装与使用

- macOS：从 Releases 下载 `SystemBalance-0.4.0-macos-arm64.pkg`，安装到 `/Applications/System Balance.app`，适用于 Apple Silicon、macOS 12+。也可以独立运行 `release/System Balance.app`。
- Windows：从 Releases 下载 Windows x64 的 NSIS `.exe` 安装器，适用于 Windows 10/11 x64，使用系统 WebView2。安装过程可选择中文或英文。未发布 Windows ARM64 原生包。
- 保留原应用标识 `local.macspace.desktop`、偏好键和 GitHub 仓库地址，延续已有数据与更新入口。旧的 `MacSpace.app` 不会被新路径覆盖；升级后请使用 System Balance，避免同时运行两个版本。
- 应用不安装常驻管理服务，不自动扫描全盘、不自动清理或结束进程。

## 功能

- **一键智能清理**：总览与清理页进入，先扫描、再勾选、最后确认。默认推荐影响较小的下载和更新缓存。构建、网页、uv 与 Playwright 缓存默认不选；使用中、读取不完整、几乎为空和未知项目不可选。串行清理、逐项报告，停止在当前项完成后生效，单项失败继续其余项。失败时可能已有部分文件删除。
- **磁盘分析**：个人文件夹、系统数据卷和自选目录，包括外接磁盘。按目录查看已分配空间，支持取消，此页面只读。不会跟随符号链接；Windows 还会跳过重解析点，包括目录联接与云文件占位符。
- **运行负载**：CPU、内存、交换空间、进程和硬件提供的温度。macOS 提供平均负载与系统热压力；Windows 显示逻辑处理器、CPU 使用率与进程数，热压力标为不可用。没有真实温度读数时明确提示，不估算。
- **性能与电源**：点击侧栏电源状态或进入运行负载页，选择模式并点击“应用模式”。macOS 支持低电量／自动，需要系统管理员授权，同时作用于电池和外接电源，不修改高功率模式。Windows 读取现有电源方案，通过系统 API 切换并复核结果；硬件或组织策略可能只提供一个方案，不创建虚假的高性能选项。此功能控制系统电源策略，不设置 CPU 使用率上限或超频。
- **进程管理**：macOS 降低 nice 优先级或发送 SIGTERM；Windows 设为 Below Normal，或向可见窗口发送 WM_CLOSE 请求，由应用决定是否保存和退出。没有窗口的后台进程不能在这里退出，不执行强制结束。复核 PID 与启动时间；系统、其他用户、应用自身与祖先进程受保护。
- **采样与节能**：顶部和负载页同步暂停／恢复，保留最后读数。默认窗口不活跃、最小化时暂停周期采样；低电量模式下至少 10 秒一次。扫描使用低优先级与可选节能节奏。电源信息最多每 30 秒刷新一次，也可手动刷新。
- **增量更新**：系统文件事件只标记发生变化的缓存，在清理页活跃时按需合并刷新，最多每 30 秒一次；不反复遍历整盘。
- **更新检查**：设置中手动检查 GitHub 正式版并打开发布页，不自动安装。私有仓库只读令牌保存到 macOS 钥匙串或 Windows 凭据管理器。
- **本机接口**：默认关闭，`127.0.0.1:47653` 提供 Bearer 认证的只读 JSON 和 SSE 状态事件，供 AI 触发自己的任务。包括平台与电源状态；不开放清理、进程或电源写操作。详见 [API.md](API.md)。

## 清理与数据边界

只清理内置规则中的缓存，源码、`.git`、聊天记录、用户设置、SDK 和 Xcode Archives 不在名单中。推荐不代表所有场景都无感；清理后可能需要重新下载、编译或安装测试浏览器。

macOS 在删除前检查目录身份、所有者与关联进程，通过目录文件描述符操作，不跟随链接或跨卷。uv 使用官方清理工具，npm 可用时使用官方命令。Windows 仅清理 LOCALAPPDATA 下已识别的 pip、npm、Go、Chrome、Edge、Electron 与 electron-builder 缓存；删除前固定各级目录句柄，复核卷与文件标识，通过句柄删除，遇到重解析点或跨卷立即报错。浏览器只处理 Default 配置的 Cache，不删除用户资料。当前 Windows 规则不包含 uv 或 Playwright。

目录占用按已分配数据块估算，硬链接去重；克隆共享、快照及其他程序的并发活动会使估算与实际释放不同。扫描仅读取元数据，不哈希内容，不主动下载云文件。

扫描快照在 macOS 的 `~/Library/Application Support/local.macspace.desktop/` 或 Windows 的 `%APPDATA%/local.macspace.desktop/`。本机接口令牌在 macOS 以 0600 文件保存，在 Windows 使用当前用户的 DPAPI 加密保存。偏好在 WebView 本机存储中。受保护目录读取失败会计入错误数；macOS 可授予完全磁盘访问权限后重启，Windows 应检查目录权限。

## 开发与验证

需要 Node.js 22.12+、Rust stable；macOS 需要 Xcode Command Line Tools，Windows 需要 MSVC C++ 构建工具和 WebView2。当前工作区 Rust 隔离在 `.tools/`，npm 包装脚本自动选用，不修改系统 PATH。其他机器使用已安装的 Rust。

```sh
npm ci
npm run tauri -- dev
npm run check
npm test
npm run test:native -- --release
```

macOS 构建与打包：

```sh
npm run tauri -- build --bundles app
npm run package:pkg
```

`package:pkg` 用系统 pkgbuild，检查源与包内版本、签名完整性和架构。支持传入应用路径：`npm run package:pkg -- "/path/to/System Balance.app"`，输出 PKG 和 SHA256。Windows 上运行 `npm run tauri -- build --bundles nsis`。GitHub Actions 的 Windows 工作流运行前端测试、原生测试、Clippy 并生成真正的 Windows 安装器。

原生测试只操作独立临时目录和自行启动的测试进程，不清理真实缓存或切换机器电源模式。Windows 测试覆盖目录固定、联接拒绝、根目录保留、DPAPI 加解密和真实电源方案读取；macOS 测试覆盖硬链接、符号链接、删除边界与进程身份。两平台共享接口认证、HTTP/SSE 服务关停、批量清理与版本比较测试。

隔离界面测试：运行 `npm run dev`，访问 `/tests/smart-cleanup.html` 或 `/tests/integrations.html`。后者支持 `?language=en`、`?windows=1`、`?powerDenied=1`、`?portBusy=1` 与 `?authError=1`。使用模拟数据和命令，不访问机器信息、凭据或真实缓存，不启动实际接口、不更改系统电源状态，不包含在正式构建中。

发布包未使用商业签名证书：macOS 应用为 ad-hoc 签名，PKG 未签名且未公证；Windows 安装器未做 Authenticode 签名。Gatekeeper／SmartScreen 可能提示来源。源码仓库不含工具链、依赖、扫描数据和编译产物，交付应用不依赖 Node 或 Rust。

## 参与贡献

- 行为异常、构建问题：开 [Issue](https://github.com/AndroidSix/mac-space/issues)
- 代码与文档改进：先读 [CONTRIBUTING.md](CONTRIBUTING.md)
- 安全相关：按 [SECURITY.md](SECURITY.md) 私下报告

## 许可证

本项目采用 [MIT License](LICENSE) 开源。
