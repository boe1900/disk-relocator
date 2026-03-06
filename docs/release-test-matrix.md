# 发布前测试矩阵（V1）

日期：2026-03-05

## Gate A：20 轮迁移 + 回滚

- 目标：验证核心迁移事务与回滚长期重复后仍一致。
- 执行命令：
  - `cd src-tauri && cargo test migration::tests::migration_and_rollback_20_rounds`
- 通过标准：
  - 20 轮均成功。
  - 每轮结束后源路径为目录（非软链接）。
  - 无残留 `target/backup/temp`。

## Gate B：步骤中断恢复

- 目标：验证“中断后可恢复一致状态”。
- 执行命令：
  - `cd src-tauri && cargo test recovery::tests::recovery_marks_symlinked_migrate_as_healthy`
  - `cd src-tauri && cargo test recovery::tests::recovery_rolls_back_partial_migrate_state`
- 通过标准：
  - 中断后可判定为 `HEALTHY` 或恢复为 `ROLLED_BACK`。
  - 不出现元数据悬挂态。

## Gate C：外接盘离线告警

- 目标：验证离线场景在健康检查周期内可识别。
- 执行命令：
  - `cd src-tauri && cargo test health::tests::health_check_marks_offline_target_root_as_degraded`
- 通过标准：
  - 返回 `HEALTH_DISK_OFFLINE`。
  - 健康状态标记为 `degraded`。

## Gate D：对账漂移检测与 safe-fix

- 目标：验证元数据 vs 文件系统漂移可发现，可对安全项自动修复。
- 执行命令：
  - `cd src-tauri && cargo test reconcile::tests::reconcile_detects_temp_residue`
  - `cd src-tauri && cargo test reconcile::tests::reconcile_safe_fix_marks_stale_state_as_rolled_back`
- 通过标准：
  - 漂移项有明确 `code/suggestion/safe_fix_action`。
  - safe-fix 后状态与快照被正确同步。

## 一键门禁

- `bash tests/release/run-release-gate.sh`
