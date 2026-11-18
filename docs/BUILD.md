# 构建与打包

## 环境要求

| 组件 | 版本 | 说明 |
|---|---|---|
| Rust | ≥ 1.80（`rust-toolchain.toml` 固定 stable） | `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \| sh` |
| Node.js | ≥ 18（推荐 20/22） | 仅桌面端需要 |
| Tauri 系统依赖 | 见下 | 仅桌面端需要 |
| TLS | 无系统依赖——全仓统一 rustls（ring），Mozilla roots 内嵌进二进制 | 无需额外安装；SFTP 的 OpenSSL 也是静态内嵌 |

### 各平台 Tauri 前置依赖

**Linux (Debian/Ubuntu)**

```bash
sudo apt-get install -y libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev patchelf
```

> **Linux 空白窗口兜底**：WebKitGTK 在部分驱动/合成器下（常见于 NVIDIA 私有驱动与若干 Mesa/Wayland 组合）会渲染出完全空白的窗口；WebKitGTK ≥ 2.42 的 **DMA-BUF 渲染路径** 在部分 NVIDIA/Mesa 驱动上还会直接崩溃/黑屏。桌面端在 **Linux 下启动时会自动设置 `WEBKIT_DISABLE_COMPOSITING_MODE=1` 与 `WEBKIT_DISABLE_DMABUF_RENDERER=1`**（分别可用 `ARIAFLOW_WEBKIT_COMPOSITING=1` / `ARIAFLOW_WEBKIT_DMABUF=1` 关闭兜底；用户自己显式设置的 `WEBKIT_DISABLE_*` 永远优先）。

**Linux (Fedora)**

```bash
sudo dnf install -y webkit2gtk4.1-devel gtk3-devel libappindicator-gtk3-devel librsvg2-devel patchelf
```

**macOS** — 仅需 Xcode Command Line Tools（`xcode-select --install`），WKWebView 随系统提供。**Apple Silicon 与 Intel 均可构建**（universal2 需两个 target 后 `lipo` 合并）。

**Windows** — WebView2 Runtime（Win10 1803+/Win11 自带）+ MSVC Build Tools（"Desktop development with C++"）。

## 构建核心与守护进程

```bash
cargo build --release                 # 全部 workspace crate（要求 apps/desktop/dist 已构建，见下）
cargo build --release -p ariaflow-daemon -p ariaflow-cli -p ariaflow-mcp
cargo test --workspace                # 单元测试（debug 构建不需要 dist）
cargo check --workspace               # 快速类型检查（debug 构建不需要 dist）
```

> **rust-embed 顺序要求**：`ariaflow-api` 在**编译 `ariaflowd` 时**把 `apps/desktop/dist/` 烘焙进二进制（`#[folder = "../../apps/desktop/dist"]`）。
> debug 构建（`cargo build` / `cargo test` / `cargo clippy`）在 `dist/` 缺失时依然可以编译（rust-embed debug 模式运行时从磁盘惰性读取）；
> **release 构建在 `dist/` 缺失时直接编译失败**——这是有意的，防止发出"网页空壳"的二进制。推荐直接用 `scripts/build.sh`（先 npm 构建前端再 cargo build --release）。
> 构建产物不再需要 `target/release/web/` 这类外部目录。

生成的可执行文件（`target/release/`）：

- `ariaflowd` —— 守护进程（内嵌 web 网页；`/` 即桌面 web 版，见 ARCHITECTURE.md "运行时特性开关"）
- `ariaflow` —— CLI
- `ariaflow-mcp` —— MCP 服务器（stdio；`ariaflowd` 也会把它内嵌挂载到 `/mcp`，见 `docs/ARCHITECTURE.md`）
- `ariaflow-native-host` —— 浏览器 native messaging host

## 桌面端

