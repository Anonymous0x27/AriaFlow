#!/usr/bin/env bash
# 桌面端（Tauri 2）发行构建脚本：前端构建 + cargo tauri build + 产物体检
#
# ■ 先把"静态编译"说清楚（重要）：
#   Linux 桌面端**无法**做出 ariaflowd 那种完全静态（musl / ET_EXEC）的单二进制：
#   WebView 是系统组件——WebKitGTK（webkit2gtk-4.1）是动态库，且没有静态版的
#   webkit2gtk，musl 目标也不被 Tauri 支持（见 docs/BUILD.md 平台支持矩阵）。
#   桌面端能做到的"静态分发"只有一种：把系统动态库（webkit/gtk/soup/…）
#   打包进 **AppImage**——即 Tauri 的 linuxdeploy 步骤。单个 .AppImage 文件拷到
#   任意 x86_64 Linux 上都能跑（目标机需要 FUSE，或用 --appimage-extract-and-run）。
#   Rust 侧本来就是静态的（std/rlib 默认静态；TLS 走 rustls；SFTP 走 vendored-openssl
#   源码构建），剩余动态依赖只有 WebKitGTK 一家。
# ■ 最常见的构建失败：
#   `failed to bundle project: failed to run linuxdeploy`
#   linuxdeploy 与 linuxdeploy-plugin-appimage 本身都是 AppImage，运行需要 FUSE；
#   Arch/BlackArch 默认**没有**装 fuse2，或 /dev/fuse 在容器/hardened kernel 中不可用；
#   另外 linuxdeploy 修 RPATH 需要 patchelf。修复：
#     Arch/BlackArch: sudo pacman -S --needed fuse2 patchelf
#     Debian/Ubuntu : sudo apt-get install -y patchelf libfuse2t64     # 仅“强制直接挂载”时需要（Debian 13+；旧版 fuse libfuse2）；Fedora: fuse fuse-libs patchelf
#     （注：fuse3 不提供 AppImage 需要的 libfuse.so.2；默认自解压根本用不到 FUSE）
#   默认行为：打包 AppImage 时自动 APPIMAGE_EXTRACT_AND_RUN=1 自解压（不依赖 FUSE/libfuse；
#   Arch 上即使装了 fuse2，挂载也常被硬化内核/权限拒绝——自解压永远可用）。
#   想强制直接挂载：APPIMAGE_EXTRACT_AND_RUN=0；想沿用：设任意非 0 值。
#   仍失败（GitHub 下载损坏）：rm -rf ~/.cache/tauri 后重试。
#   也可以手动：APPIMAGE_EXTRACT_AND_RUN=1 npm run tauri:build
#   仍不可用（如在无 FUSE 的打包机上）：--bundles deb 或 --no-bundle 跳过 AppImage。
#
# ■ 交叉构建（--target <triple>）说明：
#   桌面端打包器是平台绑定的，规则与 CLI 二进制不同：
#   - Linux → Linux (x86_64/aarch64)：可行，但 aarch64 交叉需要 aarch64 的 webkit2gtk
#     开发包 + 交叉 sysroot；没有预编译 SDK 的环境基本不现实——请在 arm64 实机上构建。
#   - Linux → Windows (x86_64-pc-windows-msvc / -gnu)：cargo 能跑，但 Tauri 的 Windows
#     打包（NSIS/MSI）额外需要 Windows SDK / WebView2 SDK；本脚本会在前置检查阶段提示。
#     正式分发请在 Windows 上运行本脚本（或直接跑 scripts\build-desktop.sh，host=Windows）。
#   - 任何 → macOS：需要 macOS + Xcode；dmg/pkg 只能在 macOS 上产出。
#   结论：跨大 OS 交叉打包请用 CI 矩阵（见 .github/workflows/ci.yml）；本参数主要用于
#   "同 OS 换架构"以及在 Windows/macOS 上显式指定 msvc/arm 目标。
#
# 用法：
#   scripts/build-desktop.sh                # 全量构建（默认产出 AppImage）
#   scripts/build-desktop.sh --check        # 仅检查前置依赖（含诊断，不构建）
#   scripts/build-desktop.sh --bundles "appimage,deb"
#   scripts/build-desktop.sh --bundles rpm
#   scripts/build-desktop.sh --no-bundle    # 只要裸二进制（src-tauri/target/release/ariaflow-desktop）
#   scripts/build-desktop.sh --debug        # debug 构建（快速迭代，不建议分发）
#   scripts/build-desktop.sh --target aarch64-unknown-linux-gnu   # 交叉构建（见上文说明）
set -euo pipefail
cd "$(dirname "$0")/.."
REPO_ROOT="$PWD"

