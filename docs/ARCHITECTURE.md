# AriaFlow 架构

## 设计原则

1. **核心与界面分离**：下载逻辑只存在于 Rust 核心；GUI 不持有任何下载状态，它是守护进程的纯客户端。
2. **统一业务接口**：所有客户端（GUI / CLI / TUI / MCP / 浏览器扩展 / native host）只操作 `ariaflowd`，业务逻辑零重复。
3. **管道优先的本地传输**：`ariaflow`（CLI/TUI）与浏览器 native host 默认经由**本地管道**（Unix domain socket / Windows 命名管道）访问 `ariaflowd`；可用 `--transport webapi` 强制走 web API。桌面程序在设置里自由选择 web API 或管道；浏览器扩展直接使用 Web API，无需 native host。
4. **可测试、可替换**：存储、协议提供器、调度器全部是 trait + 依赖注入；GUI 系统（Linux/macOS/Windows 的 WebView）只在 `apps/desktop/src-tauri` 中出现。
5. **安全默认值**：API 默认只绑定 `127.0.0.1`；管道默认以 `0600` 权限创建且仅属主可访问；token 认证、URL 校验、文件名清洗、路径越界防护全部在内核侧完成。

## 目标架构（当前实现）

```
┌─────────────────────────────────────────────────────────┐
│  ariaflowd —— 一切皆服务：下载任务 / web API / MCP / 管道 / Web 网页 │
│  ┌───────────┐ ┌──────────┐ ┌─────────┐ ┌─────────────┐ │
│  │ 下载引擎/ │ │ web API  │ │ MCP     │ │ 管道访问     │ │
│  │ 调度/存储 │ │ :8787    │ │ /mcp    │ │ (Unix sock / │ │
│  └───────────┘ └──────────┘ └─────────┘ │  named pipe) │ │
│                              ┌─────────┐ └─────────────┘ │
│                              │ Web 页面 │ 与桌面程序同款   │
│                              │  /       │ (apps/desktop/dist)│
│                              └─────────┘                 │
└───────▲───────────────▲───────────────▲─────────────────┘
        │ 管道(默认)    │ web API       │ web API
        │ 或 webAPI     │ (回环端口)    │
┌───────┴──────┐ ┌──────┴───────┐ ┌─────┴─────────┐
│ ariaflow CLI │ │ 桌面程序      │ │ 浏览器扩展 +   │
│ + TUI        │ │ (Tauri 2)    │ │ HTTP Web API   │
│ --transport  │ │ 设置中可选    │ │                │
│ auto/ipc/    │ │ 管道或webapi  │ │                │
│ webapi       │ │              │ │                │
└──────────────┘ └─────────────┘ └───────────────┘
```

## 分层

```
┌──────────────────────────────────────────────────────────┐
│  客户端层（无业务逻辑）                                    │
│  desktop (Vue3+Tauri2) │ ariaflow CLI (+ --tui TUI) │
│  ariaflow-mcp (stdio)  │ browser-extension (Web API)  │
└───────┬──────────┬────────────────┬─────────────────────┘
        │ 管道(默认) │ web API       │ web API / 管道
        │           │ (可选)        │
┌───────────────▼───────────────────────────────────────────┐
│  ariaflowd（组合根 / 组装）                                │
│  ┌─────────┐  ┌───────────┐  ┌────────────┐              │
│  │ Axum API │  │ Scheduler │  │ Event bus  │              │
│  │+ Web UI  │  │           │  │            │              │
│  │+ /mcp    │  └─────┬─────┘  └─────┬──────┘              │
│  └────┬─────┘       │              │                      │
│       │              │              │                      │
│  ┌────▼──────────────▼──────────────▼─────┐               │
│  │  ariaflow-core：DownloadService trait   │               │
│  │  模型 / 事件 / 校验 / 文件名清洗          │               │
│  └────┬──────────────┬──────────────┬─────┘               │
│       │              │              │                      │
│  ┌────▼────┐   ┌─────▼──────┐  ┌────▼─────────┐          │
│  │ Engine  │   │  Storage   │  │  Protocol    │          │
│  │ 下载引擎 │   │ SQLite/WAL │  │  Download    │          │
│  │ 段下载  │   │ Repository │  │  Provider    │          │
│  └─────────┘   └────────────┘  └──────────────┘          │
└──────────────────────────────────────────────────────────┘
```

## Crate 职责

