# 数据库（SQLite，通过 sqlx）

文件位置（默认）：`dirs::data_dir()/ariaflow/ariaflow.db` —— 即 `~/.local/share/ariaflow/ariaflow.db`（Linux）、`~/Library/Application Support/ariaflow/ariaflow.db`（macOS）、`%APPDATA%\ariaflow\ariaflow.db`（Windows）。
可用环境变量 `ARIAFLOW_DB` 或 `ariaflowd --db <path>` 覆盖。日志默认写 `state_dir/ariaflow/log`（可用 `ARIAFLOW_LOG` 设置日志级别）。

连接参数：`PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;` —— 允许读写并发、崩溃安全，且外键级联删除生效。

## 迁移机制

迁移以内嵌 SQL 数组形式存于 `ariaflow-storage::schema::MIGRATIONS`（`(version, name, sql)`），启动时按版本严格递增顺序、在同一事务内执行，并记录于 `schema_migrations` 表中（列：`version`、`name`、`applied_at`），已应用的迁移不会重复运行。新增表结构 = 追加一个更大的 version。

## 表结构

### downloads —— 任务主表

| 列 | 类型 | 说明 |
|---|---|---|
| `id` | TEXT PK | UUIDv4 |
| `url` / `final_url` | TEXT | 用户输入 URL / 重定向后最终 URL |
| `filename` / `save_path` | TEXT | 清洗后的文件名 / 目标绝对路径 |
| `total_size` / `downloaded_size` | INTEGER | 总字节 / 已下载字节（`NULL` = 未知） |
| `status` | TEXT | `queued` / `downloading` / `paused` / `completed` / `failed` / `cancelled` |
| `progress` | REAL | 0–100（展示用） |
| `speed` / `average_speed` | INTEGER | 瞬时 / 平均字节每秒 |
| `priority` | INTEGER | 越大越优先 |
| `retry_count` | INTEGER | 已重试次数 |
| `max_connections` / `current_connections` | INTEGER | 上限 / 当前连接数 |
| `category` | TEXT | 分类键，见 `categories` |
| `etag` / `last_modified` | TEXT | 断点续传协商 |
| `checksum_json` | TEXT | 校验和配置（类型 + 期望值） |
| `proxy_json` | TEXT | 代理覆盖配置（含密码？—— 见"敏感信息"） |
| `error_message` | TEXT | 最近一次失败原因 |
| `limit_rate_bytes_per_sec` | INTEGER NULL | 单任务限速（NULL = 继承全局）；migration v2 新增 |
| `max_retries` | INTEGER NULL | 单任务自动重试覆盖（NULL = 继承全局）；migration v2 新增 |
| `create_subdirs` | INTEGER | 是否按分类创建子目录 |
| `created_at` / `started_at` / `completed_at` | TEXT | ISO-8601 |

索引：`status`、`category`、`created_at DESC`、`priority DESC`。

### segments —— 分段下载断点续传事实来源

`(download_id, segment_index)` 唯一；每段记录 `start_offset/end_offset/downloaded_size/status/retry_count`。引擎恢复时读取该表决定哪些段需要重新发起 Range 请求；`downloads` 行删除时 `ON DELETE CASCADE`。

### categories —— 分类（内置 7 个：documents/images/videos/audio/archives/installers/other）

内置分类含中文标签与扩展名映射，可被任务自动归类。

### settings —— 守护进程全局配置（key/value）

下载目录、并发、连接数上限、全局/单任务限速、重试次数、UA 等；CLI 的 `ariaflow config show/set` 与 API `PUT /api/v1/settings` 均读写此表。

### download_headers —— 每任务自定义请求头

`(download_id, name)` 主键，级联删除。

### download_history —— 完成/失败历史

保留 URL、文件名、保存路径、结果（`completed`/`failed`）、速度、耗时，用于"下载历史"页面；`downloads` 表可被清理而不丢历史。

### proxies —— 代理配置

HTTP / SOCKS5；`is_default` 标记默认代理。**密码列目前以明文存储于本地库——属已知限制，见下方"已知限制"。**

### api_tokens —— API token（仅存 `token_hash`）

Bearer token 以哈希形式保存，可吊销（`revoked_at`），记录 `last_used_at`。守护进程不会以明文持久化 token。

### events —— 任务生命周期审计

`download_id + event_type + payload(JSON)`；所有 `DownloadEvent` 也写入此表，供 UI 时间线与调试。

### download_history / events 与 downloads 的关系

- `downloads` 是"进行中的事实"；完成/失败后，行可被 `remove` 清理；
- `download_history` 是不可变归档；`events` 是审计日志。

## 敏感信息策略

- API token：只存哈希；
- 代理密码：**当前版本明文落库**（本地回环部署风险可控），计划项为引入系统钥匙串（keychain/Secret Service/Credential Manager）加密存储，位于 BUILD.md 的"安全待办"。

## 手动检查

```bash
sqlite3 $ARIAFLOW_DB ".tables"
sqlite3 $ARIAFLOW_DB "SELECT status, count(*) FROM downloads GROUP BY status"
sqlite3 $ARIAFLOW_DB "PRAGMA journal_mode"   # 应输出 wal
```

## Schema 版本

| 版本 | 名称 | 内容 |
|---|---|---|
| 1 | `init` | 上述全部表与索引 |
| 2 | `task-limits` | `downloads` 增加 `limit_rate_bytes_per_sec`、`max_retries` 两列（per-task 限速/重试覆盖，均可 NULL） |

`status` 列取值（完整）：`pending` / `queued` / `connecting` / `downloading` / `paused` / `waiting` / `retrying` / `merging` / `verifying` / `completed` / `failed` / `cancelled`。
