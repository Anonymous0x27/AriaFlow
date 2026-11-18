# HTTP API 参考（ariaflowd）

- 基础地址：`http://127.0.0.1:8787`（默认，可用环境变量 `ARIAFLOW_API_URL` / CLI 全局参数 `--api-url` 修改）
- 认证：若守护进程启用了 token，则所有请求（除 `GET /api/v1/openapi.json`）需携带 `Authorization: Bearer <token>`；未启用时无需认证（仅建议在回环地址下运行）
- 请求/响应均为 JSON（`application/json`）；字段名为 `camelCase`
- 错误格式：`{"error": {"code": "machine_readable_code", "message": "..."}}`
- 完整机器可读定义：[`crates/ariaflow-api/assets/openapi.json`](../crates/ariaflow-api/assets/openapi.json)（亦可通过 `GET /api/v1/openapi.json` 获取）

## 端点总览

| 方法 | 路径 | 说明 |
|---|---|---|
| GET | `/api/v1/status` | 守护进程状态与聚合计数（总数/各状态/当前速度/累计字节） |
| GET | `/api/v1/downloads` | 列出任务；查询参数：`status`（逗号分隔）、`category`、`q`、`page`、`pageSize` |
| POST | `/api/v1/downloads` | 添加任务 |
| POST | `/api/v1/downloads/batch` | 批量添加（`{"urls": [...]}` 或 `{"tasks": [...]}`；`tasks` 优先） |
| GET | `/api/v1/downloads/{id}` | 任务详情 |
| DELETE | `/api/v1/downloads/{id}` | 删除任务；查询参数 `deleteFile=true` 同时删除本地文件 |
| POST | `/api/v1/downloads/{id}/pause` | 暂停 |
| POST | `/api/v1/downloads/{id}/resume` | 继续 |
| POST | `/api/v1/downloads/{id}/cancel` | 取消 |
| POST | `/api/v1/downloads/{id}/retry` | 重新排队 |
| PUT | `/api/v1/downloads/{id}/priority` | 设置优先级（`{"priority": 1..=10}`） |
| PUT | `/api/v1/downloads/{id}/limit-rate` | 设置/清除单任务限速（`{"limitRateBytesPerSec": 1048576}`；`null` = 清除，回到全局 `perTaskSpeedLimit`；正在传输的任务会自动 pause→resume 应用新限速，响应 `restarted` 标记是否发生了重启） |
| GET | `/api/v1/downloads/{id}/file` | 下载**已完成**任务的文件（`Content-Disposition: attachment`；支持单个 `Range: bytes=…` → 206；未完成 → 409；文件缺失 → 404） |
| POST | `/api/v1/downloads/{id}/save-as` | 服务端复制已完成任务的文件到**守护进程所在机器**的 `destDir`（`{"destDir": "/abs/dir", "filename": "可选，默认=任务文件名，不允许路径分隔符", "overwrite": false}`；同名存在且未 `overwrite` → 409 `already_exists`；源=目标 → 400 `same_file`）。响应 `{"savedTo": "...", "size": n}` |
| GET | `/api/v1/settings` | 读取全局设置 |
| PUT | `/api/v1/settings` | 更新全局设置（部分字段） |
| GET | `/api/v1/history` | 最近结束的任务归档（查询参数 `limit`，默认 100，最大 500） |
| GET | `/api/v1/history/{id}/file` | 以 `download_history` 行 id 拉取已完成记录的文件（与任务版同语义；任务行被删后仍可用） |
| POST | `/api/v1/history/{id}/save-as` | 历史记录版“另存为”（同 `/api/v1/downloads/{id}/save-as` 语义，按历史行 id 寻址） |
| GET | `/api/v1/categories` | 导出内置任务分类规则（机器可读 JSON，version 1） |
| GET | `/api/v1/fs/list` | 目录列表（供桌面端/网页端文件夹选择器使用；只读元数据，不返回文件内容）；查询参数 `path`（绝对路径，缺省为守护进程用户的主目录）、`showHidden`（默认 `false`） |
| GET | `/api/v1/proxies` | 列出已配置的命名代理 |
| POST | `/api/v1/proxies` | 新建代理（`name`/`kind`/`address` 必填） |
| PUT | `/api/v1/proxies/{id}` | 更新代理；`isDefault: true` 将其设为默认 |
| DELETE | `/api/v1/proxies/{id}` | 删除代理 |
| POST | `/api/v1/probe` | 探测 URL：返回最终地址、大小、Range 支持、ETag、Last-Modified、建议文件名（不下载） |
| GET | `/api/v1/events` | SSE 事件流（`download_*` 事件） |
| GET | `/api/v1/openapi.json` | OpenAPI 3.0.3 定义 |