MODE_CHECK=0
BUNDLES="appimage"
NO_BUNDLE=0
DEBUG_FLAG=0
DESKTOP_TARGET=""   # 空 = host（默认，行为与以前完全一致）
while [ $# -gt 0 ]; do
  arg="$1"
  case "$arg" in
    --check) MODE_CHECK=1; shift ;;
    --no-bundle) NO_BUNDLE=1; shift ;;
    --debug) DEBUG_FLAG=1; shift ;;
    --bundles) BUNDLES="${2:?--bundles 需要一个参数，例如 --bundles appimage,deb}"; shift 2 ;;
    --bundles=*) BUNDLES="${arg#*=}"; shift ;;
    --target) DESKTOP_TARGET="${2:?--target 需要一个 triple 参数，例如 --target x86_64-pc-windows-msvc}"; shift 2 ;;
    --target=*) DESKTOP_TARGET="${arg#*=}"; shift ;;
    -h|--help) sed -n '2,46p' "$0"; exit 0 ;;
    *) echo "未知参数: $arg（-h 查看用法）" >&2; exit 2 ;;
  esac
done

# 目标平台画像：决定跳过哪些 host-only 的前置检查
IS_LINUX_TARGET=0
case "$DESKTOP_TARGET" in
  "" ) IS_LINUX_TARGET=1 ;;                        # host（脚本本就假定 Linux）
  *linux*) IS_LINUX_TARGET=1 ;;
esac
IS_WINDOWS_TARGET=0
case "$DESKTOP_TARGET" in *windows*) IS_WINDOWS_TARGET=1 ;; esac
IS_MACOS_TARGET=0
case "$DESKTOP_TARGET" in *apple-darwin*) IS_MACOS_TARGET=1 ;; esac


log()  { printf '\033[1;34m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m警告:\033[0m %s\n' "$*" >&2; }
die()  { printf '\033[1;31m错误:\033[0m %s\n' "$*" >&2; exit 1; }
have() { command -v "$1" >/dev/null 2>&1; }

# ---------- 1) 前置依赖检查 ----------
MISSING=()
for tool in node npm cargo pkg-config; do
  have "$tool" || MISSING+=("$tool")
done

# WebKitGTK 4.1 开发包（这是桌面端唯一无法静态化的系统依赖）；仅 Linux 目标需要
if [ "$IS_LINUX_TARGET" = "1" ]; then
  if pkg-config --exists webkit2gtk-4.1 2>/dev/null; then
    WK=$(pkg-config --modversion webkit2gtk-4.1)
  else
    WK=""
    MISSING+=("webkit2gtk-4.1 开发包")
  fi
else
  WK="n/a (非 Linux 目标)"
