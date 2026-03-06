#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"

cd "$ROOT_DIR"

echo "[1/4] validate profile json"
jq . specs/v1/app-profiles.json >/dev/null

echo "[2/4] frontend type-check and build"
npm run build >/dev/null

echo "[3/4] rust check"
( cd src-tauri && cargo check >/dev/null )

echo "[4/4] schema file exists"
[ -f specs/v1/sqlite-schema.sql ]

echo "scaffold-check: OK"