### addMany 批量添加

```json
{"action": "addMany", "tasks": [{"url": "https://…", "saveDir": "/data/dl",
  "headers": [["Authorization", "Bearer …"]],
  "auth": {"type": "bearer", "token": "…"},
  "proxy": {"kind": "http", "address": "http://127.0.0.1:7890"},
  "startPaused": false, "scheduledStartAt": "2026-11-13T18:00:00Z"}]}
```

- `tasks` 中每一项与 `POST /api/v1/downloads` 的请求体相同；`tasks` 存在时忽略 `urls`。
- 逐项 best-effort：单项失败不中断整体；响应包含 `ids`（成功）与 `errors[]`（`index`/`url`/`message`）。
- 也兼容简化形式 `{"action": "addMany", "urls": ["https://a", "https://b"]}`（无高级选项）。

## 文件系统目录列表（文件夹选择器）

`GET /api/v1/fs/list` 为内置文件夹选择器提供目录浏览能力，替代浏览器的 `showDirectoryPicker()`（后者只能拿到句柄、且无法浏览 daemon 所在机器的目录）。

- 查询参数：`path`（绝对路径；为空或缺省时列出守护进程用户的主目录）、`showHidden`（布尔，默认 `false`，控制是否包含以 `.` 开头的项）。
- 响应字段：`path`（规范化的绝对路径）、`parent`（规范化父目录，到达文件系统根时为 `null`）、`isRoot`、`showHidden`、`truncated`（条目超过 5000 时为 `true`）、`entries[]`（`name`、`path`、`kind`：`dir`/`file`/`other`、`size`（仅文件）、`modifiedSecs`）。
- 目录优先、名称不区分大小写排序；符号链接按其目标类型归类，失效链接显示为 `other`。
- 仅暴露目录项的**元数据**（名称/类型/大小/修改时间），永远不返回文件内容。访问控制与其余 API 完全一致：配置了 `apiToken` 时需要 `Authorization: Bearer`，回环无 token 默认放行，非回环无 token 启动时强警告。
- 桌面端与网页端的“保存目录”选择框都通过该端点取数（Tauri 内经 IPC 代理、浏览器直连 API）。

## 数据模型

### DownloadTask

```json
{
  "id": "9353c9a9-…-71e14de91288",
  "url": "https://iso.example.com/ubuntu.iso",
  "finalUrl": "https://cdn.example.com/ubuntu.iso",
  "filename": "ubuntu-24.04-desktop-amd64.iso",
  "savePath": "/home/user/Downloads/ubuntu-24.04-desktop-amd64.iso",
  "totalSize": 5905580032,
  "downloadedSize": 5385939113,
  "status": "downloading",            // queued|downloading|paused|completed|failed|cancelled
  "progress": 91.2,
  "speed": 8388608,
  "averageSpeed": 9800000,
  "priority": 1,
  "retryCount": 0,
  "maxConnections": 32,
  "currentConnections": 32,
  "category": "archives",
  "etag": "W/\"…\"",
  "lastModified": "Wed, 11 Nov 2026 00:00:00 GMT",
  "createdAt": "2026-11-12T15:20:00+00:00",
  "startedAt": "2026-11-12T15:20:01+00:00",
  "completedAt": null,
  "errorMessage": null,
  "limitRateBytesPerSec": null,
  "maxRetries": null
}
```

`status` 取值：`pending`、`queued`、`connecting`、`downloading`、`paused`、`waiting`、`retrying`、`merging`、`verifying`、`completed`、`failed`、`cancelled`。

### 添加任务

`POST /api/v1/downloads`

