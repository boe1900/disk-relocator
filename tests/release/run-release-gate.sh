#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/../.." && pwd)"

run_step() {
  local title="$1"
  shift
  echo "[release-gate] ${title}"
  "$@"
}

assert_file() {
  local path="$1"
  if [[ ! -f "$path" ]]; then
    echo "[release-gate] missing required file: $path" >&2
    exit 1
  fi
}

cd "$ROOT_DIR"

echo "[release-gate] start"

run_step "build frontend" npm run build

run_step "rust compile checks" bash -lc "cd src-tauri && cargo check"

run_step "gate A: 20 rounds migrate+rollback" \
  bash -lc "cd src-tauri && cargo test migration::tests::migration_and_rollback_20_rounds"

run_step "gate B1: interruption recovery to healthy" \
  bash -lc "cd src-tauri && cargo test recovery::tests::recovery_marks_symlinked_migrate_as_healthy"

run_step "gate B2: interruption recovery rollback" \
  bash -lc "cd src-tauri && cargo test recovery::tests::recovery_rolls_back_partial_migrate_state"

run_step "gate C: disk offline alert" \
  bash -lc "cd src-tauri && cargo test health::tests::health_check_marks_offline_target_root_as_degraded"

run_step "gate D1: reconcile drift detect" \
  bash -lc "cd src-tauri && cargo test reconcile::tests::reconcile_detects_temp_residue"

run_step "gate D2: reconcile safe-fix" \
  bash -lc "cd src-tauri && cargo test reconcile::tests::reconcile_safe_fix_marks_stale_state_as_rolled_back"

assert_file "$ROOT_DIR/docs/release-test-matrix.md"
assert_file "$ROOT_DIR/docs/release-known-limitations.md"
assert_file "$ROOT_DIR/docs/rollback-runbook.md"
assert_file "$ROOT_DIR/docs/faq.md"

echo "[release-gate] docs check: OK"
echo "[release-gate] PASS"
