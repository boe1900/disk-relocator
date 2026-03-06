# Tauri Command Contract v1

本文件冻结命令签名和数据结构。当前为实现前契约，不含业务逻辑。

## 1. 通用约定

1. 时间字段统一使用 ISO-8601 UTC 字符串（例：`2026-03-05T10:00:00Z`）。
2. 所有命令返回 `Result<T, CommandError>` 语义。
3. `CommandError` 统一字段：

```json
{
  "code": "PRECHECK_DISK_OFFLINE",
  "message": "Target disk is offline.",
  "trace_id": "tr_20260305_0001",
  "retryable": true,
  "details": {}
}
```

## 2. 命令列表（冻结）

### `scan_apps() -> Vec<AppScanResult>`

```json
{
  "app_id": "telegram-desktop",
  "display_name": "Telegram Desktop",
  "tier": "supported",
  "detected_paths": [
    {
      "path": "/Users/cola/Library/Application Support/Telegram Desktop",
      "exists": true,
      "is_symlink": false,
      "size_bytes": 2147483648
    }
  ],
  "running": false,
  "last_verified_at": "2026-03-05T10:00:00Z"
}
```

### `get_disk_status() -> Vec<DiskStatus>`

```json
{
  "mount_point": "/Volumes/ExternalSSD",
  "display_name": "ExternalSSD",
  "is_mounted": true,
  "is_writable": true,
  "free_bytes": 512000000000,
  "total_bytes": 1000000000000
}
```

### `migrate_app(req: MigrateRequest) -> RelocationResult`

`MigrateRequest`:

```json
{
  "app_id": "telegram-desktop",
  "target_root": "/Volumes/ExternalSSD",
  "mode": "bootstrap",
  "allow_experimental": false
}
```

`RelocationResult`:

```json
{
  "relocation_id": "reloc_20260305_001",
  "app_id": "telegram-desktop",
  "state": "HEALTHY",
  "health_state": "healthy",
  "source_path": "/Users/cola/Library/Application Support/Telegram Desktop",
  "target_path": "/Volumes/ExternalSSD/AppData/Telegram Desktop",
  "backup_path": "/Users/cola/Library/Application Support/Telegram Desktop.bak",
  "trace_id": "tr_20260305_0001",
  "started_at": "2026-03-05T10:00:00Z",
  "updated_at": "2026-03-05T10:05:00Z"
}
```

### `rollback_relocation(req: RollbackRequest) -> RelocationResult`

`RollbackRequest`:

```json
{
  "relocation_id": "reloc_20260305_001",
  "force": false
}
```

### `export_operation_logs(req: ExportLogsRequest) -> ExportLogsResult`

`ExportLogsRequest`:

```json
{
  "relocation_id": "reloc_20260305_001",
  "trace_id": "tr_20260305_0001",
  "output_path": "/Users/cola/Library/Application Support/disk-relocator/exports/tr_20260305_0001.operation-logs.json"
}
```

`ExportLogsResult`:

```json
{
  "export_trace_id": "tr_20260305_0002",
  "relocation_id": "reloc_20260305_001",
  "trace_id": "tr_20260305_0001",
  "output_path": "/Users/cola/Library/Application Support/disk-relocator/exports/tr_20260305_0002.operation-logs.json",
  "exported_count": 42
}
```

### `list_relocations() -> Vec<RelocationSummary>`

```json
{
  "relocation_id": "reloc_20260305_001",
  "app_id": "telegram-desktop",
  "state": "HEALTHY",
  "health_state": "healthy",
  "source_path": "/Users/cola/Library/Application Support/Telegram Desktop",
  "target_path": "/Volumes/ExternalSSD/AppData/Telegram Desktop",
  "updated_at": "2026-03-05T10:05:00Z"
}
```

### `check_health() -> Vec<HealthStatus>`

```json
{
  "relocation_id": "reloc_20260305_001",
  "app_id": "telegram-desktop",
  "state": "healthy",
  "checks": [
    {
      "code": "HEALTH_RW_PROBE_FAILED",
      "ok": true,
      "message": "rw probe ok"
    }
  ],
  "observed_at": "2026-03-05T10:10:00Z"
}
```

### `list_health_events(req: HealthEventsRequest?) -> Vec<HealthEvent>`

`HealthEventsRequest`:

```json
{
  "limit": 30
}
```

`HealthEvent`:

```json
{
  "snapshot_id": "snap_20260305_001",
  "relocation_id": "reloc_20260305_001",
  "app_id": "telegram-desktop",
  "state": "degraded",
  "check_code": "HEALTH_DISK_OFFLINE",
  "message": "target disk appears offline or not mounted.",
  "observed_at": "2026-03-05T10:10:00Z"
}
```

### `reconcile_relocations(req: ReconcileRequest?) -> ReconcileResult`

`ReconcileRequest`:

```json
{
  "apply_safe_fixes": true,
  "limit": 500
}
```

`ReconcileResult`:

```json
{
  "trace_id": "tr_20260305_0009",
  "observed_at": "2026-03-05T10:12:00Z",
  "scanned": 3,
  "drift_count": 2,
  "safe_fixable_count": 1,
  "fixed_count": 1,
  "issues": [
    {
      "relocation_id": "reloc_20260305_001",
      "app_id": "telegram-desktop",
      "code": "RECON_TEMP_PATH_RESIDUE",
      "severity": "warning",
      "message": "temporary migration path residue detected.",
      "suggestion": "run safe-fix to cleanup temporary path.",
      "safe_fix_action": "cleanup_temp_path",
      "safe_fix_applied": true,
      "details": {}
    }
  ]
}
```

## 3. 状态与错误映射

1. `PRECHECK_*` 错误仅由 `migrate_app` 预检阶段返回。
2. `MIGRATE_*` 错误仅由 `migrate_app` 主流程返回。
3. `ROLLBACK_*` 错误由 `rollback_relocation` 返回。
4. `HEALTH_*` 错误用于 `check_health` 检查项与健康面板告警。

## 4. 兼容性策略

1. `tier=blocked` 的 `app_id`，`migrate_app` 必须直接返回 `PRECHECK_TIER_BLOCKED`。
2. `tier=experimental` 且 `allow_experimental=false`，必须返回 `PRECHECK_EXPERIMENTAL_NOT_CONFIRMED`。
3. `mode=bootstrap` 仅在画像声明 `allow_bootstrap_if_source_missing=true` 时允许执行。