| Crate | 职责 | 不包含 |
|---|---|---|
| `ariaflow-core` | 领域类型：`DownloadTask`、`DownloadEvent`、`DownloadStatus`、校验和、文件名清洗、`DownloadService` async trait、设置模型 | 任何 IO、任何网络、任何 GUI 依赖 |
| `ariaflow-protocol` | `DownloadProvider` trait（`download`/`probe`/`resume` 能力声明）与协议级错误类型 | 具体协议实现 |
| `ariaflow-engine` | 传输提供器：HTTP/HTTPS（Range 分段、If-Range/ETag、重定向、代理、重试）、FTP/FTPS（REST 续传、被动模式）、SFTP（seek 续传）、Metalink（镜像择优 + 校验透传）、BitTorrent（magnet 与 http(s) `.torrent` 链接，librqbit，分 piece 续传）；`ProviderRegistry` 按 URL scheme 分发（`.torrent` 路径的 http/https URL 路由到 BT provider） | 任何数据库访问 |
| `ariaflow-storage` | SQLite 连接池、WAL、迁移（`schema.rs` 中内嵌有序迁移）、`DownloadRepository`、历史/事件/设置 CRUD | 业务判断 |
| `ariaflow-scheduler` | 纯逻辑：并发配额、优先级排序、限速桶（令牌桶）、失败重试退避、时间窗口判定 | 异步运行时之外的调度循环 |
| `ariaflow-api` | Axum 应用：`/api/v1/*` 路由、SSE 事件流、Bearer token 中间件、CORS（本地开发用）、OpenAPI 3.0 文档；**rust-embed 内嵌桌面 web 版前端**（`/`，SPA 回退；`--web-dir` 显式覆盖）；管道（Unix socket / 命名管道）与 TCP 共用同一套 Router；`GET /api/v1/fs/list`（只读目录元数据，供桌面/网页端文件夹选择器经 ariaflowd 浏览目录，不返回文件内容）；**运行时特性开关**（HTTP API / MCP / 网页可即时启停，503 `feature_disabled` 降级） | 下载业务本身 |
| `ariaflow-mcp` | MCP（Model Context Protocol）服务器实现：stdio 传输（`ariaflow-mcp` 独立二进制）+ 可挂载的 `ariaflow_mcp::router`（`ariaflowd` 把它挂到 `/mcp`，stdio 与 HTTP 共用同一份工具实现）；把 API 能力映射为 12 个工具，含 JSON-Schema 与危险操作 `confirm` 守卫 | 直接访问存储 |
| `ariaflow-cli` | `ariaflow` 命令：全部任务操作、`daemon start/stop/...`、`config show/set`、`--json` 输出、前缀 id 解析、`--tui` 全屏终端界面（ratatui）；**默认管道传输**，`--transport auto|ipc|webapi` 可切换（auto：管道优先、失败回退 web API） | 下载业务本身 |
| `ariaflow-daemon` | 组装 core + storage + engine + scheduler + api；tokio 运行时；优雅停机；同时监听 **HTTP（默认 127.0.0.1:8787）、本地管道（默认开启，`--no-ipc` 关闭），以及可选的独立 Web 监听（`--web-bind ADDR:PORT` 或设置 `webUiBind`；默认不监听、启动期配置、与 api-bind 认证语义一致；该端口完整托管网页 + 同源 `/api/v1`，用于局域网浏览器直接使用）**；在 `/` 托管 Web 网页（与桌面程序同款，**编译期 rust-embed 内嵌**，无需运行时 web 目录；`--web-dir` 可覆盖为磁盘目录，`apps/desktop/dist` 不存在则返回占位说明页）；在 `/mcp` 挂载 MCP JSON-RPC（继承同样的 Bearer 认证与限流）；**下载编排器（orchestrator）**：崩溃恢复（中断任务回到 queued）、按优先级挑选任务、`Connecting→Downloading→Merging→Verifying→Completed` 状态机、进度落库（2 Hz）、暂停/取消经事件总线中断传输、失败退避重试、下载完成后 SHA-256/BLAKE3 等校验 | 具体协议实现 |
| `ariaflow-browser` | native messaging host：接收浏览器扩展指令 → 转成 API 调用 | 下载业务本身 |

## 运行时特性开关（无需重启）

三个接口面可在运行时单独启停，**不改监听、不重启进程、不影响下载中的任务**，修改写入 `GlobalSettings`（SQLite）并持久化：

| 开关 | 字段（`GlobalSettings`） | 关闭后的行为 |
|---|---|---|
| HTTP API | `apiEnabled`（默认 `true`） | `/api/v1/*`（含 SSE、OpenAPI）返回 `503 {"error":{"code":"feature_disabled"}}` |
| MCP 端点 | `mcpEnabled`（默认 `true`，v0.1.0 起真正生效） | `/mcp` 在两条监听上都返回 `503 feature_disabled`（`--no-mcp` 时直接 404） |
| 网页 | `webUiEnabled`（默认 `true`） | `/` 返回 `503` 且为说明关闭原因的 HTML 页 |

机制（`crates/ariaflow-api/src/features.rs` + `server.rs`）：

