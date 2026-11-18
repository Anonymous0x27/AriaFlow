#!/usr/bin/env bash
# Release 构建 CLI / 守护进程 / MCP / 浏览器 native host
#
# 顺序很重要：ariaflowd 通过 rust-embed 在【编译期】把 apps/desktop/dist 烘焙进二进制，
# 所以先构建前端，再 cargo build --release。若 dist 缺失，release 编译会直接失败。
#
# 用法：
#   scripts/build.sh                      # 构建 host 目标（默认）
#   scripts/build.sh --target <triple>    # 交叉构建，如 aarch64-unknown-linux-gnu、
#                                         #   x86_64-pc-windows-gnu、armv7-unknown-linux-gnueabihf
#   scripts/build.sh --target <t> --static-crt
#                                         # Windows MSVC 目标附加 -C target-feature=+crt-static
#                                         #（静态链接 UCRT/VCRuntime；windows-msvc 默认自动附加，可用 --no-static-crt 关闭）
#   scripts/build.sh -h                   # 帮助
#
# 交叉构建前提（否则 cargo 在链接期报错）：
#   - rustup target add <triple>（脚本会自动执行）
#   - 对应目标的 C 工具链（cc-rs 编译 C 依赖用），常见搭配：
#       aarch64-unknown-linux-gnu      → gcc-aarch64-linux-gnu（Debian）或交叉 gcc
#       armv7-unknown-linux-gnueabihf  → gcc-arm-linux-gnueabihf
#       i686-unknown-linux-gnu         → gcc-i686-linux-gnu
#       x86_64-pc-windows-gnu          → mingw-w64（x86_64-w64-mingw32-gcc）
#       x86_64-pc-windows-msvc         → 不建议在 Linux 交叉；请在 Windows 上构建
#       *-apple-darwin                 → 需要 macOS SDK；请在 macOS 上构建
#   完全静态的 Linux 产物请用 scripts/build-musl.sh（产物不依赖任何系统 so）。
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"

# Host triple：rustc → rustup shim → toolchain 目录扫描；都拿不到时返回空串
host_triple() {
  if command -v rustc >/dev/null 2>&1; then
    rustc -vV | awk '/^host:/ {print $2; exit}'; return 0
  fi
  if command -v rustup >/dev/null 2>&1; then
    local rustc_bin
    rustc_bin="$(rustup which rustc 2>/dev/null)" || return 1
    "$rustc_bin" -vV | awk '/^host:/ {print $2; exit}'; return 0
  fi
  return 1
}
TARGET=""   # 空 = host（默认）；--target 显式给出
USER_STATIC_CRT=""   # "" = 按目标默认（windows-msvc: on）；yes/no = 显式
while [ $# -gt 0 ]; do
  case "$1" in
    --target)
      [ $# -ge 2 ] || { echo "error: --target 需要一个 triple 参数" >&2; exit 2; }
      TARGET="$2"; shift 2 ;;
    --target=*) TARGET="${1#*=}"; shift ;;
    --static-crt)    USER_STATIC_CRT="yes"; shift ;;
    --no-static-crt) USER_STATIC_CRT="no";  shift ;;
    -h|--help) sed -n '2,25p' "$0"; exit 0 ;;
    *) echo "未知参数: $1（-h 查看用法）" >&2; exit 2 ;;
  esac
done
HOST_TRIPLE="$(host_triple || true)"
EXPLICIT_TARGET=1
if [ -z "$TARGET" ]; then
  EXPLICIT_TARGET=0
  TARGET="$HOST_TRIPLE"
fi

STATIC_CRT="$USER_STATIC_CRT"
if [ -z "$STATIC_CRT" ]; then
  case "$TARGET" in *-pc-windows-msvc) STATIC_CRT="yes" ;; *) STATIC_CRT="no" ;; esac
fi

# 1) 前端 web 版（与桌面程序同款 UI），产物 apps/desktop/dist 会被 rust-embed 嵌入 ariaflowd
if command -v npm >/dev/null 2>&1; then
  (cd apps/desktop && npm ci && npm run build)
  echo "web UI: apps/desktop/dist/ 将在下面的 release 构建中被嵌入 ariaflowd"
else
  echo "warn: npm 未找到，跳过前端构建。"
  if [ ! -f apps/desktop/dist/index.html ]; then
    echo "error: apps/desktop/dist/index.html 不存在，release 构建（rust-embed）将失败。"
    exit 1
  fi
fi

# 2) rustup target（交叉构建需要；host 目标无需处理）
if command -v rustup >/dev/null 2>&1; then
  if ! rustup target list --installed | grep -qx "$TARGET"; then
    echo "> rustup target add $TARGET"
    rustup target add "$TARGET"
  fi
else
  echo "warn: 未找到 rustup；请确认 $TARGET 的 std 已安装" >&2
fi

# 3) 交叉 C 工具链（cc-rs 编译 libz/libssh2/openssl-sys 等）；host 构建跳过
uv_target() { printf '%s' "$TARGET" | tr 'a-z-' 'A-Z_'; }
lv_target() { printf '%s' "$TARGET" | tr 'A-Z-' 'a-z_'; }