```bash
cd apps/desktop
npm ci
npm run build            # vue-tsc 类型检查 + vite 生产构建 → dist/
npm run build:web        # 仅 vite build（不做类型检查）
npm run tauri:dev        # 开发模式窗口（需系统 WebView）
npm run tauri:build      # 产出 .app / .dmg（macOS）、.msi/.exe（Windows）、.deb/.AppImage（Linux）
```

> `apps/desktop/src-tauri` **不是** workspace 成员：它依赖系统 WebView 库，headless CI 上不可用。请用上面的命令显式构建。

### 桌面端预览（无需 Rust 工具链）

```bash
npm run build && npx vite preview --port 4173
# 直接在浏览器访问 http://127.0.0.1:4173/ （注意：浏览器模式没有 Tauri 宿主，原生对话框不可用）
```

## 桌面端打包：AppImage / deb 与“静态编译”的真相

**先说清楚“静态编译”为什么不适用于 Linux 桌面端**：Linux 上 WebView 是系统组件——Tauri 通过 wry/webkit2gtk 使用系统 webkit2gtk-4.1。
**不存在静态版的 webkit2gtk**，musl 目标也不被 Tauri 支持（桌面端恒为 glibc 动态链接）。桌面端能做到的“自包含分发”只有：

1. **AppImage**（推荐）：linuxdeploy 把 webkit/gtk/soup/javascriptcore/gdk-pixbuf 等 .so **全部打包进单个 .AppImage 文件**——
   拷到任意 x86_64 Linux 即可运行（目标机需要 FUSE，或用 `--appimage-extract-and-run`）。
   这是 Linux 桌面端唯一靠谱的“静态分发”，也是默认产物。
2. **Rust 侧本已静态**：std/rlib 默认静态；TLS=rustls（Mozilla roots 内嵌）；SFTP 的 OpenSSL 源码静态内嵌；SQLite bundled。
   唯一残余动态依赖 = WebKitGTK 家族，无法消除。
3. **deb**：`npm run tauri:build -- --bundles deb`，或 `--bundles rpm`。只想要裸二进制：`--no-bundle`。
4. **Windows**：`RUSTFLAGS=-C target-feature=+crt-static` 可把 MSVC CRT 静态链接进 exe（Rust 侧）；WebView2 仍随系统提供。

一键脚本（前端构建 → `cargo tauri build` → 产物体检 `file`/`ldd`）：

```bash
scripts/build-desktop.sh                 # 默认产出 AppImage（无 FUSE 环境自动 APPIMAGE_EXTRACT_AND_RUN=1）
scripts/build-desktop.sh --check         # 仅检查前置依赖并给出诊断（fuse 检测仅参考；打包默认一律自解压）
scripts/build-desktop.sh --bundles deb   # deb；--bundles appimage,deb；--no-bundle；--debug
```

### AppImage 构建失败：`failed to bundle project: failed to run linuxdeploy`

tauri 打包 AppImage 时会**在构建期下载** `linuxdeploy` 与 `linuxdeploy-plugin-appimage`
（github.com/linuxdeploy/linuxdeploy/releases，缓存于 `~/.cache/tauri`），两者本身也是 **AppImage**：

| 根因 | 表现 | 修复 |
|---|---|---|
| 无 FUSE 或 FUSE 被硬化内核拒绝（Arch/BlackArch 常见：`/dev/fuse` 在、`libfuse.so.2` 也在，linuxdeploy 挂载仍失败） | log 停在 `Downloading linuxdeploy-plugin-appimage…` 之后，报 `failed to run linuxdeploy` | **默认走 `scripts/build-desktop.sh`（一律自解压 `APPIMAGE_EXTRACT_AND_RUN=1`，不依赖 FUSE/libfuse，也不看检测结果）**；或手动 `APPIMAGE_EXTRACT_AND_RUN=1 npm run tauri:build` |
| 缺 `patchelf`（linuxdeploy 用它改被打包库的 RPATH） | 同上（发生在 AppImage 打包步骤） | `sudo pacman -S --needed patchelf`（Debian: patchelf；Fedora: patchelf） |
| 下载损坏/被截断（网络对 github 不稳） | 同上；自解压也失败时重点怀疑此项——缓存里是 HTML/半截文件而非 ELF | `rm -rf ~/.cache/tauri` 后重试；先 `file ~/.cache/tauri/linuxdeploy-x86_64.AppImage` 验证是 ELF 而非 HTML；最后直连 GitHub 手动下载放入该路径并 `chmod +x` |
| 打包机不想折腾 FUSE | —— | `npm run tauri:build -- --bundles deb` 或 `--no-bundle`（仅裸二进制） |

