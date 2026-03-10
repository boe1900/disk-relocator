# Error Codes

本文件冻结当前可见错误码，约束前后端和日志字段，避免后续语义漂移。

## 字段约定

- `code`：稳定标识，不随文案变化。
- `stage`：`precheck` / `migration` / `rollback` / `health`。
- `retryable`：是否可直接重试。
- `user_action`：用户可执行的恢复动作。

## 预检（precheck）

| code | 含义 | retryable | user_action |
|---|---|---:|---|
| `PRECHECK_PROCESS_RUNNING` | 应用仍在运行，禁止迁移 | 是 | 退出应用后重试 |
| `PRECHECK_PERMISSION_DENIED` | 缺少 Full Disk Access 或目标路径权限 | 否 | 按引导授权后重试 |
| `PRECHECK_DISK_OFFLINE` | 目标外接盘未挂载 | 是 | 挂载磁盘后重试 |
| `PRECHECK_DISK_READONLY` | 目标磁盘只读 | 视情况 | 解除只读或更换磁盘 |
| `PRECHECK_INSUFFICIENT_SPACE` | 目标空间不足（含安全余量） | 是 | 清理空间或更换磁盘 |
| `PRECHECK_SOURCE_NOT_FOUND` | 迁移模式下源路径不存在 | 否 | 切换到首次引导模式或检查路径 |
| `PRECHECK_SOURCE_IS_SYMLINK` | 源路径已是软链接 | 否 | 执行对账/修复流程 |
| `PRECHECK_APP_BLOCKED` | 当前画像 `availability` 为 `blocked/deprecated` | 否 | 不允许迁移 |
| `PRECHECK_UNIT_BLOCKED` | 选中的迁移单元被禁用或被阻断 | 否 | 调整单元选择或配置 |
| `PRECHECK_UNIT_CONFIRMATION_REQUIRED` | 选中单元要求确认但用户未确认 | 是 | 完成风险确认后重试 |

## 迁移（migration）

| code | 含义 | retryable | user_action |
|---|---|---:|---|
| `MIGRATE_COPY_FAILED` | 复制阶段失败 | 是 | 查看日志后重试 |
| `MIGRATE_VERIFY_SIZE_MISMATCH` | 体积校验不一致 | 否 | 执行自动回滚并重试 |
| `MIGRATE_VERIFY_CHECKSUM_MISMATCH` | 校验和不一致 | 否 | 执行自动回滚并重试 |
| `MIGRATE_SWITCH_RENAME_FAILED` | 源目录改名为 `.bak` 失败 | 是 | 检查占用与权限后重试 |
| `MIGRATE_SWITCH_SYMLINK_FAILED` | 创建软链接失败 | 是 | 自动回滚后重试 |
| `MIGRATE_POSTCHECK_FAILED` | 后检失败（链接或读写探针失败） | 否 | 自动回滚，进入修复指引 |
| `MIGRATE_INTERRUPTED` | 迁移流程被中断（崩溃/断电/强杀） | 视情况 | 重启后进入恢复流程 |
| `MIGRATE_CLEANUP_FAILED` | 清理备份或临时目录失败 | 是 | 允许保留并稍后清理 |

## 回滚（rollback）

| code | 含义 | retryable | user_action |
|---|---|---:|---|
| `ROLLBACK_REMOVE_SYMLINK_FAILED` | 删除软链接失败 | 是 | 检查文件占用后重试 |
| `ROLLBACK_RESTORE_BACKUP_FAILED` | 备份目录恢复失败 | 否 | 进入手动恢复引导 |
| `ROLLBACK_CLEANUP_TEMP_FAILED` | 回滚后临时目录清理失败 | 是 | 可稍后重试清理 |
| `ROLLBACK_METADATA_RESTORE_FAILED` | 元数据状态回退失败 | 是 | 启动对账任务修复 |

## 健康（health）

| code | 含义 | state | user_action |
|---|---|---|---|
| `HEALTH_SYMLINK_MISSING` | 源路径软链接缺失 | `broken` | 执行对账或回滚 |
| `HEALTH_DISK_OFFLINE` | 目标外接盘未挂载或离线 | `degraded` | 挂载外接盘后重试健康检查 |
| `HEALTH_TARGET_MISSING` | 软链接目标不存在 | `broken` | 挂载磁盘或修复目标路径 |
| `HEALTH_TARGET_READONLY` | 目标路径只读 | `degraded` | 解除只读或迁回内置盘 |
| `HEALTH_RW_PROBE_FAILED` | 目标读写探针失败 | `degraded` | 按指引检查权限/磁盘状态 |
| `HEALTH_METADATA_DRIFT` | 元数据与文件系统不一致 | `degraded` | 运行对账修复 |

## 日志导出（log-export）

| code | 含义 | retryable | user_action |
|---|---|---:|---|
| `LOG_EXPORT_WRITE_FAILED` | 导出文件写入失败（目录创建/序列化/落盘） | 是 | 检查目标路径权限或改用默认导出路径 |

## 对账（reconcile）

| code | 含义 | severity | user_action |
|---|---|---|---|
| `RECON_SOURCE_MISSING` | 对账时源路径缺失 | `critical` | 执行回滚恢复源路径并复查 |
| `RECON_EXPECTED_SYMLINK_MISSING` | 元数据为活跃迁移但源路径不是软链接 | `critical` | 执行回滚并重新评估迁移 |
| `RECON_SOURCE_TARGET_MISMATCH` | 源软链接目标与元数据目标不一致 | `warning` | 回滚后重新迁移，避免漂移扩大 |
| `RECON_TARGET_MISSING` | 源软链接存在但目标路径缺失 | `critical` | 挂载外接盘或执行回滚 |
| `RECON_TEMP_PATH_RESIDUE` | 临时目录残留 | `warning` | 执行 safe-fix 清理 |
| `RECON_STATE_STALE_ACTIVE` | 文件系统已是活跃迁移但元数据状态落后 | `warning` | 执行 safe-fix 同步状态 |
| `RECON_STATE_STALE_ROLLED_BACK` | 文件系统已回滚但元数据仍是活跃状态 | `warning` | 执行 safe-fix 同步状态 |
| `RECON_BACKUP_PATH_RESIDUE` | 备份路径残留 | `warning` | 确认无用后手动清理 |
| `RECON_TARGET_RESIDUE_AFTER_ROLLBACK` | 回滚态下目标路径残留 | `warning` | 确认后手动清理 |
| `RECONCILE_RUN_FAILED` | 对账任务执行失败 | `critical` | 查看日志后重试对账 |