```json
{
  "url": "https://example.com/release.iso",
  "saveDir": "/home/user/Downloads",
  "filename": "release.iso",
  "priority": 5,
  "maxConnections": 16,
  "headers": [["Authorization", "Bearer …"]],
  "startPaused": false,
  "allowRename": true,
  "expectedChecksum": {"algorithm": "sha256", "digest": "…"},
  "limitRateBytesPerSec": 0,
  "maxRetries": 5
}
```

支持的 URL scheme：`http`、`https`、`ftp`、`ftps`、`sftp`、`magnet`（BitTorrent）、`metalink`。URL 路径以 `.torrent` 结尾的 http/https 链接会自动作为 BitTorrent 任务处理（由 librqbit 下载并解析 manifest，再按 piece 续传），文件名默认去掉 `.torrent` 后缀。除 `url` 外全部字段可选（`headers`/`startPaused`/`allowRename` 等缺省即用默认值）。`saveDir` 必须位于配置的下载目录之内；文件名由内核统一清洗。`201 Created` 返回任务对象本身（含 `id`）。

- `limitRateBytesPerSec`：单任务限速（bytes/sec，0/缺省 = 继承全局 `perTaskSpeedLimit`）
- `maxRetries`：单任务自动重试次数覆盖（缺省 = 全局 `retryCount`）

### 探测

`POST /api/v1/probe`

```json
{ "url": "https://example.com/file.bin", "headers": [["Referer", "https://example.com/"]] }
```

返回 `RemoteFileInfo`：

```json
{
  "finalUrl": "https://cdn.example.com/file.bin",
  "totalSize": 3145851,
  "acceptRanges": true,
  "etag": "\"test-etag-abc123\"",
  "lastModified": "Wed, 11 Nov 2026 00:00:00 GMT",
  "contentType": "application/octet-stream",
  "filenameHint": "file.bin",
  "headers": { "content-length": "3145851", "...": "..." }
}
```

服务端对上游错误返回 `502 {"code": "probe_failed"}`；未启用探测提供器返回 `502 {"code": "bad_status"}`。

### 事件流（SSE）

`GET /api/v1/events`，`text/event-stream`：

```
event: download_created
data: {"type":"download_created","id":"5d058ecc-…"}

event: progress
data: {"type":"progress","id":"…","downloadedSize":…,"speed":…}
```

客户端应使用标准 SSE（`EventSource` / `fetch` + 流式解析）。事件类型与 `ariaflow-core::DownloadEvent` 一致。

### 全局设置

`GET /api/v1/settings` 返回：

```json
{
  "downloadDir": "/home/user/Downloads",
  "maxConcurrentTasks": 5,
  "maxConnectionsPerTask": 16,
  "maxTotalConnections": 64,
  "globalSpeedLimit": 0,
  "perTaskSpeedLimit": 0,
  "retryCount": 5,
  "userAgent": "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/154.0.0.0 Safari/537.36",
  "overviewShowPaused": false,
  "overviewShowNewTasks": true,
  "overviewHiddenCards": [],
  "deleteFilesOnRemove": false,
  "autoQuitDaemon": false
}
```

`userAgent` 为出站下载（以及 `/api/v1/probe` 探测）使用的 User-Agent；**初始默认值**为 `Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/154.0.0.0 Safari/537.36`（此前为 `AriaFlow/<version>`）。注意：已写入数据库的旧默认值不会自动迁移，仅新库/缺失该键时使用新默认。`autoQuitDaemon`（默认 `false`）为桌面壳层（Tauri）偏好：为 `true` 时，托盘菜单“退出 AriaFlow”会在退出桌面程序前同时关闭本机 ariaflowd；设置存于全局设置中，网页模式下无效。本地管道始终可读写该键。其余纯 UI 偏好：`overviewShowPaused`（概览页“显示已暂停任务”）、`overviewShowNewTasks`（概览页“显示新建的任务”，默认 `true`——新创建/排队中的任务默认出现在“正在进行”列表）、`overviewHiddenCards`（概览页“统计卡片”隐藏的卡片 id 数组，取值 `downloading/waiting/paused/completed/failed/currentSpeed/totalDownloaded`；`[]` 为全部显示；未知 id 会被静默忽略）与 `deleteFilesOnRemove`（删除任务确认框中“同时删除已下载的文件”复选框的默认勾选值），服务器仅原样存储；`deleteFilesOnRemove` 的单次删除可在各确认框中覆盖。三个概览偏好均由概览页“显示内容”弹窗统一编辑（不再以内联勾选框暴露）。旧库中的缺失键：`overviewShowNewTasks` 按 `true` 处理、其余按 `false`/`[]` 处理（无需迁移）。

