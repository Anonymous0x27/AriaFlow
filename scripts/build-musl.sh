#!/usr/bin/env bash
# 静态 musl 构建：ariaflowd / ariaflow / ariaflow-mcp / ariaflow-native-host
#
# 产物【不依赖任何系统库】（glibc / OpenSSL / zlib / sqlite3 全部静态或内嵌），
# 可直接拷到其他 x86_64 Linux 发行版（含 Alpine、scratch 容器）运行。
#
# 工作原理（与 docs/BUILD.md 一致）：
#   - 全仓 HTTP/FTPS 已统一为 rustls(ring) + 内嵌 Mozilla roots（reqwest / suppaftp），不链 OpenSSL；
#   - SFTP（ssh2→libssh2-sys）与解压（libz-sys）在 Unix 下改为【源码构建】（vendored OpenSSL / zlib）；
#   - SQLite 用 sqlx 内置 bundled 源码构建；
#   - 仅 SFTP 相关保留 OpenSSL，且为静态内嵌，运行时无系统依赖。
#
# 用法：
#   scripts/build-musl.sh                  # release 构建 + 前端（rust-embed 把 web UI 嵌入 ariaflowd）
#   scripts/build-musl.sh --skip-web       # 跳过前端构建（apps/desktop/dist 已存在时）
#   scripts/build-musl.sh --debug          # debug 构建（rust-embed 走文件系统，不强制要求 dist）
#   scripts/build-musl.sh --target aarch64-unknown-linux-musl
#                                          # 交叉静态构建（参数形式；与 TARGET_TRIPLE 环境变量等价，CLI 优先）
#   TARGET_TRIPLE=aarch64-unknown-linux-musl scripts/build-musl.sh
#       ^ verified 2026-11 (build + qemu-aarch64 run + HTTPS probe OK).
#       Without an aarch64 musl-gcc, wrap zig: python3 -m ziglang cc -target aarch64-linux-musl
#       (drop --target=* args and -Wl,--fix-cortex-a53-843419; use -C link-self-contained=n).
#       FreeBSD is currently NOT buildable (librqbit-dualstack-sockets uses Linux-only bind_device); see docs/BUILD.md.
#
# 前置要求：Rust ≥ 1.80（rustup），以及目标平台的 musl C 工具链：
#   - Debian/Ubuntu: sudo apt install musl-tools            # 提供 musl-gcc
#   - Fedora:        sudo dnf install musl-gcc
#   - Alpine:        apk add musl-dev                       # 原生 musl，直接 cargo build --target 即可
# 脚本会自动 rustup target add $TARGET。
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"

TARGET="${TARGET_TRIPLE:-x86_64-unknown-linux-musl}"
PROFILE="release"
SKIP_WEB=0

while [ $# -gt 0 ]; do
  case "$1" in
    --skip-web) SKIP_WEB=1; shift ;;
    --debug)    PROFILE="dev"; SKIP_WEB=1; shift ;;
    --target)
      [ $# -ge 2 ] || { echo "error: --target 需要一个 triple 参数" >&2; exit 2; }
      TARGET="$2"; shift 2 ;;
    --target=*) TARGET="${1#*=}"; shift ;;
    -h|--help)
      sed -n 2,29p "$0"
      exit 0
      ;;
    *) echo "unknown option: $1 (see --help)" >&2; exit 2 ;;
  esac
done

case "$TARGET" in
  *-linux-musl) : ;;   # 本脚本仅服务 Linux musl 静态目标
  *-linux-gnu|*-linux-gnueabihf)
    echo "error: $TARGET 是 glibc 目标，本脚本只做 musl 静态构建。" >&2
    echo "       glibc 交叉构建请用 scripts/build.sh --target $TARGET（产物仍需目标机 glibc）。" >&2
    exit 2 ;;
  *)
    echo "error: $TARGET 不是 Linux musl 目标（如 x86_64-unknown-linux-musl / aarch64-unknown-linux-musl）。" >&2
    exit 2 ;;
esac

