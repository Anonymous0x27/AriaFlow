# 开发指南

## 环境

```bash
# Rust（rust-toolchain.toml 固定 stable，自动安装）
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"

# 桌面端（可选）
# Node ≥ 18；见 docs/BUILD.md 安装 Tauri 系统依赖
```

## 常用命令

```bash
cargo check --workspace
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check

# 注意：release 构建 ariaflowd 时 rust-embed 会要求 apps/desktop/dist 已存在（scripts/build.sh 已处理顺序）；
# check/test 是 debug 构建，不需要 dist

# 守护进程 + CLI 实测
cargo run -p ariaflow-daemon &          # 127.0.0.1:8787
cargo run -p ariaflow-cli -- status
cargo run -p ariaflow-cli -- --tui      # 全屏终端界面
cargo run -p ariaflow-cli -- add https://example.com/file.iso
cargo run -p ariaflow-cli -- list
```

仓库根目录还提供快捷脚本：

```bash
scripts/check.sh        # fmt + clippy + tests（本地与 CI 同款）
scripts/build.sh        # release 构建 CLI / daemon / MCP / native-host
scripts/desktop.sh      # 桌面前端 build + 类型检查
scripts/protocol-e2e.sh # 本地端到端协议验证：起 HTTP/FTP/SFTP mock 服务器 + ariaflowd，
                        # 断言 12 项（下载/校验/暂停续传/恢复/probe）——见 docs/BUILD.md
```

## 代码组织与规范

- **不跨层依赖**：`ariaflow-core` 不依赖任何 IO；`ariaflow-engine` 不依赖存储；`ariaflow-storage` 不含业务判断；`apps/desktop`（桌面端与 web 版）只通过管道或 web API 与守护进程对话，不直接读写存储；
- **错误处理**：库 crate 用 `thiserror` 枚举错误；二进制 crate 用 `anyhow` 风格的 `Result<(), Box<dyn Error>>`；API 层把错误映射为 `{"error":{"code","message"}}`；
- **日志**：`tracing`；`ariaflowd` 同时输出到 stderr 与 `log_dir()`（见 `ariaflow-core::paths`）；默认无 ANSI（JSON 日志可加 `--log-format json` 扩展）；
- **SQL 只允许出现在** `ariaflow-storage`；
- **新增表/列**：在 `schema.rs` 追加一个更大的迁移版本，不要修改已发布的迁移；
- **API 变更**：同步更新 `assets/openapi.json`、`ariaflow-api::client`（Rust 客户端）、桌面端 `src/lib/api.ts`、以及本目录的 `API.md`；
- **事件**：新增事件类型需同步三个位置：`ariaflow-core::DownloadEvent`、SSE 序列化、文档；
- **依赖**：新增依赖需在 PR 描述中说明用途与许可证；禁止 copyleft；锁文件随提交入库。

## 测试

- 单元测试与源码同文件（`#[cfg(test)]`）；
- 引擎/存储测试使用 `tempfile` 临时目录，不触碰真实用户目录；
- CI 在三大 OS 上运行 `cargo test --workspace`；
- 端到端协议验证：`scripts/protocol-e2e.sh`（需要 python3 + `pyftpdlib` + `paramiko`）——
  在本机起 HTTP/限速 HTTP/FTP(匿名)/SFTP(密码) mock，断言 HTTP/FTP/SFTP/Metalink 下载字节一致、
  校验和通过/不通过的行为、暂停→`.ariaflow-part`→续传完成；
- BitTorrent 传输路径（librqbit）仅有磁链解析单元测试，沙箱离线环境无法完成真实 swarm 握手；
  上线前请在有网络环境用一个公开 magnet 做人工冒烟。
- TUI/GUI 不设自动化 UI 测试（人工验收 + 截图；快速添加流程已用 Playwright 手工验证），但管道/HTTP 契约以 `docs/API.md` 为准。

## 发布流程（草稿）

1. 更新所有 `Cargo.toml` 的 `version`（workspace 继承 `workspace.package.version`）与 `apps/desktop/package.json`；
2. `scripts/check.sh` 全绿；
3. 打标签：`git tag v0.1.0 && git push origin v0.1.0`；
4. CI 触发 release 草稿：三大平台产物 + 校验和；发布前人工审核。