| **ld.so.cache 被 VMware 污染**（gtk 插件部署 `/usr/lib/vmware/lib/libgobject-…`，报 `ERROR: Could not find dependency: libffi.so.6` → `Failed to run plugin: gtk`） | VMware Workstation 把私有库目录注入 `/etc/ld.so.conf.d/`，全局缓存优先于系统库，linuxdeploy 找到的是极旧的私有 gobject（依赖绝版的 libffi.so.6） | **根治**：`grep -ri vmware /etc/ld.so.conf /etc/ld.so.conf.d/` 找到条目 → `sudo mv <该conf> /root/vmware.conf.bak` → `sudo ldconfig` → `ldconfig -p | grep libgobject-2.0` 应指向 `/usr/lib`。VMware 自身走自带库搜索路径，移出全局缓存**不影响其运行**。`scripts/build-desktop.sh` 的 `--check` 会检测该污染并打印上述修复步骤，并在打包时自动预置 `LD_LIBRARY_PATH`（系统库目录优先，best-effort；设 `ARIAFLOW_NO_LD_LIBRARY_PATH=1` 可关闭） |

按平台安装全部前置：见上方“各平台 Tauri 前置依赖”；BlackArch/Arch 完整命令：
`sudo pacman -S --needed webkit2gtk-4.1 gtk3 libayatana-appindicator librsvg patchelf fuse2 base-devel`

## musl 静态构建（完全静态、无系统依赖）

`x86_64-unknown-linux-musl`（以及其他 musl/交叉目标）现已完整支持。AriaFlow 的静态产物**不依赖任何系统库**：

| 组件 | 方案 |
|---|---|
| HTTP/HTTPS (reqwest) | rustls + `webpki-roots`（Mozilla roots 内嵌，无需系统 CA 证书） |
| FTPS（suppaftp `rustls-ring`） | rustls + 内嵌 Mozilla roots；`danger_accept_invalid_certs` 时走自定义“接受任意证书”校验器（与 HTTP 行为一致，用户显式 opt-in） |
| BitTorrent (librqbit) | rustls（`rust-tls` feature） |
| SQLite (sqlx) | bundled 源码构建 |
| SFTP (ssh2 → libssh2-sys) | Unix 下 `vendored-openssl` **源码构建**——静态内嵌 OpenSSL，不需要系统 OpenSSL |
| zlib (libz-sys) | Unix 下 `static` 源码构建——不需要系统 zlib |

一键脚本（先构建前端并内嵌 web UI，再产出 4 个静态二进制）：

```bash
scripts/build-musl.sh                # release 构建（输出 target/x86_64-unknown-linux-musl/release/）
scripts/build-musl.sh --skip-web     # 跳过前端构建（dist 已存在时）
scripts/build-musl.sh --debug        # debug 构建
scripts/build-musl.sh --target aarch64-unknown-linux-musl   # 交叉静态构建（参数形式，与 TARGET_TRIPLE 等价）
TARGET_TRIPLE=aarch64-unknown-linux-musl scripts/build-musl.sh   # 其他 musl 目标（环境变量形式）
```

前置要求（Debian/Ubuntu）：`sudo apt install musl-tools`（提供 `musl-gcc`）；Fedora：`sudo dnf install musl-gcc`；Arch：`sudo pacman -S musl`；在 Alpine 上（原生 musl）系统 gcc 即可。脚本自动执行 `rustup target add` 并设置 `CARGO_TARGET_*_LINKER` / `CC_*`。