### 运行时特性开关（无需重启）

三个开关写入 `GlobalSettings`（SQLite，重启后保留），`PUT /api/v1/settings` 成功后**立即**推入守护进程内的原子开关并立即生效：

| 字段 | 默认 | 关闭后的行为 |
|---|---|---|
| `apiEnabled` | `true` | 所有 `/api/v1/*`（含 SSE、OpenAPI）返回 `503 {"error":{"code":"feature_disabled",...}}`；**本地管道（IPC）不受此开关影响**，可随时重新打开 |
| `mcpEnabled` | `true`（v0.1.0 起真正生效；旧库中 inert 的 `false` 会作为"关闭"生效——请用 `ariaflow config set mcp-enabled true` 恢复） | 所有 `/mcp` 请求返回 `503 feature_disabled`（网页 & 管道上的 MCP 挂载同样 503） |
| `webUiEnabled` | `true` | `/`（内嵌 web 网页及 SPA 回退）返回 `503`，内容为说明关闭原因的 HTML 页；`/api/v1`、`/mcp` 不受影响 |

`webUiBind`（字符串，默认 `""`）**不是**运行时开关：它决定独立 web 监听的绑定地址，是启动期配置（见上文"独立 Web 监听端口"）。

说明：

- 开关是**请求级门控**，不重绑端口、不重启进程；已经建立的 SSE 连接不会被主动断开，但之后的请求会被拒绝。
- MCP 工具的回环自调用优先走**管道**（永不受 `apiEnabled` 影响），仅在无管道（`--no-ipc`）时才回退到 loopback HTTP API——此时若关闭 `apiEnabled`，MCP 工具会随之不可用。
- 若启动时传了 `--no-api` / `--no-ipc` / `--no-mcp`，相应监听根本不会创建（`--no-mcp` 时 `/mcp` 返回 404）；特性开关与这些启动参数是正交的两层。
- CLI：`ariaflow config set api-enabled|mcp-enabled|web-ui-enabled true|false`（默认走管道，关闭 HTTP API 后依然有效）。
- 桌面端（Tauri，默认管道传输）设置页"接口开关"一节同样直接即时生效。
- 未启用 token 的非回环绑定、以及已存在的连接，不在此门控范围内（门控只拦截新请求）。

### 主 HTTP 监听地址（`--api-bind` / `apiBind`）

ariaflowd 的主监听地址是**启动期配置**，有四个入口（优先级从高到低）：

1. `--api-bind ADDR:PORT` 启动参数；
2. `ARIAFLOW_API_BIND` 环境变量；
3. 持久化设置 `apiBind`（`PUT /api/v1/settings` / `ariaflow config set api-bind` / 设置页 / MCP `apiBind` 写入）；
4. 默认 `127.0.0.1:8787`。

```bash
ariaflowd --api-bind 0.0.0.0:8787        # 启动参数（优先于设置）
ariaflow config set api-bind 0.0.0.0:8787   # 写入设置（需重启；--api-bind / ARIAFLOW_API_BIND 优先）
```

- `PUT /api/v1/settings` 带 `apiBind` 只保存并返回完整设置，**重启后**才生效；非 `ADDR:PORT` 或为空（含非法 IP/越界端口）会被 **400 拒绝**，不会写坏数据库。
- 持久化 `apiBind` 在启动时被解析：非法值只记一条 ERROR 日志并**回退 `127.0.0.1:8787`**（不会因为配置错误启动失败）；显式 `--api-bind`/`ARIAFLOW_API_BIND` 非法则启动报错退出（显式配置应当尽早暴露错误）。
- 非回环绑定且无 `--token`：启动日志强警告（同 `webUiBind` 语义）——任何能连到该地址的人都能控制下载；LAN 场景务必设置 token，必要时再套 TLS 反向代理。
- 自 v0.1.0 起该设置真正接线：此前仅有存储字段、不影响启动；现在它与 `--api-bind` 是同一条生效路径。