if ! command -v cargo >/dev/null 2>&1; then
  echo "error: cargo 未找到。请先安装 Rust: https://rustup.rs/" >&2
  exit 1
fi

# ---------- 1) rustup target ----------
if command -v rustup >/dev/null 2>&1; then
  if ! rustup target list --installed | grep -qx "$TARGET"; then
    echo "> rustup target add $TARGET"
    rustup target add "$TARGET"
  fi
else
  echo "warn: 未找到 rustup；请确认 $TARGET 的 std 已安装 (rustup target add $TARGET)" >&2
fi

# ---------- 2) musl C 工具链（交叉构建时需要；原生 musl 不需要）----------
# CARGO_TARGET_*_LINKER 用于 rustc 的最终链接；CC_*（小写、- 换成 _）用于 cc-rs 编译 C 依赖。
uv_target() { printf '%s' "$TARGET" | tr 'a-z-' 'A-Z_'; }
lv_target() { printf '%s' "$TARGET" | tr 'A-Z-' 'a-z_'; }

NATIVE_MUSL=0
if command -v cc >/dev/null 2>&1 && cc -dumpmachine 2>/dev/null | grep -q musl; then
  NATIVE_MUSL=1   # 原生 musl 系统（如 Alpine）：系统 cc 即可，无需指定链接器
fi

find_musl_gcc() {
  local cand
  case "$TARGET" in
    x86_64-unknown-linux-musl)   cand="x86_64-linux-musl-gcc musl-gcc musl-cc" ;;
    aarch64-unknown-linux-musl)  cand="aarch64-linux-musl-gcc aarch64-musl-gcc" ;;
    armv7-unknown-linux-musleabihf) cand="arm-linux-musleabihf-gcc" ;;
    i686-unknown-linux-musl)     cand="i686-linux-musl-gcc i386-linux-musl-gcc" ;;
    *)                           cand="${TARGET%%-*}-linux-musl-gcc ${TARGET%%-*}-musl-gcc" ;;
  esac
  for c in $cand; do
    if command -v "$c" >/dev/null 2>&1; then echo "$c"; return 0; fi
  done
  return 1
}

# Classic ET_EXEC static binaries (see note below). Append to any user-provided
# RUSTFLAGS rather than overwriting.
RUSTFLAGS_VAR="CARGO_TARGET_$(uv_target)_RUSTFLAGS"
_ET_EXEC_FLAG="-C relocation-model=static"
case " ${!RUSTFLAGS_VAR:-} " in
  *" $_ET_EXEC_FLAG "*) ;;
  *) export "$RUSTFLAGS_VAR=${!RUSTFLAGS_VAR:+${!RUSTFLAGS_VAR} }$_ET_EXEC_FLAG" ;;
esac
# NOTE (why ET_EXEC): the musl target defaults to -static-pie (ET_DYN, self-
# relocating GOT). Such binaries built fine with Debian's musl-gcc but
# consistently died with "general protection fault" at a fixed instruction
# offset (before main, all four binaries) on some Arch/BlackArch systems —
# the relocation machinery at startup is the only thing ET_EXEC removes.
# Round-14 (2026-11): enforced for every TARGET; x86_64 verified end-to-end.

if [ "$NATIVE_MUSL" = "0" ]; then
  MUSL_GCC="$(find_musl_gcc || true)"
  if [ -z "${MUSL_GCC:-}" ]; then
    cat >&2 <<EOF
error: 找不到目标 $TARGET 的 musl C 工具链。
  Debian/Ubuntu: sudo apt install musl-tools        # 提供 musl-gcc
  Fedora:        sudo dnf install musl-gcc
  Arch:          sudo pacman -S musl

Cross targets (e.g. aarch64) may use a zig wrapper instead: python3 -m ziglang cc -target <arch>-linux-musl
(drop --target=* args; aarch64 also needs -Wl,--fix-cortex-a53-843419 dropped and
CARGO_TARGET_AARCH64_UNKNOWN_LINUX_MUSL_RUSTFLAGS=-C link-self-contained=n). See docs/BUILD.md.