产物（ldd 应显示 `not a dynamic executable`）：`ariaflowd`、`ariaflow`、`ariaflow-mcp`、`ariaflow-native-host`。桌面端（Tauri / WebKitGTK）**不**走 musl——WebView 是系统组件，静态 musl 不支持。

> 许可证：静态内嵌的 OpenSSL 为 Apache-2.0（实际构建自其上游源码，Apache-2.0 单一条款）与其它同样宽松的协议组合（ring/aws-lc/sqlite/zlib 均为宽松许可）。

**已验证**（2026-11）：在 glibc 机器上 `cargo check --workspace --target x86_64-unknown-linux-musl` 全量通过；四个二进制完整链接并静态运行（`ariaflowd --version`）；静态 musl 产物实际发起 `https://example.com/` 的 probe 与下载（rustls + webpki-roots 链路）成功；`ldd` = not a dynamic executable。

> **2026-11-16 重要修复（BlackArch #GP 崩溃）**：musl target 默认产出 `-static-pie`（ET_DYN + 自重定位 GOT）。这类产物在本 Debian 构建机上一切正常，但在用户的 **Arch/BlackArch** 上四个二进制（ariaflow/ariaflowd/…）**启动即 `general protection fault`**（内核陷阱日志显示固定指令偏移、`error:0`、均发生于 `main` 之前；强制 `--pack-relative-relocs` 产出 `.relr.dyn` 后依旧故障）。
> 脚本现强制 **`-C relocation-model=static`**（产出经典 ET_EXEC 静态二进制，无启动期重定位机器，彻底移除该故障类），并新增**构建后冒烟测试**（host 能执行时自动跑 `ariaflow --version`，失败即让构建报错退出并给出诊断而非留下一个"看起来正常、跑起来必崩"的二进制）。
> 已验证：ET_EXEC 产物 `file` 显示 `statically linked`、`ldd` 为 not a dynamic executable、`ariaflowd` 启动、`/api/v1/status`、对 `https://example.com/` 的 HTTPS probe 全部正常。**若你在用 2026-11-16 之前构建的 musl 产物，请用新脚本重新构建。**
> 环境注意：`cargo` 的 target 专用 `CARGO_TARGET_*_RUSTFLAGS` 环境变量优先级高于 `~/.cargo/config.toml`，新脚本以追加（不覆盖）方式注入该 flag；残留旧 RUSTFLAGS（如 `-C target-cpu=native` 在不同 CPU 间拷贝二进制）仍可能引入各自问题。

## 运行时 so 依赖盘点（glibc 动态构建，2026-11-16 实测）

`ldd target/release/ariaflowd`（以及 `ariaflow` / `ariaflow-mcp` / `ariaflow-native-host`）：

```
linux-vdso.so.1
libgcc_s.so.1   => /lib/x86_64-linux-gnu/libgcc_s.so.1
libm.so.6       => /lib/x86_64-linux-gnu/libm.so.6
libc.so.6       => /lib/x86_64-linux-gnu/libc.so.6
/lib64/ld-linux-x86-64.so.2
```

**没有任何 OpenSSL / zlib / sqlite3 / brotli 动态依赖**——TLS 走 rustls（纯 Rust）、
SQLite 走 bundled 源码、zlib/OpenSSL 仅 SFTP 使用且全部源码静态内嵌。glibc 构建只剩
`libc / libm / libgcc_s / ld-linux` 四件套，这是 glibc 动态链接的下限，无法再少。

各平台残余依赖与“能压缩到多少”：

