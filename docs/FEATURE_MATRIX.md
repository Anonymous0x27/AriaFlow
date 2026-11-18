# AriaFlow 功能对照：IDM 与其他优秀下载工具

> 调研日期：2026-11。方法：以 IDM 6.43 的公开功能清单与产品评测为基准，对照 FDM、aria2、JDownloader、Motrix，
> 逐项评估「是否已有 → 能否低成本移植 → 不值得/不该做」，并直接移植了优选项（见下文「本轮已移植」）。
>
> 调研来源：[IDM 6.43 功能清单（softexia）](https://www.softexia.com/internet/downloaders/internet-download-manager)、
> [IDM 功能说明（updatestar）](https://internet-download-manager.updatestar.com/llms.txt)、
> [IDM 2026 评测与替代品（mobilemasr）](https://mobilemasr.com/en/blogs/a-complete-review-of-internet-download-manager-idm-in-2026-features-and-alternatives)、
> [10 款最佳下载管理器 for Mac（itechguides）](https://www.itechguides.com/10-best-download-managers-for-mac/)、
> [9 款最佳下载管理器（techbloat）](https://www.techbloat.com/11-best-download-managers-in-2026-fast-free-and-worth-using.html)、
> [FDM vs Motrix 对比（freedom251）](https://freedom251.com/compare-free-download-manager-vs-motrix/)。

## 结论先行

1. **AriaFlow 已覆盖 IDM 的大部分核心能力**（分段下载、断点续传、浏览器接管、调度、分类、批量、限速、校验），
   并在 AI 原生（MCP）、本地管道传输、Web 与桌面三版本同根架构上领先。
2. **本轮移植了 IDM 最值得拥有的三样**：同名不覆盖、下载后钩子（IDM 杀毒检查的通用化 / aria2 `on-download-complete`）、
   粘贴/拖拽即添加（IDM 剪贴板监视与拖放的手势版）。
3. IDM 的独占功能中，要么已有等价物，要么与"下载管理器"的本质无关或平台受限（视频嗅探、站点抓取、验证码识别），
   详见下表与路线图。

## IDM 功能逐项对照

| IDM 特性 | AriaFlow | 说明 |
|---|---|---|
| 动态分段加速（dynamic segmentation） | ◑ 静态分段 | engine 按 `maxConnectionsPerTask` 一次性切分 Range，失败段重调度；下载中途动态再平衡未实现（XL 工程，见路线图） |
| 连接复用 | ◑ | reqwest 连接池复用 TCP/TLS；HTTP/2 自动协商 |
| 断点续传（If-Range/ETag、崩溃恢复） | ✅ | 段表 + `.ariaflow-part` + WAL；崩溃后中断任务回到 queued |
| 浏览器集成（一键接管、右键菜单） | ✅ | MV3 扩展（Chromium/Firefox）+ native messaging host；扩展内识别链接 |
| 下载全部链接（Download All） | ✗ | 路线图（扩展侧 contextMenu"下载本页所有链接"） |
| 调度器（定时开始/到点结束） | ✅ | `--schedule`（单任务）+ 每日时间窗口（跨午夜）+ 优先级队列 |
| 完成后自动动作（断开/关机） | ✅ 等价物 | **本轮新增** `onDownloadCompleteCommand`（如 `systemctl poweroff`）；不内置系统级动作，避免误操作 |
| 杀毒检查 | ✅ 等价物 | **本轮新增** `onDownloadCompleteCommand`（如 `clamscan "$ARIAFLOW_FILE"`） |
| 站点抓取器 / Spider | ✗ | 与下载器正交，需爬虫与过滤规则；路线图（插件化），不进核心 |
| 视频抓取（YouTube grabber） | ✗ | 需 yt-dlp 式提取器 + 潜在 DRM 风险；路线图，不内置 |
| 下载分类 | ✅ | 内置规则导出（`GET /api/v1/categories`）、自动归类、可按分类筛选 |
| 限速 / 公平访问配额（FAP） | ✅ 限速 / ✗ 配额 | 全局/单任务令牌桶限速（`global-speed-limit`、`per-task-speed-limit`、`--limit-rate`）；月配额为边缘场景，路线图 |
| 批量添加、拖放、命令行 | ✅ | `batch-add`（文件/stdin）、CLI、**本轮新增**整窗口拖拽链接 & `Ctrl/Cmd+V` |
| 剪贴板监视（自动检测复制的链接） | ◑ | **本轮新增**手势版：整窗粘贴 + 工具栏剪贴板按钮；被动监听受 WebView 权限模型限制，路线图 |
| 同名文件不覆盖 | ✅ **本轮新增** | 目标存在或已有进行中的同名任务时自动存为 `name (n).ext` |
| 下载后校验 | ✅ | SHA/BLAKE3/MD5，支持 Metalink 与响应头校验（Content-MD5 等） |
| 多语言界面 | ✅ | zh-CN / en（桌面端即时切换） |
| 代理 / 认证 | ✅ | HTTP/HTTPS/SOCKS5 命名代理、Basic/Bearer、站点登录（任务级 headers） |
| 代理 / 连接选项、防火墙友好 | ✅ | 自定义 UA/超时/CA（HTTP provider） |
| 托盘常驻、关闭窗口不退出下载 | ✅ | Tauri 托盘 + daemon 持有任务；设置页“自动退出 ariaflowd”（全局设置 `autoQuitDaemon`，默认关）可在托盘退出时同时关闭 ariaflowd |
| 保存目录文件夹选择器 | ✅ **本轮新增** | 桌面端/网页端统一走 `GET /api/v1/fs/list`（ariaflowd 提供目录列表；Tauri IPC 管道或浏览器直连），面包屑导航、隐藏文件开关、键盘操作；输入框仍可直接手动输入路径 |
| 历史记录管理 | ✅ **本轮新增** | 历史页多选删除 / 清空所有；后端 `DELETE /api/v1/history[?ids=1,2,3]`（不带 ids = 清空全部，OpenAPI 已登记） |
| 多选与批量操作（下载列表） | ✅ | 全选/半选态、批量暂停/继续/重试/删除（可勾选连文件一起删）、已完成页“清空已完成”；危险操作二次确认弹窗（复用共享 ConfirmDialog） |
| 任务详情（分段进度/当前链接/时间线） | ✅ **本轮新增** | 右键任务 →“查看任务详情”弹窗：当前下载链接（可复制）、保存路径、大小/速度/ETA/优先级/连接数、单任务限速、ETag/Last-Modified/校验、创建/开始/完成时间、失败原因；**分段进度表**（每段 Range、已下载字节、进度条、状态）——传输中/暂停态实时（读 `.seg-NNN` side files + `.segments` manifest），完成后展示持久化的段表 |
| 单任务限速 UI | ✅ **本轮新增** | 添加对话框“高级选项”限速字段（KB/s）；任务右键“设置限速…”：不限速/256K/512K/1M/5M/10M 预设 + 自定义 KB/MB +“清除任务级设置（继承全局）”（`PUT /api/v1/downloads/{id}/limit-rate`，传输中自动重启连接应用，Range 服务器断点续传、非 Range 从 0 重下） |
| 应用内右键菜单 | ✅ **本轮新增** | 全局 `contextmenu` 接管（桌面 + Web）：任务行 → 详情/暂停·继续·重试/限速/复制链接/删除（二次确认+连文件勾选）；输入框 → 剪切/复制/粘贴/全选；选中文本/链接 → 复制；原生菜单一律抑制；菜单防视口溢出、滚动/失焦/Esc 关闭 |
| 外观自定义（强调色/字号/不可选中） | ✅ **本轮新增** | 强调色：预设色板 + 取色器 + `#rgb/#rrggbb` 十六进制输入（localStorage，启动预读）；界面字号 90/100/115/130%（`documentElement.style.zoom`，含弹窗）；全局 `user-select:none`，输入框/路径/错误信息可选中复制 |
| 概览页“显示已暂停任务” | ✅ **本轮新增** | 守护进程设置 `overviewShowPaused`（`PUT /api/v1/settings`，跨重启保留）；仅影响概览列表，不污染下载页过滤状态 |
| 概览页“显示内容”自定义 + 卡片点击跳转 | ✅ **本轮新增** | “查看全部”左侧“显示内容”弹窗：7 张统计卡片（正在下载/等待中/已暂停/已完成/失败/当前速度/总下载量）可分别显示/隐藏，连同两个任务列表勾选项统一编辑；卡片隐藏状态存 `overviewHiddenCards`（daemon 全局设置）；7 张卡片均可点击跳转对应页面 |
| 概览页“显示新建的任务” | ✅ **本轮新增** | 守护进程设置 `overviewShowNewTasks`（默认 true，跨重启保留）；`filter=active` 时追加 `pending,queued,waiting,retrying`；与“显示已暂停任务”一并在“显示内容”弹窗中管理 |
| 主题首帧生效 & 复选框主题化 | ✅ **本轮新增** | `index.html` 启动脚本预读 localStorage 应用主题/强调色/圆角（修复“亮色设置暗色闪现”）；复选框统一自绘、浅/深色适配 |

## 其他工具的横向对照

| 工具 | 值得借鉴的能力 | AriaFlow 状态 |
|---|---|---|
| **aria2** | 全局总限速 `--max-overall-download-limit` | ✅ 已有（`global-speed-limit`） |
| **aria2** | `on-download-complete` / `on-download-error` 钩子 | ✅ **本轮新增** `onDownloadCompleteCommand` / `onDownloadErrorCommand` |
| **aria2** | Metalink | ✅ 已有（镜像择优 + 校验透传） |
| **aria2** | RPC 接口 | ✅ 有等价物：web API + MCP（JSON-RPC）+ 管道 |
| **JDownloader** | 事件脚本（Event Scripts） | ✅ **本轮新增**（同上钩子机制） |
| **JDownloader** | 链接嗅探/解压/验证码 | ✗ 超出范围（路线图评估 yt-dlp 子进程方案） |
| **FDM** | 内置 BT + 友好界面 | ✅ magnet + http(s) `.torrent` 链接 BT 下载（librqbit，分 piece 续传）；做种持久化列入路线图 |
| **FDM** | 原生带宽控制 | ✅ 全局/单任务限速 + 调度器并发/连接数控制 |
| **Motrix** | aria2 作为预设后端 + 干净 UI + RPC | ✅ 架构同构（自研内核替代 aria2 预设）；web/CLI/TUI/MCP 多客户端 |
| **Free Download Manager** | 浏览器拦截 | ✅ MV3 扩展直接通过 Web API 接管（不需要 native host） |

## 本轮已移植（2026-11）

### 1. 同名文件不覆盖（IDM 基础行为，防数据丢失）

- **问题**：两个任务下载同名文件时，后完成的会静默覆盖先完成的文件；且两者共享同一个 `.ariaflow-part`，可能互相损坏。
- **实现**：`Download::from_request` 之后，存储层扫描磁盘 + 所有未结束任务的 `save_path`，冲突时自动落名为
  `name (n).ext`（复用 `ariaflow-core::filename::dedupe_path_reserved`）。
- **验证**：连续添加 3 次同一 URL → `payload.txt` / `payload (1).txt` / `payload (2).txt`，三个文件均 `completed` 且内容完整。

### 2. 下载后钩子（IDM"完成后处理" / aria2 `on-download-complete` / JDownloader 事件脚本）

- **设置**：`onDownloadCompleteCommand`、`onDownloadErrorCommand`（空串清除，单行 ≤1024 字节）。
- **执行**：任务进入终态时由 ariaflowd 通过 `sh -c`（Windows：`cmd /C`）执行；分离进程、stdio 为空、10 分钟硬上限；**不会**阻塞下载。
- **环境变量**：`ARIAFLOW_RESULT`、`ARIAFLOW_ID`、`ARIAFLOW_URL`、`ARIAFLOW_FINAL_URL`、`ARIAFLOW_FILE`、`ARIAFLOW_FILENAME`、`ARIAFLOW_TOTAL_BYTES`、`ARIAFLOW_ERROR`。
- **接入面**：REST API、CLI（`ariaflow config set on-download-complete-command …`）、MCP（`ariaflow_update_settings`）、桌面端设置页"下载完成后自动执行"。
- **示例**：`clamscan "$ARIAFLOW_FILE"`（杀毒）、`notify-send AriaFlow "$ARIAFLOW_FILENAME 完成"`（通知）、
  `command -v notify-send …`，或关机：`systemctl poweroff`（自行斟酌）。
- **验证**：CLI 设置钩子 → 完成 3 个任务，钩子各执行一次并正确写出 `completed|<path>|<name>`；
  404 + `--retry 0` → `failed|probe failed: …`；MCP `""` 可清除。
- **安全**：钩子=**任意代码执行**，仅在用户显式配置后运行，默认关闭；文档与 UI 均明示。

### 3. 粘贴 / 拖拽 / 剪贴板按钮 即添加（IDM 剪贴板监视 + 拖放）

- 桌面端（以及 web 版 `/`）任意位置按 `Ctrl/Cmd+V` 粘贴含链接的文本 → 弹出预填 URL 的"添加任务"对话框；
  输入框/文本域内粘贴不受影响；多个链接取第一个并提示。
- 从浏览器把链接拖进窗口 → 同上（`dragover/drop`，`text/uri-list`）。
  Tauri 窗口 `dragDropEnabled=false`，使 WebView 走 HTML5 DnD。
- 工具栏新增剪贴板图标按钮 → `navigator.clipboard.readText()`（用户手势，符合权限模型）→ 识别链接并预填。
- 不支持的场景（无手势的被动监听、OS 级剪贴板监视）已记录为路线图。

### 4. Web 网页编译期内嵌 + 三接口面运行时启停（2026-11，rust-embed / no-restart）

- **网页内嵌**：`ariaflowd` 通过 `rust-embed` 把 `apps/desktop/dist` 烘焙进二进制（debug 惰性读盘、release 内联字节）；
  `--web-dir` 指向磁盘目录可覆盖（前端迭代时不用重编），`dist` 缺失时 release 编译直接失败，运行时无 `web/` 目录概念。
- **运行时开关**：HTTP API（`apiEnabled`）、MCP（`mcpEnabled`）、网页（`webUiEnabled`）三个面均可在
  `PUT /api/v1/settings` 后**立即生效**（原子布尔门控，503 `feature_disabled`/说明页降级），
  写入 SQLite settings 跨重启保留；**本地管道永远可用**（可随时重新打开），MCP 回环自调用也走管道。
- **接入面**：CLI（`config set api-enabled|mcp-enabled|web-ui-enabled`）、MCP（`ariaflow_update_settings` 新增 `webUiEnabled`）、
  桌面端设置页"接口开关（即时生效）"、HTTP API 本体。
- **验证**：关闭 → `curl /api/v1/status` / `/mcp` / `/` 各自 503；管道 `ariaflow status` 仍正常；重启 ariaflowd 后开关保持；
  重新打开 → 立即恢复 200。

### 5. 独立 Web 监听端口 + Linux 桌面修复（2026-11）

- **`--web-bind ADDR:PORT`（或设置 `webUiBind`）**：额外绑定一个独立端口，完整托管桌面 web 版
  （`/` 网页 + 同源 `/api/v1`），局域网浏览器零配置可直接提交/管理任务；与 `--api-bind` 解耦
  （API 可留在回环、网页对外）。认证语义与 `api-bind` 一致（回环免认证/设 token 才验证/非回环无 token 警告）；
  启动期配置，改后重启生效（`--web-bind` 覆盖设置）。web 壳本身默认同源调用 API，远程浏览器无需任何额外配置。
- **Linux 空白窗口**：桌面端在 Linux 下启动时自动设置 `WEBKIT_DISABLE_COMPOSITING_MODE=1`
  （WebKitGTK DMA-BUF 合成路径在部分驱动/合成器下渲染空白），`ARIAFLOW_WEBKIT_COMPOSITING=1` 可关闭兜底。
- **滚动条**：主内容区显式声明经典细滚动条（`::-webkit-scrollbar`），WebKitGTK 覆盖式滚动条不再"消失"。

### 14. Round-14（2026-11）：ariaflowd 监听地址持久化 + Web 响应式 + 跨平台构建

- **ariaflowd 监听地址可配置（网页 + 后台 + CLI + MCP）**：`apiBind` 设置自始存在于 settings 存储但
  **此前不接线**（改它不影响启动）。本轮起：
  - daemon 启动解析顺序：`--api-bind` / `ARIAFLOW_API_BIND` > 持久化 `apiBind`（非法值记 ERROR 并回退
    `127.0.0.1:8787`，不会 brick）> 默认 `127.0.0.1:8787`；显式参数非法 → 启动报错退出；
  - `PUT /api/v1/settings` 对 `apiBind` 做 `ADDR:PORT` 校验（空值/非法 → 400），未写坏数据库；非回环无 token 启动时 WARNING；
  - 入口：CLI `ariaflow config set api-bind 0.0.0.0:8787`、MCP `ariaflow_update_settings.apiBind`、
    `ariaflow` OpenAPI（`apiBind` 增加描述）、Web 设置页新增「ariaflowd 监听地址」卡片（含非回环警示条、回车保存、
    客户端正则校验）；保存后提示"重启后生效"；
  - 验证：单测 4 例（flag 优先 / 显式非法报错 / persisted 生效 / persisted 非法回退）；沙箱端到端——
    显式 flag 引导启动 → PUT `apiBind=0.0.0.0:8797` → 重启后无 flag 以 0.0.0.0:8797 监听；PUT `garbage`/空串 → 400；
    数据库直接植入 `garbage!!` → 启动回退 loopback 且日志 ERROR；`GET /api/v1/status` 正常。
- **IPC 选择后显示路径输入框**：根因是 Socket path 输入框被 `v-if="inTauri"` 包裹，浏览器（Web 预览）下选择
  「Pipe (IPC)」时完全不可见。现改为：选择 IPC（或 Auto）即显示，浏览器中附带说明
  （"浏览器无法连接命名管道，也不使用此路径；该设置仅桌面应用（Tauri）生效。浏览器中保存的值不会同步到桌面端。"）。
- **Web 界面响应式**：
  - ≥1200px 桌面：不变（232px 侧栏）；
  - 768–1199px 平板：76px 图标栏（隐藏文字标签/分组标题/页脚）；
  - ≤767px 手机：侧栏变为离屏抽屉（汉堡按钮开关、ESC / 遮罩点击 / 路由切换自动关闭，焦点语义 `aria-expanded`）；
    顶栏紧凑化（剪贴板按钮保留、速度药丸 ≤560px 隐藏）；内容区 padding 收窄；
  - 表格（历史/队列/详情分段）小屏横向滚动（`.table-scroll` + 最小宽度），不再硬挤压；
  - `.form-grid` 900px 以下单列；`.segmented` 等宽可换行；任务行小屏缩图标/间距、meta 换行；
    概览统计卡 auto-fit 自适应（已有）；浏览器实测 390×844 / 900×1200 截图确认。
- **musl static-pie #GP 崩溃修复（2026-11-16）**：BlackArch 用户报告 2026-11-15 产出的四个 musl 二进制
  启动即 `general protection fault`（固定偏移、error:0、早于 main，`--help` 亦崩溃）。本地以真实 musl-gcc（Debian musl-tools 1.2.5）
  复现构建/运行**均正常**，故障为发行版/工具链相关的启动期 GOT 自重定位问题（ET_DYN static-pie）。
  修复：`scripts/build-musl.sh` 强制 `-C relocation-model=static`（ET_EXEC）+ 构建后冒烟测试（host 可执行时跑
  `ariaflow --version`，失败即终止构建并给诊断）；ET_EXEC 产物已实测 `ldd` not dynamic、daemon 启动、HTTPS probe 通过。
- **跨平台**：aarch64-unknown-linux-musl 四个静态二进制**构建 + qemu-aarch64 运行验证通过**
  （`--version`、daemon 启动、`/api/v1/status`、对 `https://example.com/` 的 HTTPS probe）；
  FreeBSD 结论：传递依赖 `librqbit-dualstack-sockets` 使用 Linux 专有 `bind_device`，上游未支持，当前不可构建；
  桌面端 Tauri 因 WebKitGTK 是系统 WebView **无法**走 musl（arm64 桌面走 `aarch64-unknown-linux-gnu` + webkit2gtk-4.1）。
  详见 docs/BUILD.md「平台支持矩阵」。

### 15. 添加对话框：UA 覆盖 + 文件名实时解析 + SSH 证书认证 + 证书管理（2026-11-16）

- **#1 解析类错误改弹窗提示（toast）**：`url parse error: relative URL without a base` 等 daemon 拒绝信息此前在页面内联红字显示
  （添加对话框/批量对话框 `<p v-if="error">`、概览页/下载页快捷添加 `add-error`）。现全仓统一走共享 toast：
  新增 `lib/notice.ts`（模块级响应式 store + `showNotice()`，App.vue 顶栏 `.notice-pill` 唯一渲染处），
  AddTaskDialog / BatchAddDialog / OverviewView / DownloadsView 的所有提交类 catch 均改为 toast（含批量部分失败提示），
  内联错误元素与 `.add-error`/`.error-text` 样式已删除。
- **#2 每任务 User-Agent 覆盖（单/批量添加对话框）**：`AdvancedDownloadFields` 新增 UA 输入框——
  留空 = 继承 daemon 全局设置 `userAgent`（占位符实时显示全局值，title 提示语义），填值则单独覆盖；
  `AdvancedOptions.userAgent` / `buildAdvancedPayload()` 透传 `userAgent`（仅在非空时下发）。
  后端链路：`NewDownloadRequest.user_agent` →（migration **v5** `downloads.user_agent TEXT`）→ `Download.user_agent`
  （`Download::from_request` 复制）→ orchestrator 构造 `ProviderOptions` 时 `dl.user_agent`（trim 后非空）
  优先、否则回退 `settings.user_agent`；http.rs 既有的 `.user_agent(options.user_agent …)` 直接生效。
- **#3 文件名实时解析**：`AddTaskDialog` 的 URL 输入 `@input` 时从 URL path 推导文件名（percent-decode 的最后一段；
  空段/不可解析（magnet 等）则不动文件名框）。用户手动编辑文件名后停止自动填充（清空文件名框会重新启用自动填充）；
  对话框（重新）打开时若 initialUrl 有文件名先填入。`filenameFromUrl()` 位于 `lib/task-options.ts`（与后端
  `filename_from_url` 同语义，含 `decodeURIComponent` 回退）。
- **#3 SSH/SFTP 证书认证（“证书认证”选项）**：
  - 协议识别：URL scheme ∈ {`ssh`, `sftp`} 时认证下拉出现「证书认证」（`isSshLikeUrl()`）。**`ssh://` 自此为
    `sftp://` 的正式别名**（同一 libssh2/SFTP transport，已注册 `SftpProvider` + 协议白名单 + `Protocol::Sftp`）。
  - 认证顺序：任务选择了证书时**证书优先且唯一**（`userauth_pubkey_memory`，私钥仅在内存中流转，不落盘临时文件、
    不经 agent/`~/.ssh` 回退，避免“选了证书却静默走了别的钥匙”）；未选则维持原有三阶（URL 密码 → agent → ~/.ssh）。
  - 存储：migration **v5** 新增 `ssh_certificates` 表（id / name / private_key / passphrase / algorithm / fingerprint
    / created_at）与 `downloads.ssh_certificate_id TEXT`；`SshCertificateRepository`（CRUD + `material()` 取钥），
    入库即校验（ssh-key crate 解析 OpenSSH PEM；加密私钥必须给口令且能解开才接受；`algorithm` 与 `SHA256:` 指纹入库）；
    **私钥绝不出现在任何 API 响应/Download 序列化中**（`ProviderOptions::ssh_certificate` 带 `#[serde(skip)]`，
    `SshCertificateEntry` 只含元数据）。更新语义：`privateKey` 空 = 保留原钥（仅改名/口令），重粘贴 = 替换并重算指纹；
    口令留空 = 保留（加密钥清空口令被显式拒绝，防止把钥“弄坏”）。删除有引用的证书 → 任务下次运行即永久失败并提示原因。
  - API：`GET/POST /api/v1/certificates`、`PUT/DELETE /api/v1/certificates/{id}`（复刻 proxies 的 handler/路由模式；
    仅回元数据：name/algorithm/fingerprint/hasPassphrase）；OpenAPI 已同步（见下方“已知限制”条）。
  - orchestrator：构建 options 前按 `dl.ssh_certificate_id` 解析证书（`SqliteDownloadService::ssh_certificate_material`）；
    证书不存在 → `Stage::FailedPermanent`（不重试）；读取失败 → 可重试失败。
  - 重试语义修正（顺带修复既有缺陷）：`permanent_probe_error` 此前**只影响错误文案、不影响是否重试**，
    导致 401/403/404 与本场景的“证书不存在”也被打满重试预算。现新增 `Stage::FailedPermanent`，
    永久错误（永久 4xx/Authentication/Unsupported/证书不存在）直接转 `Failed`（记录历史、发事件、触发 error hook），
    不再空转重试；`handle_failure` 的“重试预算耗尽”分支与之共用 `fail_permanently()`。
  - 前端：`AdvancedDownloadFields` 认证下拉按 `sshLike` 追加「证书认证」；选中后出现证书选择器
    （AriaSelect，标签 = 名称 · 算法；列表为空时提示先去 设置 → SSH 证书 添加）；`certificateAuthError()` 在
    两个对话框提交前拦截“选了证书认证却未选证书”并 toast。批量对话框同样支持（任一行是 ssh/sftp URL 即出现该选项）。
  - **设置 → SSH 证书**（`SettingsView` 新 section，proxies 同款表格/表单）：表格（名称/算法/SHA256 指纹(截断+title)/
    口令 ✓/编辑/删除）；添加 = 名称 + 粘贴 OpenSSH PEM（ed25519/rsa/ecdsa，支持口令保护）+ 可选口令；
    编辑 = 改名随手可，私钥留空保留、重粘贴替换（指纹重算）；表格与任何响应都不含私钥；删除即硬删除。
- **验证**：cargo test（storage 新增 `certificate_tests::create_list_update_delete_roundtrip`——真实 ed25519 未加密/
  加密两把夹具钥（tests/fixtures/，仅测试用）、口令错误/口令缺失 400、改名保指纹、替换重算、加密钥禁清空口令、
  NotFound 路径；migration v1→v5 升级测试扩展，全绿）；`cargo check --workspace` 无警；`vue-tsc` 与 `vite build` 通过。
  **真实 SFTP 端到端**（沙箱内 sshd，仅放行测试 pubkey，`PasswordAuthentication no`）：上传证书 → `sshCertificateId` 添加
  → 任务 `completed` 且文件字节级一致；加密私钥 + 口令全程同样通过；无证书/bogus 证书 id → 立即 `failed`（不重试）；
  per-task UA 实测命中本地 UA 回显服务器（override / 回退全局 Chrome UA 均确认）。**headless Chromium 全流程**：
  概览输入 `example.com/no-scheme.zip` + 添加 → toast 显示 "url parse error: relative URL without a base"、页面无内联错误；
  高级对话框 `sftp://` URL → 文件名自动填 `payload.bin`、UA 占位 = 全局 UA、认证出现「证书认证」、未选证书提交被 toast 拦截；
  设置页添加证书（指纹入库展示、私钥不渲染）→ 回对话框选证书添加 → 任务完成（字节一致）；批量对话框同 UA 字段/证书选项；
  预览 8792 已重启加载本次产物（旧库自动迁移 v4→v5）。headless Chromium 复跑（fresh DB + 独立 sshd，ariaflowd 新构建）：
  **16/16 PASS**（含 “no console errors”——第 1 步故意输入的非法 URL 所触发的预期 400 已在断言中精确过滤）；
  任务落库 `sshCertificateId` 已设置、cert 任务经真实 SFTP 完成且字节一致（ua-ui-test.bin 404 为预期失败，UA 覆盖字段 AriaFlowUI/1.2.3 已持久化）。
  测试环境备忘：sshd StrictModes 拒绝将 `/tmp` 下目录作为 authorized_keys 所在层级（“bad ownership or modes for directory /tmp”），
  须把 authorized_keys 放 home 下（或 StrictModes=no），否则同钥 manual ssh 亦会 publickey 拒绝。
- **已知限制 / 不做**：私钥**明文**存本地 SQLite（与 proxies 密码同级待遇；无主密钥体系，不做半吊子加密）；
  API 不提供导出私钥接口；引用任务不阻止删除证书（删除后引用它的任务失败并提示，符合“本地单用户”语义）；
  CLI/MCP 未暴露证书/UA 字段（保持保守范围）；OpenAPI（crates/ariaflow-api/assets/openapi.json）已同步：
    certificates 两组路由 + SshCertificateInput/SshCertificateEntry 模式 + NewDownloadRequest/Download 新增 userAgent/sshCertificateId。

### 16. 桌面端打包：linuxdeploy/FUSE 故障修复 + `scripts/build-desktop.sh`（2026-11-16）

- **AppImage 打包失败** `failed to run linuxdeploy`（BlackArch 用户）：根因 = tauri 构建期下载的
  `linuxdeploy` / `linuxdeploy-plugin-appimage` 本身是 AppImage，运行需要 FUSE（`/dev/fuse` + `libfuse.so.2`），
  Arch 系默认无 `fuse2`；另一硬性依赖 `patchelf` 缺失时同样失败于该步骤。修复均已写入 docs/BUILD.md
  （"桌面端打包"一节，含按发行版安装命令、`APPIMAGE_EXTRACT_AND_RUN=1` 免 FUSE 方案、`~/.cache/tauri` 缓存损坏重建、
  以及 `--bundles deb` / `--no-bundle` 兜底）。
- **“静态编译”结论（文档化，非新代码）**：Linux 桌面端**无法**完全静态——WebKitGTK 是系统动态组件且没有静态发行版，
  musl 目标不被 Tauri 支持；桌面端“自包含分发”的唯一形态 = AppImage（linuxdeploy 打包 webkit 家族 .so）。Rust 侧本已
  静态（rustls / vendored-openssl / bundled sqlite）。Windows 可 `+crt-static`。详见 BUILD.md。
- **新增 `scripts/build-desktop.sh`**：前端构建 → `cargo tauri build` → 产物体检（`file` + `ldd` 残余动态库列举 + 安装包路径）。
  `--check` 只诊断不构建（含 webkit2gtk-4.1/patchelf/fuse 三项检测与按发行版修复提示，BlackArch 专属提示走 `ID`/`ID_LIKE` 解析）；
  `/dev/fuse` 或 `libfuse.so.2` 缺失时自动 `export APPIMAGE_EXTRACT_AND_RUN=1`；`--bundles` 可多选、`--no-bundle`、`--debug`；
  `-h` 打印用法。`bash -n` 通过；`--check` 在 headless Debian 沙箱（缺 cargo/webkit）正确识别缺失并退出 1；
  依赖打桩后复验“仅缺 patchelf”分支输出完整 AppImage 链诊断并退出 1。

### 17. 桌面端：`WEBKIT_DISABLE_DMABUF_RENDERER` 兜底 + AppImage 打包链自解压默认 + FUSE 误判修复（2026-11-16，第二轮）

- **DMA-BUF 兜底**：BlackArch 用户反馈启动还必须 `WEBKIT_DISABLE_DMABUF_RENDERER=1 npm run tauri:dev`（WebKitGTK ≥ 2.42 的
  DMA-BUF 渲染路径在部分 NVIDIA/Mesa 驱动上崩溃/黑屏）。现在与既有 `WEBKIT_DISABLE_COMPOSITING_MODE=1` 兜底同构：
  Linux 下启动自动设置、用户显式设置优先、`ARIAFLOW_WEBKIT_DMABUF=1` 可关闭兜底。cargo check 级验证受限（沙箱无 webkit2gtk-4.1），main.rs 已 `rustfmt` 解析验证。
- **`scripts/build-desktop.sh` FUSE 误判修复**：旧实现把 `/dev/fuse` 挂在 `ldconfig -p` 成功之后——但 `ldconfig` 不在 PATH /
  `ldconfig -p` 无输出时 `LIBFUSE_OK=0`，即使 `/dev/fuse` 和 `libfuse.so.2` 都在也判"不可用"。而且检测出"可用"反而走直接挂载——
  在硬化内核上挂载恰恰是会失败的路径。修复：① libfuse 检测 = ldconfig（有 awk 取路径）或常见多架构库目录 glob 兜底，仅供参考展示；
  ② **打包默认一律 `APPIMAGE_EXTRACT_AND_RUN=1`**（用户设 0 强制挂载不推荐并警示；设非 0 沿用），检测结果不再决定行为；
  ③ `--check` 打印完整诊断（含两个值分别来自哪里）；④ `set -u` 下残留的一个 `FUSE_OK` 未定义变量引用已消除。
  已用依赖打桩复验四个场景（ldconfig 有/无 libfuse、用户设 0/1），全部干净退出。
- **linuxdeploy 仍失败时的处置**（次生可能，非 FUSE 问题）：缓存 `~/.cache/tauri/` 里的 linuxdeploy 因 GitHub 下载损坏时，
  直接挂载和自解压都会败。诊断：`file ~/.cache/tauri/linuxdeploy-x86_64.AppImage`（应为 ELF）；修复：`rm -rf ~/.cache/tauri` 重试。
  BUILD.md 故障表已更新。

### 18. 桌面端：ld.so.cache 的 VMware 污染导致 gtk 插件失败 + 构建脚本 pipefail/SIGPIPE 修复（2026-11-16，第三轮）

- **`failed to run …/linuxdeploy-07333c6-x86_64.AppImage`（`ERROR: Failed to run plugin: gtk`）根因**：
  linuxdeploy gtk 插件把 `libgobject-2.0.so.0` 解析到 **VMware Workstation 注入的私有库**
  （`/usr/lib/vmware/lib/libgobject-2.0.so.0/libgobject-2.0.so.0`），该极旧版本 DT_NEEDED 含 `libffi.so.6`
  （现代系统只有 libffi.so.8）→ `Could not find dependency: libffi.so.6` → gtk 插件 exit 1 → 打包失败。
  “dpkg-query 找不到 copyright” 是良性警告，无关。**修复**：`grep -ri vmware /etc/ld.so.conf /etc/ld.so.conf.d/` →
  把注入的 `vmware.conf` 移到 `/root/` 备份 → `sudo ldconfig` → `ldconfig -p | grep libgobject` 确认指向系统库 → 重跑打包。
  VMware 自身用自带库搜索路径，移出全局缓存不受影响。
- **`scripts/build-desktop.sh` 的两个 pipefail/SIGPIPE 缺陷**：`LIBFUSE_SO=$(ldconfig -p | awk '…exit')` 在
  `pipefail` 下，awk 提前退出向 ldconfig 发 SIGPIPE（141）→ `set -e` 直接终止脚本（用户实测“脚本无法正常运行”的真因）；
  同类隐患 `ldd | awk '{print}' | head -12`（head 提前退出杀 awk）一并修复（awk 自己 `NR<=12` 截断）。
  libfuse 检测改为 **先完整读取 ldconfig 输出到变量、再用 here-string 提取**（用户提供的修复方案），并保留目录 glob 兜底；
  已用“60k 行输出、libfuse 在前”的 stub 验证旧法必死、新法 exit 0。
- **脚本新增 VMware 污染预检**：ldconfig 解析的 `libgobject-2.0.so.0` 路径含 `vmware` 时，`--check` 打印警告 +
  完整修复命令（不自动改系统配置——构建脚本不替用户动 /etc）；打包时另预置 `LD_LIBRARY_PATH`
  （系统库目录优先，best-effort，`ARIAFLOW_NO_LD_LIBRARY_PATH=1` 可关）。打桩验证：污染场景警告正确打印、非污染场景静默通过。

## 路线图（已调研、未实现，按优先级）

| # | 项 | 工作量 | 备注 |
|---|---|---|---|
| 1 | 完成时**系统通知**（Web Notification API / Tauri plugin-notification） | S | 通知权限需用户授权；`onDownloadCompleteCommand` 已可零成本等价实现 |
| 2 | 浏览器扩展"下载本页全部链接"（Download All） | M | 扩展 contextMenu + `document.links`；native host 支持 `download-many` |
| 3 | HTTP 动态分段再平衡（IDM dynamic segmentation） | XL | 需重写 range 分配器 + 段状态机；收益集中在高延迟/限速镜像站 |
| 4 | 被动剪贴板监视（复制即弹泡询问） | M | 需要 OS 级监听（Tauri 无此能力）或轮询 + 权限；隐私与打扰权衡 |
| 5 | BT 做种持久化（ratio/seed time） | M | librqbit 具备基础能力，需任务模型 + 停止条件 |
| 6 | 月下载配额（FAP） | S | 统计 + 调度器判断窗口；边缘场景 |
| 7 | 重复 URL 提醒（add 时返回 409 + 现有任务 id） | M | 破坏性 API 变更，需客户端同步升级 |
| 8 | yt-dlp 提取器（视频站点） | XL | 子进程方案规避 DRM/许可证问题；法律灰色，需谨慎 |
| 9 | 站点抓取器（Site Grabber） | XL | 独立插件；与核心正交 |
| 10 | 验证码识别 / premium 账号（JDownloader） | XL | 明确不做（合规与维护成本） |

### 6. 右键菜单 + 任务详情 + 单任务限速 UI（2026-11）

- **全局自定义右键菜单**：`useContextMenu`（AppContextMenu.vue）统一处理 `contextmenu`——任务行（详情/暂停·继续·重试/限速/复制链接/删除）、输入框（剪切/复制/粘贴/全选）、选中文本与链接（复制）；其余位置一律抑制原生菜单；位置处理带防溢出；
  Chromium 在输入框上右键时会紧跟一个 caret-scroll `scroll`，菜单在其上方 150ms 内被忽略，否则菜单会“闪一下消失”（真机复现并修复）。
- **任务详情弹窗**：数据来自 `GET /api/v1/downloads/{id}`（1s 轮询）。分段进度两条数据通路：
  1. **实时**——段 side files（`{part}.seg-NNN`）大小 + 引擎启动时写入的 `{part}.segments` manifest（连接预算影响分段计划，运行时重算不可靠，故落盘）；暂停/在途任务即时可见；
  2. **持久化**——引擎 merge 前把段表经 `TransferOutcome.segment_snapshot` 返回，orchestrator 经 `DownloadService::replace_segments` 写入 `segments` 表，完成后详情弹窗永久可见。
- **单任务限速**：`PUT /api/v1/downloads/{id}/limit-rate`（`null` 清除→继承全局 `perTaskSpeedLimit`；`0` 显式不限速；缺省键 400；进行中任务自动 pause→resume，Range 续传 / 非 Range 重头下）。UI 两处入口：添加对话框高级字段（KB/s）+ 右键“设置限速…”对话框（预设 + 自定义 KB/MB）。
- **概览页暂停任务开关**：`overviewShowPaused` 持久化在守护进程 settings 表（非 localStorage），重启后保留；仅影响概览列表请求（`status=downloading,connecting,merging,verifying,paused`），不污染共享过滤状态。
- **验证**：headless Chromium 全流程（context-menu 打开/关闭、详情弹窗 4 段实时 + 6 段持久化、512K 预设 PUT 落库、inherit 清除、添加对话框 256 KB/s → 262144 B/s 落库、删除连带文件、外观四项持久化），见提交附带的 CI 记录；cargo fmt/clippy/test 全绿。
### 7. 删除确认全局默认 + 详情弹窗操作按钮 + 弹窗/页面动画（2026-11）

- **全局“同时删除已下载的文件”**：daemon 全局设置 `deleteFilesOnRemove`（GlobalSettings，SQLite 持久化；旧库缺键按 false）。作用范围 = 所有“删除任务”确认框里“同时删除已下载的文件”复选框的**默认勾选值**——`:checkbox-default="store.deleteFilesOnRemove"`，单次删除可覆盖；历史记录清空/删除**不受影响**（仍然只删记录、不删文件）；不是强制删除。设置页入口在下载区（User-Agent 行之后），保存走 `saveSettings()` 白名单；桌面/Web 共享同一份全局设置。
- **任务详情弹窗**：正文禁用文字选择**仅 Tauri 桌面端生效**（`.af-selectable` 按 `!isTauri` 绑定），Web 版保持可选中（内容区靠复制按钮）。弹窗底部新增两枚按钮：**开始任务**（resume；status ∈ pending/paused/waiting/retrying 时启用）与**暂停任务**（status ∈ queued/connecting/downloading 时启用）；busy 期间禁用，失败原因显示在按钮左侧 footer；没有“关闭任务”按钮。成功操作后弹窗内容 1s 轮询自动刷新。
- **动画**：桌面 + Web 共用同一份 Vue 代码。所有弹窗统一——backdrop 淡入 0.18s；面板 `dialog-in`（fade + translateY + scale 1.12，0.22s easing）；覆盖 `.dialog-backdrop/.dialog`（通用弹窗）、`.cd-overlay/.cd-panel`（确认框）、`.fp-overlay/.fp-panel`（目录选择器）。路由切换 `<router-view>` 包 `<Transition name="page" mode="out-in">`（opacity + translateY，0.17s）。`prefers-reduced-motion: reduce` 时全部动画禁用。
- **验证**：headless Chromium —— 全局开关 round-trip（默认 false / PUT true / PUT false）；删除确认框默认勾选随开关变化且可单次覆盖（取消勾选后文件保留、任务删除）；详情弹窗两枚按钮按状态启用/禁用，且“开始任务”真实恢复暂停中的任务、“暂停任务”将其重新暂停；backdrop/panel 动画 `animationName` 计算值核对；设置页勾选→保存→GET 落库（true→false 双向）；路由切换 Transition 类挂载；无 console error。截图核对详情弹窗 footer 与设置页 checkbox 行对齐（修复 `.check-row` 继承 column 方向导致竖排居中的问题）。cargo fmt/clippy/test 全绿；web 预览走查通过（`http://<host>:8792/#/settings`）。Tauri 桌面端 `user-select` 行为需 BlackArch 重建后实机确认。
### 8. 下拉框主题化 + 弹窗关闭动画 + API 频率限制修复（2026-11）

- **`<select>` 完全主题化**：全局 `select.af-input` —— `appearance: none` 去掉浏览器/系统原生下拉外观；`background-color: var(--af-panel-solid)`、`color: var(--af-text)`、`:hover` 边框、`:focus` 沿用 `.af-input:focus`（accent 光环）；右侧 13px 自绘 chevron（内联 data-URI SVG，中性灰 `#8e8e93`，浅/深色主题均可辨识，无需两套资源）；`option`/`optgroup` 固定面板底+主题文字色；`option:checked` 以强调色高亮；`font-family: inherit`。覆盖全部 4 处 `<select>`（添加对话框 认证/代理、限速弹窗 单位、设置页 代理类型），桌面 WebKitGTK 与 Web 端一致。`color-scheme` 已随主题切换（theme.css），原生弹层跟随应用而非 OS。
- **所有弹窗/弹出层关闭动画**：6 处弹层全部包进 Vue `<Transition>`——通用弹窗 `name="dialog"`（TaskDetail / AddTask / BatchAdd / SpeedLimit + ConfirmDialog(.cd) + DirPicker(.fp)）、右键菜单 `name="menu"`、复制提示 `name="toast"`。新增 keyframes `backdrop-out`（淡出 0.18s）/`dialog-out`（fade+translateY(10px)+scale(.97) 0.18s）/`menu-out`（淡出+上移 4px 0.12s）/`toast-out`（淡出+下移 6px 0.18s）；关闭快于打开（打开 0.22s）；离开期间 `pointer-events: none`。右键菜单/提示的关闭动画选择器用**三元类**（`.context-menu.context-menu.menu-leave-active`）压过组件 scoped 的 `menu-in`（同为 0-2-0、作用域样式后加载），保证关闭时必定播放 out 动画。`prefers-reduced-motion: reduce` 时全部关闭动画禁用（含离开类）。
- **API "rate limited" 修复**：受保护路由按 IP 令牌桶限流原为 突发 120 / 2 次秒 —— 本机 UI（列表+状态 2 req/1.5s，详情弹窗额外 1 req/s，多个窗口共享 127.0.0.1）轻松打满 → 桌面端与 Web 端频繁弹出 "rate limited"。修复：① **回环地址不限流**（本机 UI 是合法流量；远程 token 认证逻辑不变）；② 非回环配额上调为 突发 600 / 持续 10 次秒（600 req/min）。单元测试覆盖回环豁免与非回环耗尽拒绝；curl 实测 150 连发全部 200。
- **验证**：headless Chromium —— select 计算样式（appearance=none、data-URI chevron、≥2 个 select）、5 种弹层关闭动画（leave 态 opacity<0.95 且元素随后移除、toast/menu 动画名=toast-out/menu-out）、Round-7 详情弹窗开始/暂停按钮回归、pw-verify 全量回归 ALL CHECKS PASSED、cargo clippy/test 全绿（+2 限流单测）。截图核对浅色/深色主题下拉框与限速弹窗。
### 14. 已完成文件的“保存/另存为”全链路 + 托盘退出提速 + 手机布局 + 交叉构建 (2026-11-16)

- **任务 5 · 已完成文件获取**：
  - **API 新增 4 端点**：`GET /api/v1/downloads/{id}/file`（已完成任务 → `application/octet-stream` + `Content-Disposition: attachment`（ASCII 兜底 + RFC 5987 `filename*`）+ `Accept-Ranges: bytes`；支持单个 `Range` → 206 + `Content-Range`；未完成 → 409 `not_completed`；文件缺失 → 404 `file_missing`；多区间/非法 Range → 200 全量或 416）、`POST /api/v1/downloads/{id}/save-as`（守护进程侧复制到 `destDir`：`{destDir, filename?, overwrite?}`；目标存在且未 `overwrite` → 409 `already_exists`；源=目标 → 400 `same_file`；`filename` 拒绝路径分隔符/控制字符/`..`；响应 `{savedTo, size}`）、`GET /api/v1/history/{id}/file`、`POST /api/v1/history/{id}/save-as`（历史记录版，**任务行被删除后仍可用**；仅 `result=completed` 放行，否则 409）。OpenAPI 同步 4 个 operation + `SaveAsRequest`/`SaveAsResponse` schema。
  - **MCP 新增 2 工具**（共 12 个）：`ariaflow_get_download_file`（base64 内联，默认上限 1 MiB、硬上限 10 MiB，适合小文件；超限时引导用 `ariaflow_save_download`）、`ariaflow_save_download`（daemon 侧复制到指定目录，适合任意大小；服务端 409/400 原样透传为 tool 错误）。stdio 与 `/mcp` 两种传输实测通过。
  - **Web / 桌面 UI**：任务行（Downloads/概览 共用 TaskRow）与下载历史（HistoryView）对 `completed` 任务显示 ⬇（fetch+blob 触发浏览器下载，携带 Bearer token）与 💾（“另存为…”对话框：目标目录预填全局下载目录 + DirPicker 浏览 + 文件名 + “覆盖同名文件”复选；`SaveAsDialog` 按 `kind: task|history` 路由到对应端点）。保存成功 toast 提示含完整路径。
  - **验证**：curl 矩阵（206/200/416/404/409/already_exists/same_file/invalid_filename 全覆盖）；浏览器实测（历史行 ⬇ 触发下载、💾 另存为对话框含“源=目标同名文件”行内报错、覆盖后保存成功、字节级一致）；MCP stdio + daemon 内嵌 `/mcp` 实测（12 工具列举、base64 解码一致、maxBytes 拒绝路径、409 透传）；`cargo test -p ariaflow-api -p ariaflow-storage -p ariaflow-mcp` 全绿；`vue-tsc`+`vite build` 干净；openapi 由 `include_str!` 编译期嵌入，已重建 daemon 验证。
- **任务 1 · 托盘“退出”提速**：原链路 `settings() fetch → quitDaemon()（SIGTERM 后最多 3s 轮询）→ exit` 串行阻塞。修复：① `autoQuitDaemon` 在每次 settings 读/写时缓存进 `localStorage`，托盘退出直接读缓存（未命中时以 1.2s 超时探测，失败按 false 处理——与“关窗不停下载”语义一致）；② 新增 `ariaflow_quit_daemon({ wait: false })`：仅解析 pid、process_alive 校验、发 SIGTERM 后**立即返回**（Rust 端抽出 `signal_daemon_stop()` 非阻塞路径，pid 文件读取/陈旧清理与原 3s 等待版逻辑共享）；③ 退出前后台预取 settings 以保持缓存新鲜。daemon 仍自行 graceful 关闭，UI 不再等待。
- **任务 2 · 下载总览移动端布局**：`.add-bar` 增加 `.af-btn{flex-shrink:0;white-space:nowrap}`、`.add-input{min-width:0}`（flex item 默认 `min-width:auto` 导致输入框推按钮溢出）；≤560px 改为两行布局（输入框独占整行、两枚按钮各占一半、隐藏图标）；媒体查询块移到 scoped style 末尾（Vue SFC 按源码顺序输出，靠后才能覆盖 base 规则，修复首版 patch 中媒体规则被 base 覆盖不生效的问题）。Playwright 375/768/1280 三宽度实测：bar `scrollWidth==clientWidth`、按钮等宽不溢出。
- **任务 3 · 运行时 so 依赖盘点**：实测 glibc release 四二进制仅依赖 `libc/libm/libgcc_s/ld-linux`（**零 OpenSSL/zlib/sqlite3 动态依赖**——rustls + bundled-sqlite + vendored SFTP 链生效）；文档化：Linux 想归零 → musl 产物；Linux 桌面端无法静态（WebKitGTK 无静态分发，AppImage 为唯一“目标机零安装”途径）；Windows CLI → `build.sh` 对 `*-pc-windows-msvc` 默认附加 `-C target-feature=+crt-static`（`--no-static-crt` 可关）；详见 docs/BUILD.md「运行时 so 依赖盘点」。
- **任务 4 · 交叉构建参数**：三个脚本统一 `--target <triple>`（musl 脚本另保留 `TARGET_TRIPLE`，CLI 优先）：`build.sh` 自动 `rustup target add`、常见目标自动配 `CARGO_TARGET_*_LINKER/CC_*`（aarch64-linux-gnu、armv7-linux-gnueabihf、i686-linux-gnu、riscv64gc-linux-gnu、x86_64-pc-windows-gnu），musl/msvc/darwin 目标给出明确引导错误，产物路径随 target 变为 `target/<triple>/release`；`build-desktop.sh` 的 `--target` 透传给 `tauri build --target`，非 Linux 目标自动跳过 webkit2gtk/FUSE/patchelf 前置检查与 AppImage 打包（降级 `--no-bundle`），产物体检跳过 `ldd`；桌面端跨 OS 打包本质需目标 OS 主机，引导走 CI 矩阵。

### 13. Linux 静态 musl 产物 + TLS 全仓切换 rustls（2026-11）

- **`scripts/build-musl.sh`**：一键产出完全静态的 4 个二进制（`ariaflowd`/`ariaflow`/`ariaflow-mcp`/`ariaflow-native-host`，`--target <triple>` 或 `TARGET_TRIPLE` 可覆盖目标）；自动 `rustup target add`、探测/校验 musl C 工具链（Alpine 原生 musl、Debian `musl-tools`、Fedora `musl-gcc`）、先构建前端再 release 构建（rust-embed 内嵌 web UI），最后打印产物类型与 `ldd` 提示。
- **TLS 全仓移除 OpenSSL 依赖**：reqwest 0.12 → `rustls-tls-webpki-roots`（Mozilla roots 内嵌）；suppaftp → `rustls-ring`（FTPS 显式 AUTH TLS，`danger_accept_invalid_certs` 走自定义“接受任意证书”校验器，与 HTTP 语义一致、用户显式 opt-in，默认仍校验）；librqbit → `rust-tls`（其 reqwest 0.13 默认本就 rustls）；SFTP（ssh2→libssh2-sys）Unix 下 `vendored-openssl` 源码构建；zlib（libz-sys）Unix 下 `static` 源码构建；SQLite 本就 bundled。**全仓 `cargo tree` 中 native-tls = 0**，OpenSSL 仅随 SFTP 静态内嵌、无系统依赖，musl 直接可构建。
- **行为说明**：静态产物内嵌 Mozilla CA 根，不再读系统 `/etc/ssl/certs`（自建 CA/企业 TLS 拦截场景需在系统安装 roots 的动态构建中使用，或逐任务 `--header` 处理）；其他平台（glibc/macOS/Windows）此前也统一走 rustls，运行时零外部 TLS 依赖，行为与之前一致（公开可信 CA 全部正常）。
- **验证**：glibc 侧 `cargo check --workspace` 通过、`cargo test --workspace` 18 个测试套件全绿、clippy/fmt 干净；**musl 侧实测**：`cargo check --workspace --target x86_64-unknown-linux-musl` 全量通过、四个二进制完整链接（静态，`ldd` = not a dynamic executable）、`ariaflowd 0.1.0` 静态运行、**实际 `https://example.com/` probe 与下载成功**（rustls+webpki 链路）、`--version` 正常。沙箱无 musl-gcc，最终链接验证使用 zig cc 作为 musl C 工具链（与真实 musl-gcc 同一代码路径；真实 musl-gcc 无需 `link-self-contained=n`）。

### 12. 桌面“自动退出 ariaflowd”开关 + 初始 User-Agent 改为 Chrome UA（2026-11）

- **桌面设置新增“自动退出 ariaflowd”**：设置页“后台守护进程”一节（仅 Tauri 桌面壳显示）新增开关，文案为“自动退出 ariaflowd”——开启后，托盘菜单“退出 AriaFlow”会先关闭本机 ariaflowd 再退出桌面程序（正在进行的下载会被中断）。设置存于**全局设置** `autoQuitDaemon`（SQLite settings 表，默认 `false`；`PUT /api/v1/settings` 持久化，失败回滚；网页模式无效仅不显示）。实现链路：托盘“退出 AriaFlow”不再直接 `app.exit`——壳层改为向前端 `emit("ariaflow-quit")` 并启动 5s 看门狗强制退出；前端收到事件后读全局设置（管道/HTTP 均可），`autoQuitDaemon=true` 时先 invoke `ariaflow_quit_daemon`（读 pid 文件、SIGTERM 优雅关闭 ariaflowd，最多等待 3s、回收 stale pid），最后 invoke `ariaflow_exit` 退出。任何一步失败都不会卡住退出（看门狗兜底）。CLI：`ariaflow config set auto-quit-daemon true`；MCP：`ariaflow_update_settings` 支持 `autoQuitDaemon`。
- **初始 User-Agent 改为 Chrome 桌面 UA**：`GlobalSettings` 初始 `userAgent` 由 `AriaFlow/<version>` 改为 `Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/154.0.0.0 Safari/537.36`（`DEFAULT_USER_AGENT` 常量）；出站下载（HTTP provider）、Metalink 拉取、以及 `/api/v1/probe` 探测（此前硬编码 `AriaFlow/{API_VERSION}`，现优先用全局设置 `userAgent`、缺失/空回退新默认）全部统一。**已落库的 userAgent 不自动迁移**（“初始设置”语义），新库/缺失该键时用新值；预览库已通过 API 同步为新 UA。
- **验证**：cargo check/clippy 全绿（core/engine/api/cli/mcp）、`cargo test -p ariaflow-core settings` 8/8（新增 `auto_quit_daemon_defaults_false_and_patches`、`default_user_agent_is_chrome_desktop`）；quit 逻辑以独立 harness 在隔离 XDG 环境验证（无 pid 文件→跳过、存活进程 SIGTERM→3s 内退出并删 pid、stale pid→清理、非法 pid→报错不 panic）；fresh daemon（全新 XDG 目录）`GET /api/v1/settings` 回读 `userAgent`=Chrome UA、`autoQuitDaemon`=false；`PUT autoQuitDaemon` true→GET true→false 往返一致（preview.db 回读）；前端 `vue-tsc + vite build` 通过（dist 含 `ariaflow-quit` 监听与中英文案）；headless 复验设置页无 console error。Tauri 实机（托盘退出全链路）需 BlackArch 重建后确认（沙箱缺 webkit2gtk-4.1，shell 仅通过 rustfmt 与 quit 逻辑隔离编译验证）。

### 11. 概览页卡片跳转 + “显示内容”弹窗 + CLI --bin + TUI 扩展 + 目录占位 + 文本域修复（2026-11）

- **统计卡片可点击跳转**：7 张卡片统一改为 `<button class="stat-card">`（`:focus-visible` accent 描边、hover 抬升）；路由映射：正在下载→`/downloads/active`、等待中→`/downloads/queued`、已暂停→`/downloads/paused`、已完成→`/downloads/completed`、失败→`/downloads/failed`、当前速度→`/downloads/active`、总下载量→`/history`。
- **“显示内容”弹窗**（“查看全部”左侧，滑块图标）：原“显示新建的任务/显示已暂停任务”内联勾选框**取消**，统一收入弹窗两个分组——「统计卡片」7 项开关（`overviewHiddenCards: string[]`，daemon 全局设置、`PUT /api/v1/settings` 持久化、未知 id 静默过滤/去重，服务端 `KNOWN_OVERVIEW_CARDS` 校验）与「“正在进行”列表」2 项（复用 `overviewShowNewTasks`/`overviewShowPaused`）；卡片网格 `repeat(auto-fit,minmax(112px,1fr))` 自适应剩余数量；离线时开关禁用、卡片全部显示；失败回滚、1.5s 轮询自动刷新。关闭走完成按钮/backdrop/Transition。
- **`ariaflow daemon start|restart --bin <PATH>`**：显式指定 ariaflowd 路径（ariaflowd 不在常用路径时可用）；指定路径必须是文件否则报错。**先校验 --bin 再停服务**（此前 `restart --bin <bad>` 会先停旧进程再失败——已修复并回归）；未指定时查找顺序：`ariaflow` 同目录 → PATH（与原一致）。
- **TUI（`ariaflow --tui`）扩展**：`/` 搜索过滤（服务端 `DownloadFilter.search`，列表标题与状态行回显，Esc 清除）、`s` 循环排序（created/name/size/speed/progress/priority）、`t` 重试失败/已取消任务、`+`/`-` 调优先级、`c` 清空全部已完成任务（二次确认）、`?` 快捷键覆盖层、`Home/End`/`g/G` 跳首尾、页眉新增 act/wait/done/fail 计数。
- **保存目录占位符 = 全局设置目录**：添加/批量添加对话框“保存目录”留空语义不变（= 守护进程全局下载目录），占位符由 `/path/to/folder` 改为 `settings.downloadDir`（实时读取、读取失败回退原值），title 提示继承语义。
- **多行文本框不可手动拉伸**：批量添加“下载链接”（`height:15vh`、overflow-y:auto）与“请求头”（`height:8.5vh`、overflow-y:auto）均 `resize:none;width:100%`，文本超出时内部滚动。
- **验证**：headless Chromium —— 7 张卡片均为 BUTTON、点击“Completed/Failed”跳转 `#/downloads/completed|failed`；“显示内容”弹窗分组与 9 项开关渲染；取消勾选“Waiting”→ `PUT overviewHiddenCards:["waiting"]`、卡片 7→6 枚、重新勾选复位 `[]`；弹窗内“Show new tasks”切换 PUT `overviewShowNewTasks` true→false→true（GET 回读一致）；单/批量添加保存目录占位符均为全局目录（`/home/user`）；两处 textarea 计算样式 resize=none、overflow-y=auto；暗/明双主题截图核对。cargo: 新增 `hidden_cards_patch_filters_unknown_and_dedupes` / `hidden_cards_absent_patch_leaves_settings_unchanged` 单测、`restart --bin` 校验顺序修复编译通过；前端 vue-tsc + vite 构建通过；openapi.json 同步 `overviewHiddenCards`。

### 10. 自绘下拉/取色器 + 概览“显示新建的任务”（2026-11）

- **原生 `<select>` 全部移除，改用自研 `AriaSelect.vue`**：Round 8 仅做了 `appearance:none` 闭合态换肤 + `option` 配色，但 **`<option>` 弹层仍由浏览器/OS 绘制、不跟随应用主题**（反馈：批量添加弹窗下拉框仍是浏览器外观）。新组件全 Vue + `--af-*` 变量：触发器与 `.af-input` 同款（面板底/主题字/hover 边框/`asel-chevron` 旋转），弹层 `.asel-pop`（accent-soft hover、选中项 accent 字 + 勾线图标、`asel-in` 入场动画、`prefers-reduced-motion` 降级），完整键盘支持（↑↓ 循环、Home/End、Enter/Space 选中、Esc 仅关闭弹层不冒泡、Tab 关闭）、`role=listbox/option` + `aria-selected`、点击外部关闭、`placement="auto"` 视口下缘自动翻转向上（显式 `up|down` 可覆盖）。覆盖全部 4 处：添加/批量添加对话框 认证方式 + 代理、限速弹窗 单位（auto 上翻）、设置页 代理类型；`AdvancedDownloadFields` 单/批量共用。`app.css` 中遗留的 `select.af-input` 规则已删除，全仓无 `<select>`。
- **概览页“显示新建的任务”**：守护进程全局设置 `overviewShowNewTasks`（**默认 true**；`GlobalSettings.overview_show_new_tasks`，`#[serde(default = "default_true")]`，补丁 `GlobalSettingsPatch.overview_show_new_tasks: Option<bool>` 缺省不动；openapi.json 同步；旧库缺键读回 `true`，不影响存量）。“正在进行”列表在 `filter=active` 时默认追加 `pending,queued,waiting,retrying`——新建任务即可见；取消勾选则回落到纯活跃态（与“显示已暂停任务”完全同构：onMounted 读 settings、toggle 失败回滚、overview-only store 标志 `includeNewInActive`、离开页面复位、1.5s 轮询自动刷新列表）。该勾选项后端持久化（重启保留），中英文案 `overview.showNewTasks(Hint)`。
- **概览页“批量添加”改为按钮**：`.link-btn` 文字按钮删除，`.toolbar-under` 内为 `.af-btn` 描边按钮（与“添加/浏览/全部已完成”一致）。
- **强调色取色器自绘，弃用原生 `<input type="color">`**：原生弹层由浏览器/OS 按浏览器外观绘制（跟随浏览器而非应用主题，反馈项 3）。新组件 `AriaColorPicker.vue`：触发器为 conic-gradient 彩环（与原“自定义色板”外观一致，选中态 accent outline）；弹层为面板底参主题的 SV 渐变区（白→透/透→黑叠色、crosshair 光标环）+ 色相条（彩虹渐变、缩略白点钳制）+ 当前 `#rrggbb` 只读显示 +“完成”；指针 `setPointerCapture` 拖拽、饱和度/明度/色相键盘微调（方向键 ±2，Shift ±10/12，`role=slider` + aria-value*）、Esc 仅关闭弹层、点击外部关闭、**`placement` 自动向上翻转**（视口下缘无空间时——设置页取色器上一位置处于页面底部，修复后向上弹出不再裁剪）。Live-apply：拖动即写 `--af-accent`/`--af-accent-soft`（与预设色板同一 `applyAppearance()` 通路，hex 输入/预设/localStorage 持久化链路不动）。i18n：`colorPicker.title/saturationValue/hue`（中/英）。
- **设置页取色器后端链路**：`normalizeHex` 兼容 `#rgb`/`#[0-9a-f]{6}`，越界回退 `#0a84ff`；预设色板与 hex 校验分支保留；原生 input 与 `.accent-color-input` CSS 全部删除，全仓 `input[type=color]` = 0。
- **验证**：headless Chromium —— 概览默认请求 `status=downloading,connecting,merging,verifying,pending,queued,waiting,retrying`、取消勾选后回落后端 `PUT` 落库 `overviewShowNewTasks=false`（GET 回读一致）、重新勾选恢复 true；批量添加弹窗认证下拉点选“Basic Auth”、username 字段随动、Esc 仅关弹层不关弹窗、点击 backdrop 关弹窗；设置页原生 color input=0、取色器弹层像素级位于应用主题面板（暗 #2c2c2e 72% + blur）、色相/SV 点击后 `--af-accent` 实时变化并写入 localStorage（`{accent:"#df20d9"}` 回读一致）、Esc/Done/外部点击关闭、暗/明双主题截图核对；新建任务/已暂停两个勾选框状态独立持久化。cargo test settings 全绿（+openapi.json 语法校验）。

### 9. 日期/时间选择器全主题化（2026-11）

- **原生 `datetime-local` 已全部移除**（全仓仅 `AdvancedDownloadFields` 添加任务的“定时开始”一处），替换为自研 `AriaDateTimePicker.vue`：月视图日历（上月/下月导航、本月/邻月灰显、今天 accent 描边、选中 accent 实心白字——跟随 AriaFlow 强调色）、星期/月份本地化（`Intl.DateTimeFormat` + 应用内语言）、时:分输入（0-23/0-59 钳制）、“今天”/“清除”/“完成”按钮、触发器显示 `YYYY-MM-DD HH:MM`、右上角 ✕ 快速清空、键盘 ↑↓←→ 网格导航 + Enter 选中、Esc 仅关闭选择器不关闭外层弹窗、点击外部关闭。
- **v-model 契约保持 `""` 或 `"YYYY-MM-DDTHH:MM"`（本地时区）**——与 datetime-local 完全一致，`task-options.ts` 的 RFC 3339 转换零改动；添加对话框提交后 `scheduledStartAt` 经 API 回读核对一致。
- 所有颜色均走 `--af-*` 变量（面板/边框/文字/强调色/软色），浅色/深色/accent 自定义全部跟随设置主题，桌面（WebKitGTK）与 Web 一致——不再依赖浏览器/OS 绘制原生选择器。
- **验证**：headless Chromium —— 原生 datetime-local 输入=0；打开/导航到 2026-12、点选 15 号、设 09:30 → 触发器显示 `2026-12-15 09:30`、提交后 API 回读 `2026-12-16T09:30:00Z`（ArrowRight+Enter 键盘选择同效）；Esc 仅关闭选择器；点击外部关闭；明/暗两主题截图核对；`aria-expanded`/`role=dialog`/aria-label 完备。
