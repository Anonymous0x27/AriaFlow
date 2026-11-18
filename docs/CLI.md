# ariaflow CLI 参考

`ariaflow` 控制 `ariaflowd`。默认通过**本地管道**（Unix socket / Windows 命名管道）访问；可用 `--transport` 切换到 web API。所有命令支持 `--json`（机器可读输出）与前缀任务 id（≥4 个字符，唯一即可）。

## 全局参数

| 参数 | 环境变量 | 默认值 | 说明 |
|---|---|---|---|
| `--json` | — | 关闭 | 全部输出为 JSON |
| `--transport` | `ARIAFLOW_TRANSPORT` | `auto` | `auto`：管道优先、失败回退 web API；`ipc`：仅管道；`webapi`：仅 web API |
| `--ipc-path` | `ARIAFLOW_IPC_PATH` | 平台默认 | 管道端点（Unix：`$XDG_RUNTIME_DIR/ariaflowd.sock` 或数据目录下 `ariaflowd.sock`；Windows：`\\.\pipe\ariaflowd`） |
| `--api-url` | `ARIAFLOW_API_URL` | `http://127.0.0.1:8787` | 守护进程 web API 地址（webapi 模式 / auto 回退时使用） |
| `--token` | `ARIAFLOW_API_TOKEN` | — | Bearer token |

退出码：`0` 成功；`1` API/业务错误；`2` 参数错误（如 id 前缀有歧义）；`3` 无法连接守护进程。

## 交互界面

```bash
ariaflow --tui                 # 全屏终端界面（ratatui），等效于旧 `ariaflow-tui`
```

按键：`↑↓/jk` 移动 · `Tab/f` 切换过滤 · `a` 添加下载 · `p/r` 暂停/继续 · `d` 删除（需确认）· `q` 退出。

## 任务操作

```bash
ariaflow add <url> [--dir DIR] [--connections 1-64] [--priority N] [--header "K: V"]...
                   [--referer R] [--bearer TOKEN] [--basic USER:PASS] [--filename NAME] [--paused]
                   [--limit-rate 10M] [--retry N] [--schedule "2026-11-13T02:00+08:00"]
ariaflow batch-add <file|-> [--dir DIR] [--connections 1-64] [--priority N] [--header "K: V"]...
                   [--referer R] [--bearer TOKEN] [--basic USER:PASS] [--paused]
                   [--limit-rate 10M] [--retry N] [--schedule ...]
                   # 每行一个 URL，'#' 开头为注释，'-' 表示 stdin；
                   # 所有任务共用同一组高级选项（headers/auth/限速等）
ariaflow list [--status downloading,queued] [--category archives] [--search KEY] [--page 1] [--page-size 50]
ariaflow info <id>
ariaflow pause <id> | ariaflow pause --all
ariaflow resume <id> | ariaflow resume --all
ariaflow cancel <id> | ariaflow cancel --all
ariaflow remove <id> [--delete-file] | ariaflow remove --completed
ariaflow retry <id>
ariaflow probe <url> [--header "K: V"]... [--referer R]
```

说明：

- `pause --all` / `resume --all` / `cancel --all` 返回受影响的任务数；
- `remove --completed` 清空全部已完成任务；`remove --delete-file` 同时删除磁盘文件（慎用）；
- `probe` 只取元数据（重定向后的最终 URL、大小、Range 支持、ETag……），不会下载任何内容；
- `--limit-rate` 单任务限速（bytes/sec，支持 `10M` 等写法），由下载引擎执行；`ariaflow config set global-speed-limit` / `per-task-speed-limit` 分别控制全局与默认单任务限速（aria2 风格令牌桶）。
- 添加同名任务时，目标已存在或已有进行中的同名任务时，文件会自动保存为 `name (1).ext`（不覆盖任何已有文件）。

## 配置与守护进程

```bash
ariaflow config show                                  # 打印 daemon 当前设置
ariaflow config set <key> <value>                     # 如 download-dir / max-concurrent-tasks / global-speed-limit
# 运行时特性开关（即时生效、不重启、写入 settings 持久化；关闭 HTTP API 后仍可走管道操作）：
ariaflow config set api-enabled false                 # 关闭 HTTP API（/api/v1/* 返回 503 feature_disabled）
ariaflow config set mcp-enabled false                 # 关闭 MCP 端点（/mcp 返回 503 feature_disabled）
ariaflow config set web-ui-enabled false              # 关闭内嵌网页（/ 返回 503 HTML 说明页）
ariaflow config set api-enabled true                  # 重新打开
# 独立 web 监听端口（网页+API 同源托管，给局域网浏览器用；启动期配置，改后重启生效）：
ariaflow config set web-ui-bind 0.0.0.0:9090          # 绑定独立 web 端口
ariaflow config set web-ui-bind ""                    # 清空 = 关闭独立 web 端口
# （等价启动参数：ariaflowd --web-bind 0.0.0.0:9090，优先级高于设置）
# ariaflowd 主监听地址（启动期配置，改后重启生效；--api-bind 优先；非回环建议配合 --token）：
ariaflow config set api-bind 0.0.0.0:8787             # 持久化监听地址（PUT 校验，非法值 400）
ariaflow config set api-bind 127.0.0.1:8787           # 恢复仅本机
ariaflow config set on-download-complete-command 'notify-send "AriaFlow" "$ARIAFLOW_FILENAME done"'
ariaflow config set on-download-error-command 'notify-send "AriaFlow" "failed: $ARIAFLOW_ERROR"'
ariaflow daemon start | stop | restart | status
ariaflow daemon start --bin /opt/ariaflow/bin/ariaflowd   # 指定 ariaflowd 路径（ariaflowd 不在常用路径时）
ariaflow daemon restart --bin ./target/release/ariaflowd  # --bin 先校验后停服务；路径非法时旧进程不受影响
```

配置键：`download-dir` · `max-concurrent` · `max-connections` · `max-total-connections` · `global-speed-limit` · `per-task-speed-limit` · `retry-count` · `min-segment-size` · `connect-timeout` · `read-timeout` · `api-enabled` · `api-bind` · `mcp-enabled` · `user-agent` · `auto-quit-daemon` · `download-window-start`（`HH:MM` 或分钟数）· `download-window-end` · `on-download-complete-command`（空串清除）· `on-download-error-command`（空串清除）。

## 输出示例

```console
$ ariaflow list
ID        STATUS        PROGRESS  SPEED      FILENAME
9353c9a9  downloading   91.2%     8.0 MiB/s  ubuntu-24.04-desktop-amd64.iso
9ea4752f  queued        0.0%      -          ariaflow-0.1.0-x86_64.AppImage
```

```console
$ ariaflow --json list
[ { "id": "9353c9a9-…", "status": "downloading", … } ]
```

```console
$ ariaflow probe https://example.com/file.iso --json
{ "finalUrl": "…", "totalSize": 3145851, "acceptRanges": true, "etag": "\"…\"", … }
```

## 与 AI 代理配合

所有输出稳定、无装饰 ANSI（`--json` 时），错误信息走 stderr 且带退出码，适合在脚本与 LLM 工具调用中直接使用。配合 MCP 服务器（`ariaflow-mcp`）效果更佳，见 [ARCHITECTURE.md](ARCHITECTURE.md#统一接口与事件)。