| 平台 | 残余 so 依赖 | 进一步压缩途径 |
|---|---|---|
| CLI/daemon/MCP（Linux glibc） | libc、libm、libgcc_s、ld-linux | **无更少空间**；如需 0 依赖直接用 musl 静态产物（`build-musl.sh`） |
| CLI/daemon/MCP（Linux musl） | 无（ET_EXEC 完全静态） | 已到下限 |
| CLI/daemon/MCP（Windows） | `api-ms-win-crt-*` / `vcruntime*.dll`（如未静态 CRT） | `scripts/build.sh --target x86_64-pc-windows-msvc` 自动附加 `-C target-feature=+crt-static`（可 `--no-static-crt` 关闭）；产物不再依赖 VC++ Redistributable |
| 桌面端二进制（Linux） | webkit2gtk-4.1 / javascriptcoregtk-4.1 / soup3 / gtk-3 / glib / cairo / pango / atk / gdk-pixbuf / appindicator + 上表 glibc 四件套 | WebKitGTK 无静态版、musl 不被 Tauri 支持——这条路无法归零；**正解是 AppImage**（linuxdeploy 把上述系统库全部打进去，目标机零安装）。deb/rpm 则保留系统依赖声明 |
| 桌面端二进制（Windows） | WebView2（Evergreen runtime，系统组件，Win10/11 自带或自动更新）+ UCRT/VCRuntime | `+crt-static` 去掉 VCRuntime；WebView2 无法静态链接 |
| 桌面端二进制（macOS） | WKWebView（系统 framework） | 无同类问题 |

> 结论：**Linux CLI 侧想“无 so 依赖”→ musl；Linux 桌面侧想“目标机零安装”→ AppImage（bundle 已内置）；
> Windows CLI 侧想“不装 VC++ Redist”→ `build.sh` 默认的 crt-static。**

## 交叉构建（2026-11-16 新增）

三个构建脚本统一接受 `--target <triple>`：

```bash
# CLI / daemon / MCP / native-host（不打安装包，裸二进制）
scripts/build.sh                              # host 目标（行为不变）
scripts/build.sh --target aarch64-unknown-linux-gnu
scripts/build.sh --target armv7-unknown-linux-gnueabihf
scripts/build.sh --target x86_64-pc-windows-gnu      # mingw-w64
scripts/build.sh --target x86_64-pc-windows-msvc     # 默认 +crt-static；Linux 上不可用，请在 Windows 构建
scripts/build.sh --target aarch64-unknown-linux-musl # 报错并引导改用 build-musl.sh

# musl 完全静态（CLI 侧零 so 依赖）
scripts/build-musl.sh --target aarch64-unknown-linux-musl

# 桌面端（Tauri；打包器平台绑定，见下）
scripts/build-desktop.sh --target x86_64-pc-windows-msvc   # 需 Windows 主机
scripts/build-desktop.sh --target aarch64-unknown-linux-gnu  # 需 arm64 webkit2gtk sysroot
```

行为细节：

- 脚本自动 `rustup target add`；交叉目标自动配置 `CARGO_TARGET_*_LINKER` / `CC_*`（常见目标映射：
  `aarch64-unknown-linux-gnu→aarch64-linux-gnu-gcc`、`armv7-unknown-linux-gnueabihf→arm-linux-gnueabihf-gcc`、
  `i686-unknown-linux-gnu→i686-linux-gnu-gcc`、`riscv64gc-unknown-linux-gnu→riscv64-linux-gnu-gcc`、
  `x86_64-pc-windows-gnu→x86_64-w64-mingw32-gcc`）。C 工具链缺失时报错并给出安装提示；
  未知目标打印警告并继续（自行设 `CARGO_TARGET_*_LINKER`）。
- `*-pc-windows-msvc`：自动附加 `-C target-feature=+crt-static`（可用 `--no-static-crt` 关闭；`--static-crt` 显式开启）。
- 产物目录随目标变化：`target/<triple>/release/`；交叉产物跳过 `ldd`，Windows 目标产物列 `*.exe`，
  并用 `file` 展示产物类型。