- `ariaflow_api::Features`：对 `Api/Mcp/Web` 三个 `AtomicBool` 的廉价克隆句柄；启动时从**已持久化设置**初始化。
- `PUT /api/v1/settings` 成功落库后立即 `set_all(api_enabled, mcp_enabled, web_ui_enabled)`，此后新请求立即看到新值（原子读/写，无锁）。
- 门控是 **axum 中间件 + state-aware fallback**，按请求判定；已建立的 SSE / HTTP 连接不被强制断开。
- 与门控不同，**监听地址属于启动期配置**，只在启动时生效。生效顺序：`--api-bind` / `ARIAFLOW_API_BIND` > 持久化设置 `apiBind`（非法值记 ERROR 并回退 `127.0.0.1:8787`）> 默认 `127.0.0.1:8787`；`--web-bind` > 持久化 `webUiBind` > 不监听。`PUT /api/v1/settings` 对 `apiBind`/`webUiBind` 做格式校验，非法值 400 拒绝；改动重启后生效。
- **管道（IPC）不受 `apiEnabled` 门控**——它是本地可信控制通道：关掉 HTTP API 后仍可用 `ariaflow config set api-enabled true` 或桌面端（管道传输）重新打开。MCP 的工具回环自调用也优先走管道，关闭 HTTP API 不影响 MCP 工具。
- `webUiEnabled` 只影响 `/`（及 SPA 回退）：API 与 MCP 不受影响；关闭 `apiEnabled` 不影响网页静态文件，但网页会因拿不到 API 数据而失去功能。
- 桌面端设置页"接口开关（即时生效）"一节、CLI `config set api-enabled|mcp-enabled|web-ui-enabled`、MCP `ariaflow_update_settings` 均可写入这三个字段。

升级提示：旧库里 `mcpEnabled: false` 是此前**不生效**的 inert 值；升级后会真正生效（/mcp 返回 503）。请执行 `ariaflow config set mcp-enabled true` 恢复，或在设置页打开开关。

## 统一接口与事件

核心 trait（`ariaflow-core::service`）：

```rust
#[async_trait]
pub trait DownloadService: Send + Sync {
    async fn add(&self, url: &str, options: AddOptions) -> Result<DownloadId>;
    async fn pause(&self, id: DownloadId) -> Result<()>;
    async fn resume(&self, id: DownloadId) -> Result<()>;
    async fn cancel(&self, id: DownloadId) -> Result<()>;
    async fn remove(&self, id: DownloadId, delete_file: bool) -> Result<()>;
    async fn retry(&self, id: DownloadId) -> Result<DownloadId>;
    async fn pause_all(&self) -> Result<u64>;
    async fn resume_all(&self) -> Result<u64>;
    async fn cancel_all(&self) -> Result<u64>;
    async fn remove_completed(&self) -> Result<u64>;
    // ... 列表、详情、设置、优先级
}
```

状态变更通过 `DownloadEvent`（`download_created/started/progress/paused/resumed/completed/failed/cancelled/removed`）广播；`ariaflow-api` 把该广播流以 **SSE** 暴露给客户端（`GET /api/v1/events`），CLI 轮询，GUI 使用轮询 +（后续）SSE。

## 数据一致性

- 存储层唯一入口：`ariaflow-storage::repository`；其他 crate 不得直连 SQL；
- 迁移在启动时同事务执行，版本号严格递增，已执行过的迁移不会重复运行；
- `segments` 表记录每段 `start_offset/end_offset/downloaded_size/status`，是断点续传的事实来源；
- `events` 表保留任务生命周期审计轨迹；
- SQLite 打开时强制 `PRAGMA journal_mode=WAL` + `foreign_keys=ON`。

## 安全模型

| 威胁 | 对策 |
|---|---|
| 未授权本地/远程访问 API | 默认 `127.0.0.1` 绑定；可选 `ARIAFLOW_API_TOKEN` Bearer 认证；绑定非回环地址时必须显式配置 token（否则启动失败） |
| URL 注入 / SSRF | 内核只允许白名单 scheme（`http(s)`/`ftp(s)`/`sftp`/`magnet`/`metalink`）；http(s) 的 `.torrent` 链接同样合法（自动按 BT 处理）；URL 规范化后入库；重定向由 reqwest 跟随但记录 `final_url` |
| 文件名/路径越界 | `ariaflow-core::filename` 统一清洗：去除非法字符、去 `..`、长度上限、跨盘符前缀剥离；写入路径必须位于配置的下载目录之内 |
| 敏感信息泄漏 | Token / 代理密码只写 `api_tokens`（`token_hash`）表；日志与 MCP 输出对密码、token 做 redaction（`***`） |
| 恶意/危险操作 | MCP 的 `ariaflow_remove_download deleteFile=true`、`cancel_all` 等危险操作要求 `confirm: true`，否则直接 `isError`；HTTP API 侧删除有显式路径 |
| 依赖供应链 | 全 MIT/Apache-2.0 宽松许可证；锁文件 `Cargo.lock` 入库；CI 固定工具链（`rust-toolchain.toml`） |

## 为什么 GUI 不用 Naive UI 而用原生 CSS？

桌面端追求 macOS 原生质感（毛玻璃、系统强调色跟随），未引入组件库依赖；所有视觉令牌集中在 `apps/desktop/src/styles/theme.css`（浅色/深色双模式 + `--af-accent` 强调色），便于后续整体换肤。