if [ "$EXPLICIT_TARGET" = "1" ] && [ "$TARGET" != "$HOST_TRIPLE" ]; then
  case "$TARGET" in
    aarch64-unknown-linux-gnu)      CC_BIN="aarch64-linux-gnu-gcc" ;;
    armv7-unknown-linux-gnueabihf)  CC_BIN="arm-linux-gnueabihf-gcc" ;;
    i686-unknown-linux-gnu)         CC_BIN="i686-linux-gnu-gcc" ;;
    riscv64gc-unknown-linux-gnu)    CC_BIN="riscv64-linux-gnu-gcc" ;;
    x86_64-pc-windows-gnu)          CC_BIN="x86_64-w64-mingw32-gcc" ;;
    *-pc-windows-msvc)
      echo "error: $TARGET 无法在 Linux 上干净交叉（需要 MSVC / Windows SDK）。" >&2
      echo "       请在 Windows 机器上运行 scripts\build.sh --target $TARGET，或使用 --target x86_64-pc-windows-gnu + mingw-w64。" >&2
      exit 1 ;;
    *-apple-darwin|*-apple-ios*)
      echo "error: $TARGET 需要 macOS SDK，只能在 macOS 上构建。" >&2
      exit 1 ;;
    *-linux-musl)
      echo "error: $TARGET 是 musl 静态目标，请改用 scripts/build-musl.sh --target $TARGET（产物不依赖任何系统 so）。" >&2
      exit 1 ;;
    *)                              CC_BIN="" ;;
  esac
  if [ -n "$CC_BIN" ]; then
    if ! command -v "$CC_BIN" >/dev/null 2>&1; then
      echo "error: 找不到 C 交叉工具链 $CC_BIN（cc-rs 编译 C 依赖必需）。" >&2
      echo "       Debian/Ubuntu: sudo apt install gcc-aarch64-linux-gnu  # 按目标替换" >&2
      echo "       Windows GNU:   sudo apt install mingw-w64" >&2
      exit 1
    fi
    export "CARGO_TARGET_$(uv_target)_LINKER=$CC_BIN"
    export "CC_$(lv_target)=$CC_BIN"
    export "AR_$(lv_target)=$(command -v ar || echo ar)"
    echo "> 交叉 C 工具链: $CC_BIN"
  else
    echo "warn: 未知的交叉目标 $TARGET，未自动配置 C 工具链；若构建失败，请自行设置 CARGO_TARGET_$(uv_target)_LINKER 与 CC_$(lv_target)" >&2
  fi
fi

# 4) Windows MSVC 目标：静态链接 UCRT/VCRuntime（运行机器无需安装 VC++ Redistributable）
if [ "$STATIC_CRT" = "yes" ]; then
  RUSTFLAGS_VAR="CARGO_TARGET_$(uv_target)_RUSTFLAGS"
  _CRT_FLAG="-C target-feature=+crt-static"
  case " ${!RUSTFLAGS_VAR:-} " in
    *" $_CRT_FLAG "*) ;;
    *) export "$RUSTFLAGS_VAR=${!RUSTFLAGS_VAR:+${!RUSTFLAGS_VAR} }$_CRT_FLAG" ;;
  esac
  echo "> crt-static: 已附加 $_CRT_FLAG"
fi

TARGET_FLAG=()
OUT_DIR="target/release"
if [ "$EXPLICIT_TARGET" = "1" ] && [ -n "$HOST_TRIPLE" ] && [ "$TARGET" != "$HOST_TRIPLE" ]; then
  TARGET_FLAG=(--target "$TARGET")
  OUT_DIR="target/$TARGET/release"
elif [ "$EXPLICIT_TARGET" = "1" ] && [ -z "$HOST_TRIPLE" ]; then
  # 无法识别 host：照常把 --target 交给 cargo（cargo 自己知道自己的 host）
  TARGET_FLAG=(--target "$TARGET")
  OUT_DIR="target/$TARGET/release"
fi

# 5) Rust 产物（此时 dist 已就绪，ariaflowd 会嵌入它）
cargo build --release "${TARGET_FLAG[@]}" \
  -p ariaflow-cli \
  -p ariaflow-daemon \
  -p ariaflow-mcp \
  -p ariaflow-browser

echo
echo "产物目录: $OUT_DIR/（ariaflowd 已内嵌 web UI）"
for bin in ariaflow ariaflowd ariaflow-mcp ariaflow-native-host; do
  case "$TARGET" in
    *windows*) f="$OUT_DIR/$bin.exe" ;;
    *)         f="$OUT_DIR/$bin" ;;
  esac
  if [ -f "$f" ]; then
    ls -lh "$f"
    if command -v file >/dev/null 2>&1; then file "$f" || true; fi
    # 仅 host 可执行的 ELF 打印 ldd（交叉/PE 产物跳过）
    case "$TARGET" in
      *linux*|*android*) ldd "$f" 2>/dev/null | awk '{print "    "$1}' || true ;;
    esac
  fi
done
