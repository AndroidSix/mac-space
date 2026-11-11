# MacSpace 本机接口

创建于 2026-11-11

适用于 0.3.0 及以上版本，用于本机 AI 和自动化读取状态、触发自己的任务。默认关闭，在“设置 → 本机自动化接口”中开启。应用退出后停止，不安装常驻服务。

## 连接与认证

固定地址 `http://127.0.0.1:47653`。仅监听 IPv4 回环地址，不接受局域网连接，不提供 CORS，拒绝带 `Origin` 的网页请求和非本机 Host。请使用终端、原生程序或服务端 HTTP 客户端。

所有请求都需要 `Authorization: Bearer <本机接口令牌>`。本机接口令牌与 GitHub 更新令牌独立。在设置中可显示并复制；关闭接口后可以更换，旧令牌随即失效。

配置及 256 位随机令牌保存在 `~/Library/Application Support/local.macspace.desktop/local-api.json`，权限为 `0600`。本机同一用户运行的 AI 可以读取它。不要上传该文件或将令牌写入日志；持有令牌即可读取进程名称和已扫描路径。

```sh
export MACSPACE_TOKEN="$(node -e 'const fs = require("node:fs"); const path = require("node:path"); const config = JSON.parse(fs.readFileSync(path.join(process.env.HOME, "Library/Application Support/local.macspace.desktop/local-api.json"), "utf8")); process.stdout.write(config.token)')"
curl --fail --silent --show-error \
  -H "Authorization: Bearer $MACSPACE_TOKEN" \
  http://127.0.0.1:47653/v1/status
```

## JSON 接口

全部为只读 `GET`。请求只返回已有数据，不触发额外文件扫描或系统采样，不提供清理、进程控制或任意命令执行能力。

- `/v1/status`：`schemaVersion`、应用 `version`、响应时间 `observedAt`、`samplingPaused`、扫描／清理 `busy`。
- `/v1/metrics`：最近一次指标，包括 `cpu`、`memoryUsed`、`memoryTotal`、`swapUsed`、`diskTotal`、`diskAvailable`、`sensors`、`thermalState`、`processes`、`loadAverage`、`sampledAt` 等。未采样时返回 JSON `null`。时间为 Unix 秒，容量为字节，`sensors[].celsius` 为摄氏度。
- `/v1/snapshot`：最近一次扫描，包括 `caches`、`cacheScannedAt`、`directory`、`dirty`、`watching`。`dirty=true` 表示缓存目录已变化，大小可能过期；需要在应用中重新扫描。未扫描结果可能为空。

开启接口后，后台最多每 10 秒补采一次指标，前台已有足够新的读数时复用它。右上角或运行负载页的暂停按钮会同时停止前台和接口后台采样，保留最后读数。暂停不取消扫描或清理，也不停止文件事件通知。客户端应检查 `sampledAt` 和 `samplingPaused`，避免把旧读数当作实时数据。

HTTP 401 表示未启用、认证错误、Host 错误或带有 Origin；405 表示不支持的方法；404 表示路径不存在；SSE 超过 8 个同时连接返回 429。端口占用会在设置里报错，不会静默切换端口。

## SSE 事件流

```sh
curl --fail --no-buffer \
  -H "Authorization: Bearer $MACSPACE_TOKEN" \
  http://127.0.0.1:47653/v1/events
```

连接时立即收到 `event: state`。指标更新、暂停／恢复、缓存变化、扫描／清理完成时推送同名事件。`data` 为单行 JSON，包含 `/v1/status` 字段，加上 `metrics` 和 `snapshot`。空闲连接每 15 秒发送 SSE 心跳注释。

采用“最新状态”语义，慢客户端可能合并中间变化，不是审计日志，不提供 Last-Event-ID 回放。客户端断线后应退避重连，并从新的初始状态恢复。关闭接口会断开事件流。

以下 Node.js 22+ 示例在磁盘可用空间低于 20 GB 时触发一次，恢复到 22 GB 以上才重新布防，暂停或过期指标不触发。将 `console.log` 所在分支替换为你的 AI 任务入口。

```js
const token = process.env.MACSPACE_TOKEN;
if (!token) throw new Error('Set MACSPACE_TOKEN first');
let armed = true;
let delay = 1000;
for (;;) {
  try {
    const response = await fetch('http://127.0.0.1:47653/v1/events', {
      headers: { Authorization: `Bearer ${token}` },
    });
    if (response.status === 401) throw new Error('AUTH');
    if (!response.ok) throw new Error(`HTTP ${response.status}`);
    delay = 1000;
    let buffer = '';
    const decoder = new TextDecoder();
    for await (const chunk of response.body) {
      buffer += decoder.decode(chunk, { stream: true });
      let boundary;
      while ((boundary = buffer.indexOf('\n\n')) >= 0) {
        const block = buffer.slice(0, boundary);
        buffer = buffer.slice(boundary + 2);
        const line = block.split('\n').find((line) => line.startsWith('data:'));
        if (!line) continue;
        const state = JSON.parse(line.slice(5).trim());
        const metrics = state.metrics;
        if (state.samplingPaused || !metrics || Date.now() / 1000 - metrics.sampledAt > 30) continue;
        if (armed && metrics.diskAvailable < 20e9) {
          armed = false;
          console.log('disk.low', { available: metrics.diskAvailable, sampledAt: metrics.sampledAt });
        } else if (metrics.diskAvailable > 22e9) {
          armed = true;
        }
      }
    }
  } catch (error) {
    if (error.message === 'AUTH') throw new Error('Refresh the local API token before reconnecting');
  }
  await new Promise((resolve) => setTimeout(resolve, delay));
  delay = Math.min(delay * 2, 30000);
}
```

## 更新检查

设置中手动检查 `AndroidSix/mac-space` 的最新正式 Release，使用语义版本比较，忽略草稿和预发布。下载按钮打开发布页，不自动安装或替换程序。

私有仓库需要 GitHub 细粒度令牌：仅授权此仓库，授予 `Contents: Read-only`。在设置中保存后进入 macOS 钥匙串，不写入本机接口配置或 WebView 存储。更新请求仅发送至 `https://api.github.com`，关闭重定向，不上传机器指标或扫描数据。不配置令牌也可打开发布页，使用浏览器现有的 GitHub 登录状态下载。