fi
# AppImage 打包链：linuxdeploy 及其 plugin 是 AppImage + 需要 patchelf
PATCHELF_OK=0; have patchelf && PATCHELF_OK=1
# AppImage type-2 运行时直接挂载需要 /dev/fuse + libfuse.so.2。
# 检测只做"信息展示"：打包默认走 APPIMAGE_EXTRACT_AND_RUN=1（自解压，不需要 FUSE/libfuse），
# 除非用户自己设了 APPIMAGE_EXTRACT_AND_RUN（非 0）才沿用——检测不准也不再卡住构建。
LIBFUSE_SO=""
LDCONFIG_OUTPUT=""
if [ "$IS_LINUX_TARGET" = "1" ] && have ldconfig; then
  # 先完整读取 ldconfig 输出，避免 awk 提前退出导致 ldconfig 收到 SIGPIPE
  # （pipefail + set -e 下 141 会让脚本直接退出）
  LDCONFIG_OUTPUT=$(ldconfig -p 2>/dev/null || true)
  # 再从变量中提取路径，不再与 ldconfig 形成管道
  LIBFUSE_SO=$(awk '
      /libfuse\.so\.2|libfuse\.so/ {
          print $NF
          exit
      }
  ' <<< "$LDCONFIG_OUTPUT")
fi
if [ "$IS_LINUX_TARGET" = "1" ] && [[ -z "$LIBFUSE_SO" ]]; then
  # ldconfig 不可用/不含 libfuse 时，常见多架构库目录兜底
  for f in /usr/lib/libfuse.so.2 /usr/lib64/libfuse.so.2 /lib/libfuse.so.2 /lib64/libfuse.so.2 /usr/lib/*-linux-gnu/libfuse.so.2 /lib/*-linux-gnu/libfuse.so.2; do
    if [[ -e "$f" ]]; then LIBFUSE_SO="$f"; break; fi
  done
fi
DEV_FUSE=no; [[ -e /dev/fuse ]] && DEV_FUSE=yes
if [ "$IS_LINUX_TARGET" = "1" ]; then
  echo "fuse 诊断: /dev/fuse=$DEV_FUSE  libfuse.so.2=${LIBFUSE_SO:-未找到}"
fi
# ld.so.cache 污染检测：第三方（如 VMware）把私有库目录注入全局缓存后，
# linuxdeploy 的 gtk 插件会把 libgobject 等解析到极旧的私有版本（依赖 libffi.so.6）而失败。
if [[ -n "$LDCONFIG_OUTPUT" ]]; then
  GOBJECT_PATH=$(awk '/libgobject-2\.0\.so\.0/ {print $NF; exit}' <<< "$LDCONFIG_OUTPUT")
  case "$GOBJECT_PATH" in
    *vmware*|*VMware*)
      warn "ld.so.cache 把 libgobject-2.0.so.0 解析到 VMware 私有库：$GOBJECT_PATH"
      echo >&2 "      gtk 插件会随之部署极旧的 gobject 并报 'Could not find dependency: libffi.so.6'（ERROR: Failed to run plugin: gtk）。"
      echo >&2 "      修复（不影响 VMware 运行，它自带库搜索路径）："
      echo >&2 "        grep -ri vmware /etc/ld.so.conf /etc/ld.so.conf.d/   # 找到注入的条目"
      echo >&2 "        sudo mv /etc/ld.so.conf.d/<vmware相关>.conf /root/vmware.conf.bak"
      echo >&2 "        sudo ldconfig && ldconfig -p | grep libgobject-2.0   # 应指向 /usr/lib 而非 /usr/lib/vmware"
      echo >&2 "      然后重新运行本脚本。"
      ;;
  esac
fi
# 行为（不再依据检测结果决定，避免误判卡构建）：
#   用户设 APPIMAGE_EXTRACT_AND_RUN（非 0）→ 沿用；设 0 → 强制直接挂载（风险自负）；未设 → 一律自解压
if [[ -n "${APPIMAGE_EXTRACT_AND_RUN:-}" && "${APPIMAGE_EXTRACT_AND_RUN}" != "0" ]]; then
  FUSE_MODE="user-extract"
elif [[ -n "${APPIMAGE_EXTRACT_AND_RUN:-}" ]]; then
  FUSE_MODE="user-mount"
else
  FUSE_MODE="auto-extract"   # 默认自解压——即使 fuse 齐备，硬化内核也常拒绝非特权挂载
fi

echo "平台: $(grep -m1 PRETTY_NAME /etc/os-release 2>/dev/null | cut -d= -f2- | tr -d '\"')"
PATCHELF_DESC=缺失; if [ $PATCHELF_OK = 1 ]; then PATCHELF_DESC=$(patchelf --version | awk '{print $2}'); fi
echo "webkit2gtk-4.1: ${WK:-未找到}  |  patchelf: $PATCHELF_DESC  |  AppImage 模式: $FUSE_MODE"

if (( ${#MISSING[@]} )); then
  warn "缺少前置依赖：${MISSING[*]}"
  case "$(. /etc/os-release 2>/dev/null; echo "${ID:-}${ID_LIKE:+,$ID_LIKE}")" in
    *arch*|*blackarch*)
      echo >&2 "Arch/BlackArch 修复： sudo pacman -S --needed nodejs npm rust webkit2gtk-4.1 gtk3 libayatana-appindicator librsvg patchelf fuse2 base-devel"
      ;;
    *debian*|*ubuntu*)
      echo >&2 "Debian/Ubuntu 修复： sudo apt-get install -y nodejs npm rustc cargo libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev patchelf fuse libfuse2"
      ;;
    *fedora*)
      echo >&2 "Fedora 修复： sudo dnf install -y nodejs npm rust cargo webkit2gtk4.1-devel gtk3-devel libappindicator-gtk3-devel librsvg2-devel patchelf fuse fuse-libs"
      ;;
    *) echo >&2 "请按上游文档安装 node(≥18)、rust(≥1.80)、webkit2gtk-4.1 开发包、patchelf、fuse2。" ;;
  esac
  [[ "$MODE_CHECK" == "1" ]] && exit 1 || die "前置依赖未满足，见上方安装提示。"
fi
echo "前置依赖检查通过。"

if [[ "$MODE_CHECK" == "1" ]]; then
  echo
  echo "AppImage 打包链诊断："
  case "$FUSE_MODE" in
    user-extract) echo "  APPIMAGE_EXTRACT_AND_RUN=$APPIMAGE_EXTRACT_AND_RUN（用户设置）→ 自解压运行" ;;
    user-mount)   echo "  APPIMAGE_EXTRACT_AND_RUN=0（用户强制直接挂载）→ fuse 再齐备，硬化内核也可能拒绝挂载而重演 'failed to run linuxdeploy'" ;;
    auto-extract) echo "  fuse 检测: /dev/fuse=$DEV_FUSE  libfuse.so.2=${LIBFUSE_SO:-未找到}（仅供参考）"
                  echo "  → 打包将自动 APPIMAGE_EXTRACT_AND_RUN=1 自解压（默认，不依赖 FUSE/libfuse）" ;;
  esac
  (( PATCHELF_OK )) || echo "  patchelf: 缺失 → linuxdeploy 必失败，请先安装 patchelf"
  exit $(( PATCHELF_OK ? 0 : 1 ))
fi

# ---------- 2) AppImage 默认走自解压（不依赖 FUSE，CI/打包机通用） ----------
if [[ "$NO_BUNDLE" == "0" && "$BUNDLES" == *appimage* && "$IS_LINUX_TARGET" == "1" ]]; then
  case "$FUSE_MODE" in
    user-extract) : ;;
    user-mount) warn "APPIMAGE_EXTRACT_AND_RUN=0：强制直接挂载——硬化内核/权限拒绝时会重演 'failed to run linuxdeploy'（建议不设置或设为 1）" ;;
    auto-extract)
      export APPIMAGE_EXTRACT_AND_RUN=1
      warn "默认 → 已自动 export APPIMAGE_EXTRACT_AND_RUN=1（linuxdeploy 自解压运行，不依赖 FUSE/libfuse）"
      ;;
  esac
  # 无论自解压还是挂载，打包仍失败时，最常见原因是 GitHub 下载损坏的缓存：
  echo "提示: 若仍报 'failed to run linuxdeploy'，先执行： rm -rf ~/.cache/tauri  后重跑；"
  echo "      诊断： file ~/.cache/tauri/linuxdeploy-x86_64.AppImage（应为 ELF 可执行文件，不是 HTML/损坏文件）"
fi
# 最佳尝试（best-effort）：若 ld.so.cache 被 VMware 等第三方目录污染，
# 让构建进程优先解析系统库（LD_LIBRARY_PATH 先于 ld.so.cache 生效）。
# 不能保证 linuxdeploy 对每个依赖都走 dlopen——根治方案仍是清理 /etc/ld.so.conf.d/ 中的 vmware 条目。
if [ "$IS_LINUX_TARGET" = "1" ] && [[ "${ARIAFLOW_NO_LD_LIBRARY_PATH:-0}" != "1" ]]; then
  for d in /usr/lib /usr/lib64 /usr/lib/x86_64-linux-gnu /lib /lib/x86_64-linux-gnu; do
    [[ -d "$d" ]] && LD_LIBRARY_PATH="${LD_LIBRARY_PATH:+$LD_LIBRARY_PATH:}$d"
  done
  export LD_LIBRARY_PATH
fi

# 跨 OS 目标：显式提醒打包器不可用（避免数小时编译后才在 bundle 步骤报错）
if [ "$IS_LINUX_TARGET" != "1" ]; then
  if [ "$IS_WINDOWS_TARGET" = "1" ]; then
    warn "目标为 Windows ($DESKTOP_TARGET)：NSIS/MSI 需要 Windows 主机 + WebView2 SDK；本脚本继续编译，bundle 阶段可能失败。"
  elif [ "$IS_MACOS_TARGET" = "1" ]; then
    warn "目标为 macOS ($DESKTOP_TARGET)：dmg/pkg 只能在 macOS 主机上产出；本脚本继续编译，bundle 阶段可能失败。"
  else
    warn "目标 $DESKTOP_TARGET 不是 Linux：跳过 Linux 前置检查（webkit2gtk/fuse/patchelf）。"
  fi
  # Windows/mac 目标默认不套用 appimage bundling
  if [[ "$BUNDLES" == *appimage* ]]; then
    BUNDLES=""
    warn "已忽略 --bundles appimage（目标非 Linux）；改为不打包裸二进制。如需安装包请在对应 OS 上用 --bundles nsis 等。"
  fi
fi
if [[ "$BUNDLES" == *appimage* && "$PATCHELF_OK" == "0" && "$IS_LINUX_TARGET" == "1" ]]; then
  die "patchelf 缺失时 linuxdeploy 一定会失败，请先安装 patchelf（Arch: pacman -S patchelf）。"
fi

# ---------- 3) 前端构建 ----------
log "构建前端（vue-tsc 类型检查 + vite 产物）"
cd "$REPO_ROOT/apps/desktop"
npm ci
npm run build

# ---------- 4) Tauri 构建 ----------
TAURI_ARGS=(build)
[[ "$DEBUG_FLAG" == "1" ]] && TAURI_ARGS+=(--debug)
if [[ -n "$DESKTOP_TARGET" ]]; then
  TAURI_ARGS+=(--target "$DESKTOP_TARGET")
  log "目标平台：$DESKTOP_TARGET"
fi
if [[ "$NO_BUNDLE" == "1" || -z "$BUNDLES" ]]; then
  TAURI_ARGS+=(--no-bundle)
  log "跳过安装包，仅产出二进制（--no-bundle）"
else
  TAURI_ARGS+=(--bundles "$BUNDLES")
  log "打包类型：$BUNDLES"
fi
npm run tauri -- "${TAURI_ARGS[@]}"

# ---------- 5) 产物体检 ----------
echo
BUNDLE_ROOT="$REPO_ROOT/apps/desktop/src-tauri/target"
[[ "$DEBUG_FLAG" == "1" ]] && PROFILE_DIR=debug || PROFILE_DIR=release
if [[ -n "$DESKTOP_TARGET" ]]; then
  BIN_DIR="$BUNDLE_ROOT/$DESKTOP_TARGET/$PROFILE_DIR"
else
  BIN_DIR="$BUNDLE_ROOT/$PROFILE_DIR"
fi
log "产物："
find "$BIN_DIR" -maxdepth 1 -type f \( -name 'ariaflow-desktop*' \) -print0 | while IFS= read -r -d '' f; do
  file "$f" || true
  case "$DESKTOP_TARGET" in
    ""|*linux*)
      echo "  ldd 残余动态库（前 12 条）："; ldd "$f" 2>/dev/null | awk 'NR <= 12 {print "    "$1}'
      echo "    （…其余 webkit2gtk/javascriptcore/soup/gtk/glib 等系统库，AppImage 已全部打包进去）"
      ;;
    *windows*|*apple*)
      echo "  （非 Linux 目标，跳过 ldd；用对应平台工具检查：Windows → dumpbin /dependents，macOS → otool -L）"
      ;;
  esac
done
find "$BIN_DIR/bundle" -maxdepth 2 -type f \( -name '*.AppImage' -o -name '*.deb' -o -name '*.rpm' \) -print 2>/dev/null | sed 's/^/  安装包: /'
echo
echo "提示：AppImage 拷贝到目标机器需要 FUSE，或执行  ./xxx.AppImage --appimage-extract-and-run"
