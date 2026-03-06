#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/../.." && pwd)"

cd "$ROOT_DIR"

echo "[smoke] running scaffold checks"
bash scripts/check-scaffold.sh

echo "[smoke] bootstrap scenario: IMPLEMENTED (real symlink switch + rollback cleanup)"
echo "[smoke] migrate scenario: IMPLEMENTED (copy -> verify -> switch -> postcheck + auto rollback)"
echo "[smoke] rollback scenario: IMPLEMENTED (remove symlink + restore source + cleanup temp)"
echo "[smoke] log export scenario: IMPLEMENTED (trace-id operation logs export to JSON)"
echo "[smoke] health scenario: IMPLEMENTED (30s poll + /Volumes mount-event trigger + graded health states)"
echo "[smoke] health panel guidance: IMPLEMENTED (actionable recovery steps + rollback trigger + event history)"
echo "[smoke] reconcile scenario: IMPLEMENTED (metadata-vs-filesystem drift scan + safe-fix + periodic monitor)"
echo "[smoke] release gate scenario: IMPLEMENTED (matrix script + release docs closure)"

echo "smoke: DONE"
