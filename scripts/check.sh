#!/usr/bin/env bash
# 本地与 CI 同款检查：fmt + clippy + tests
set -euo pipefail
cd "$(dirname "$0")/.."
export PATH="$HOME/.cargo/bin:$PATH"

echo "== cargo fmt --check =="
cargo fmt --all -- --check

echo "== cargo clippy (deny warnings) =="
cargo clippy --workspace --all-targets -- -D warnings

echo "== cargo test =="
cargo test --workspace

echo "== 全部检查通过 =="