安装后重跑本脚本。
EOF
    exit 1
  fi
  echo "> 使用 musl C 工具链: $MUSL_GCC"
  export "CARGO_TARGET_$(uv_target)_LINKER=$MUSL_GCC"
  export "CC_$(lv_target)=$MUSL_GCC"
  export "AR_$(lv_target)=ar"
fi

# ---------- 3) 前端（release 的 ariaflowd 通过 rust-embed 内嵌 web UI）----------
if [ "$SKIP_WEB" = "1" ]; then
  echo "> 跳过前端构建 (--skip-web/--debug)；使用已有 apps/desktop/dist"
  if [ "$PROFILE" = "release" ] && [ ! -f apps/desktop/dist/index.html ]; then
    echo "error: apps/desktop/dist/index.html 不存在，release 的 rust-embed 会失败。先构建前端或去掉 --skip-web。" >&2
    exit 1
  fi
else
  if command -v npm >/dev/null 2>&1; then
    (cd apps/desktop && npm ci && npm run build)
    echo "web UI: apps/desktop/dist/ 将嵌入 ariaflowd"
  else
    echo "warn: npm 未找到，跳过前端构建。"
    if [ ! -f apps/desktop/dist/index.html ]; then
      echo "error: apps/desktop/dist/index.html 不存在，release 构建（rust-embed）将失败。" >&2
      exit 1
    fi
  fi
fi

# ---------- 4) 静态链接构建 ----------
PROFILE_FLAG=(--release)
OUT_DIR="target/$TARGET/release"
if [ "$PROFILE" = "dev" ]; then
  PROFILE_FLAG=()
  OUT_DIR="target/$TARGET/debug"
fi

echo "> cargo build --target $TARGET ${PROFILE_FLAG[*]}"
cargo build --target "$TARGET" "${PROFILE_FLAG[@]}" \
  -p ariaflow-cli \
  -p ariaflow-daemon \
  -p ariaflow-mcp \
  -p ariaflow-browser

echo
echo "产物 ($OUT_DIR):"
for bin in ariaflow ariaflowd ariaflow-mcp ariaflow-native-host; do
  if [ -f "$OUT_DIR/$bin" ]; then
    ls -lh "$OUT_DIR/$bin"
    if command -v file >/dev/null 2>&1; then
      file "$OUT_DIR/$bin" || true
    fi
  fi
done

# ---------- 5) post-build smoke test (only when the host can execute the target) ----------
host_machine="$(uname -m)"
runnable=0
case "$host_machine:$TARGET" in
  x86_64:x86_64-*|aarch64:aarch64-*|arm*:armv7-*|riscv64:riscv64gc-*) runnable=1 ;;
esac
if [ "$runnable" = "1" ] && [ -x "$OUT_DIR/ariaflow" ]; then
  if "$OUT_DIR/ariaflow" --version >/dev/null 2>&1; then
    echo "> 冒烟测试: ariaflow --version OK"
  else
    cat >&2 <<'SMOKE_EOF'
error: 产物无法运行（ariaflow --version 崩溃或退出非零）。
这正是 2026-11 BlackArch 上遇到的问题：musl 默认 -static-pie（ET_DYN）产物在
部分发行版 userland 上启动即 #GP。本脚本已强制 -C relocation-model=static（ET_EXEC），
若仍失败，请检查：
  1) 是否残留旧 RUSTFLAGS 干扰（env | grep -i rustflags）；
  2) 系统 musl-gcc 是否混链了 glibc CRT（file $OUT_DIR/ariaflow 应显示
     \"statically linked\" 而非 static-pie linked）；
  3) 以 Alpine（原生 musl）或 zig 封装重建对照。
SMOKE_EOF
    exit 1
  fi
else
  echo "> 跳过冒烟测试（host=$host_machine 无法直接执行 $TARGET，或非 release 产物缺失）"
fi
echo
echo "验证: ldd $OUT_DIR/ariaflowd  （应显示 not a dynamic executable）"