### 独立 Web 监听端口（`--web-bind` / `webUiBind`）

桌面 web 版除了挂在 `--api-bind`（默认 `127.0.0.1:8787` 的 `/`）之外，还可以绑定**独立端口**，用于把"人用网页"和"API 客户端入口"分开（例如 API 保持回环、网页对局域网开放）：

```bash
ariaflowd --web-bind 0.0.0.0:9090        # 启动参数（优先于设置）
ariaflow config set web-ui-bind 0.0.0.0:9090   # 或写入设置（需重启；--web-bind 优先）
ariaflow config set web-ui-bind ""        # 清空即关闭
```

- 默认**不监听**；绑定地址在**启动时确定**，`PUT /api/v1/settings` 修改 `webUiBind` 只保存不生效，重启后起作用（与 `api-bind` 一致）。配置了非法值会在启动时直接报错退出；`PUT /api/v1/settings` 带非法值会被 400 拒绝，不会污染数据库。
- 该监听**完整托管**桌面 web 版：`/` 网页 + 同源 `/api/v1`（网页默认同源调用 API，局域网浏览器零配置可用）；若启用了 MCP，`/mcp` 也会挂载。
- 认证语义与 `--api-bind` 完全一致：配置了 token 则所有请求都需要 `Authorization: Bearer`；回环绑定且无 token 免认证；非回环绑定且无 token 时启动日志强警告（任何人都能连上控制下载，务必设置 `--token`；LAN 上建议再套一层 TLS 反向代理）。
- 与运行时开关正交：`webUiEnabled=false` → 该端口的 `/` 返回 503 说明页（API 仍在）；`apiEnabled=false` → `/api/v1` 503（网页可开但取不到数）；管道不受影响。
- 局域网使用 = 浏览器访问 `http://<机器IP>:9090/`；首次打开后按需在设置页填入 API token（若有）。

`PUT /api/v1/settings` 接受同名部分字段并返回更新后的完整设置。时间窗口相关字段：`downloadWindowStartMinute` / `downloadWindowEndMinute`（本地零点起分钟数，`0..1439`，二者存在且必须不相等；窗口可跨午夜；发送显式 `null` 可清除窗口，省略则保持不变）。返回中始终包含当前窗口配置，`null` 表示不限制。

此外还有两个**下载后钩子**（IDM 式"完成后处理"，可选，默认空）：`onDownloadCompleteCommand` / `onDownloadErrorCommand`，在任务进入终态（完成 / 永久失败）时由 ariaflowd 通过系统 shell 执行（Windows 用 `cmd /C`，其他平台 `sh -c`）。环境变量：`ARIAFLOW_RESULT`（`completed|failed`）、`ARIAFLOW_ID`、`ARIAFLOW_URL`、`ARIAFLOW_FINAL_URL`、`ARIAFLOW_FILE`、`ARIAFLOW_FILENAME`、`ARIAFLOW_TOTAL_BYTES`、`ARIAFLOW_ERROR`（仅失败时有内容）。发送空字符串可清除钩子。钩子以分离进程方式运行（stdin/stdout/stderr 均为空），超过 10 分钟会被终止，不会阻塞下载流程。钩子是**任意代码执行**，只应配置为你自己编写或信任的命令。

## 单任务限速

`PUT /api/v1/downloads/{id}/limit-rate`

```json
{ "limitRateBytesPerSec": 262144 }
```

- 单位：字节/秒；`0` = 不限速；`null` = 清除任务级覆盖（沿用全局 `perTaskSpeedLimit`）。`limitRateBytesPerSec` 键必须显式出现（省略会返回 400）。
- 引擎在**开始传输时**读取该值；对正在下载/连接中的任务，本端点会自动执行 pause→resume 使新限速立即生效（断点续传无损），响应中 `restarted: true` 表示发生了重启；对排队中/暂停/未开始的任务，新限速在下次开始时生效。
- 响应：`{ "id": "…", "limitRateBytesPerSec": 262144, "restarted": true }`。