- `*-linux-musl` 一律引导至 `build-musl.sh`（ET_EXEC + 静态，避免 glibc 动态链接误区）；
  `*-pc-windows-msvc` 与 `*-apple-darwin*` 在 Linux 上直接报错（需要对应 OS 的 SDK）。
- 桌面端 `--target` 只影响 cargo 的 `--target` 与产物路径；Linux 前置检查（webkit2gtk/FUSE/patchelf）
  在非 Linux 目标下自动跳过，appimage bundling 在非 Linux 目标被忽略并降级为 `--no-bundle`。
  跨大 OS 打包（Linux→Windows、任意→macOS）本质需要目标 OS 主机，建议用 CI 矩阵（见下节“CI”）。

### 平台支持矩阵（2026-11-16 更新）

| 目标 | 状态 | 说明 |
|---|---|---|
| `x86_64-unknown-linux-musl` | ✅ 构建 + 运行验证 | `scripts/build-musl.sh [--target <triple>]`；静态产物实测 probe/下载 HTTPS 成功 |
| `aarch64-unknown-linux-musl` | ✅ 构建 + 运行验证 | 无 aarch64 musl-gcc 时用 zig cc 封装（`python3 -m ziglang cc -target aarch64-linux-musl`）+ `CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_LINKER/CC_*`；rustc 内建 late-link 参数 `-Wl,--fix-cortex-a53-843419` 不被 zig 接受，封装需将其剔除（zig 面向通用 CPU，不涉及 Cortex-A53 erratum）；`-C link-self-contained=n` 避免与 zig 的 musl CRT 冲突。四个 aarch64 静态二进制均实测通过 `file`（statically linked）并在 **qemu-aarch64**（Debian `qemu-user`）下完成：`--version`、`ariaflowd` 启动、`/api/v1/status` 应答、`ariaflow probe https://example.com/`（rustls 真实 TLS 链路） |
| `armv7-unknown-linux-musleabihf` / `i686-unknown-linux-musl` | ⚠️ 脚本已覆盖候选链接器名 | 未实际构建验证 |
| `x86_64-unknown-freebsd` | ❌ 当前不可构建 | 传递依赖 `librqbit-dualstack-sockets` 调用仅 Linux 存在的 `Socket::bind_device`（SO_BINDTODEVICE），上游未提供 FreeBSD 实现；除非上游修复或 ariaflow 把 BitTorrent 做成 Linux-only feature，FreeBSD 上无法编译 ariaflowd。桌面端 Tauri 在 FreeBSD 上也无官方支持 |
| 桌面端（Tauri 2，任何发行版） | ❌ 不走 musl | WebView 是**系统组件**（Linux webkit2gtk / macOS WKWebView / Windows WebView2），静态 musl 不存在这些实现；桌面端保持 glibc 动态链接 |
| `aarch64-unknown-linux-gnu`（arm64 桌面/服务端） | ⚠️ 可行、未在本仓库验证 | 桌面：arm64 原生 runner + `sudo apt install libwebkit2gtk-4.1-dev` 后 `npm run tauri:build`；服务端/无头：`cargo build --target aarch64-unknown-linux-gnu --release`（C 依赖用 `gcc-aarch64-linux-gnu`）。注意**不是 musl**，仍动态链接 glibc |
| Windows / macOS | ✅ CI 覆盖（check/test） | musl 概念不适用；发行构建走 release 工作流 |

Rust 官方提供 `x86_64-unknown-freebsd` 的 std（本次已实测 `rustup target add x86_64-unknown-freebsd` + zig 封装交叉编译，失败于上述 librqbit 依赖，非工具链问题）。若上游 librqbit-dualstack-sockets 为 FreeBSD 提供实现，本仓库无需改动即可重试。

## 依赖与许可证

直接依赖（关键项）：

