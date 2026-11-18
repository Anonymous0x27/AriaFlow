#!/usr/bin/env bash
# 桌面前端：类型检查 + 生产构建
set -euo pipefail
cd "$(dirname "$0")/../apps/desktop"

npm ci
npm run build

echo
echo "前端构建完成 → apps/desktop/dist/"
echo "本地预览: npx vite preview --port 4173"
echo "桌面窗口: npm run tauri:dev   （需系统 WebView，见 ../../docs/BUILD.md）"