## 历史归档

`GET /api/v1/history?limit=100` 返回按完成时间倒序的数组，元素为：

```json
{
  "url": "https://example.com/f.zip",
  "filename": "f.zip",
  "savePath": "/home/user/Downloads/f.zip",
  "totalSize": 2097152,
  "averageSpeed": 262144,
  "result": "completed",
  "errorMessage": null,
  "completedAt": "2026-11-12T09:30:00+00:00"
}
```

`result` ∈ `completed | failed | cancelled`。历史记录写入时机与 `download_history` 表一致（任务进入终态时）。

### 已完成文件的获取（web / 桌面 / AI）

三种方式，按使用方选择：

1. **浏览器触发下载**（web UI “已完成”列表的 ⬇ 按钮）：`GET /api/v1/downloads/{id}/file`（或历史版 `/api/v1/history/{id}/file`）。浏览器实现为先 `fetch`（携带 Bearer token）再转 Blob 触发 `<a download>`；全量读入内存，超大文件建议改用方式 3。
2. **桌面端“另存为…”**（任务行 / 历史行的 💾 按钮）：先选目标目录（DirPicker），然后 `POST …/save-as`，文件在**守护进程所在机器**上直接复制，不经前端。`filename` 仅接收纯文件名。
3. **AI（MCP）**：`ariaflow_get_download_file`（base64 内联，默认上限 1 MiB、硬上限 10 MiB，适合小文件/文本）与 `ariaflow_save_download`（守护进程侧复制到本地路径，适合大文件）。stdio 与 `/mcp` HTTP 两种传输均可用。

## 分类规则导出

`GET /api/v1/categories` 返回内置分类规则（扩展名 → 分类 key 映射），可用于备份或生成自定义规则：

```json
{
  "version": 1,
  "categories": [
    { "key": "documents", "label": "文档", "labelEn": "Documents", "extensions": ["pdf", "doc"] }
  ],
  "fallback": "other"
}
```

CLI 等价操作：`ariaflow categories export [file]`（省略 file 时输出到 stdout）。

## 命名代理

`proxies` 表保存命名代理，桌面端可在设置中管理；`isDefault=true` 的代理在同一事务内自动取消其他默认代理。`kind` ∈ `http | https | socks5 | socks5h`，`address` 需以对应协议前缀开头，例如 `http://127.0.0.1:7890`。密码明文存储（与任务内嵌 `ProxyConfig` 一致），导出/共享配置时请注意脱敏。

## 错误码（常见）

| HTTP | code | 含义 |
|---|---|---|
| 400 | `invalid_argument` | 参数非法（URL、目录、优先级越界等） |
| 401 | `unauthorized` | 缺少/错误的 Bearer token |
| 404 | `not_found` | 任务不存在 |
| 409 | `conflict` | 状态不允许该操作（如对已完成任务 resume） |
| 502 | `probe_failed` / `bad_status` | 探测失败 / 上游响应异常 |
| 503 | `feature_disabled` | 对应接口已被运行时开关关闭（见上文"运行时特性开关"） |
| 429 | `rate_limited` | 超出调用频率限制（见下文"频率限制"） |
| 500 | `internal` | 内部错误（详见日志） |

## 频率限制

受保护的 API/MCP 路由按**客户端 IP** 做令牌桶限流（进程内、重启清零）：

- **回环地址（127.0.0.0/8、::1）不限流**——桌面端/Web UI 高频轮询（列表+状态 1.5s、详情弹窗 1s，且多个窗口共享 127.0.0.1）是合法流量；设置 `--token` 后回环仍免 token，但不代表未授权。
- **非回环地址**保留限流：突发 600 次、持续 10 次/秒（每分钟 600）。超出返回 `429 rate_limited`，建议客户端按响应稍后重试。

## 桌面端客户端示例

```ts
const res = await fetch('http://127.0.0.1:8787/api/v1/downloads?status=downloading', {
  headers: token ? { authorization: `Bearer ${token}` } : {},
});
const tasks = await res.json();
```