| 依赖 | 用途 | 许可证 |
|---|---|---|
| tokio | 异步运行时 | MIT |
| reqwest | HTTP 客户端（rustls、http2、socks、cookies） | MIT OR Apache-2.0 |
| axum | HTTP 服务框架 | MIT |
| tower-http | CORS / 访问日志中间件 | MIT |
| rust-embed | 编译期内嵌 web 前端 | MIT OR Apache-2.0 |
| sqlx (sqlite) | 数据库访问（WAL） | MIT OR Apache-2.0 |
| serde / serde_json | 序列化 | MIT OR Apache-2.0 |
| clap | CLI 参数解析 | MIT OR Apache-2.0 |
| ratatui + crossterm | CLI 内嵌 TUI（`--tui`） | MIT |
| blake3 | 校验和 | CC0-1.0 OR Apache-2.0 |
| sha1 / sha2 / md-5 | 校验和 | MIT OR Apache-2.0 |
| suppaftp | FTP / 显式 FTPS 传输（阻塞客户端，线程池执行） | MIT OR Apache-2.0 |
| ssh2 (libssh2-sys → libssh2) | SFTP 传输 | MIT OR Apache-2.0（libssh2 本身 BSD） |
| librqbit | BitTorrent（magnet / http(s) `.torrent` 链接）下载，纯 Rust BTv1 客户端 | Apache-2.0 |
| uuid / chrono | ID 与时间 | MIT OR Apache-2.0 |
| thiserror | 错误类型 | MIT OR Apache-2.0 |
| tracing / tracing-subscriber | 结构化日志 | MIT |
| Vue 3 / Vite / Pinia / Vue Router | 桌面前端 | MIT |
| Tauri 2 | 桌面外壳 | MIT OR Apache-2.0 |

全 workspace 共 **488 个第三方包**，许可证分布：`MIT OR Apache-2.0` 250、`MIT` 93、`Apache-2.0 OR MIT` 29、`MIT/Apache-2.0` 23、`Unicode-3.0` 18、`Apache-2.0` 17（含其余）；**未选择任何 copyleft 许可证**。两点特别说明（均为传递依赖、原样使用、未修改）：

- `option-ext 0.2.0`（**MPL-2.0**，经 `dirs-sys` ← `dirs` 引入，文件级弱 copyleft；未修改其代码，仅随二进制分发）；
- `r-efi 5.3.0 / 6.0.0`（`MIT OR Apache-2.0 OR LGPL-2.1-or-later`，经 `getrandom` 引入；LGPL 选项**未被选择**，按 MIT / Apache-2.0 使用）。

`aws-lc-sys`（rustls 0.23 的默认 crypto provider，经 reqwest 引入；我们显式启用的是 `ring`，但 rustls 默认 features 仍会带上它）为 ISC/MIT/Apache-2.0/BSD 组合许可，同样宽松；两者均为纯源码 + cc/perl 构建，musl 下已实测通过。锁文件 `Cargo.lock` 已入库，保证可复现构建。

前端依赖同样全部为宽松许可证（MIT）。

## CI

`.github/workflows/ci.yml` 在 **ubuntu-latest / windows-latest / macos-latest** 上执行 `cargo fmt --check`、`clippy`、`cargo check`、`cargo test --workspace`；另有 release 工作流草稿（打 `v*` 标签时生成草稿 Release）。musl 静态构建由 `scripts/build-musl.sh` 手动触发（需 musl C 工具链，未纳入 CI 矩阵）。

## 安全待办（诚实的已知限制）

1. 代理密码以明文落 SQLite 与（桌面端的）内存；计划：接入系统钥匙串加密存储；
2. 守护进程未实现 TLS（只绑回环）；远程暴露时务必置于带 TLS 的反向代理之后；
3. 桌面端未做代码签名（macOS notarization / Windows Authenticode），分发前需要证书；
4. `ariaflowd` 目前不自注册为系统服务（launchd/systemd/Windows Service）；计划在 sidecar 里程碑补齐；
5. 下载的目标目录越界防护已在服务端落地，但 Linux 上未做 link 攻击防护（`O_NOFOLLOW`），Windows 上未做 ADS 检查。
