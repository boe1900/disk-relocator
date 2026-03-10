# MVP 规格与验收标准

## 1. 当前范围

包含：
1. 扫描预定义应用画像
2. 展示应用数据体积
3. 将选中应用数据迁移到外接盘
4. 创建并管理软链接记录
5. 提供健康面板与一键回滚

不包含：
1. 通用“任意应用自动适配”迁移
2. Docker data-root 自动重配置
3. 云端同步与跨设备画像共享

## 2. 功能需求

- `FR-001` 应用扫描：
  - 列出已支持画像并识别本地路径
  - 展示预估占用体积
- `FR-002` 迁移前预检：
  - 运行进程检查
  - 权限检查
  - 外接盘在线与可用空间检查
- `FR-003` 迁移执行：
  - 按事务化流程执行复制/校验/切换
  - 落盘迁移元数据
- `FR-004` 健康监控：
  - 识别断链与外接盘离线
  - 在状态面板中告警
- `FR-005` 回滚恢复：
  - 恢复原始路径
  - 清理链接并更新元数据

## 3. Tauri 命令接口草案

```rust
#[tauri::command]
fn scan_apps() -> Result<Vec<AppScanResult>, String>;

#[tauri::command]
fn get_disk_status() -> Result<Vec<DiskStatus>, String>;

#[tauri::command]
fn migrate_app(req: MigrateRequest) -> Result<RelocationResult, String>;

#[tauri::command]
fn rollback_relocation(req: RollbackRequest) -> Result<RelocationResult, String>;

#[tauri::command]
fn list_relocations() -> Result<Vec<RelocationRecord>, String>;

#[tauri::command]
fn check_health() -> Result<Vec<HealthStatus>, String>;
```

## 4. 最小交互要求

必须具备的页面：
1. 应用列表页
2. 迁移确认弹窗（含风险提示）
3. 迁移状态页
4. 健康告警面板

必须具备的文案类型：
1. 为什么当前被拦截
2. 下一步如何处理后重试
3. 回滚会恢复什么、不会恢复什么

## 5. 非功能需求

1. 正常与中断场景均不丢数据
2. 操作日志结构化、可导出
3. 异常退出后元数据可恢复
4. 文件操作具备幂等或防重保护

## 6. 验收门槛

1. 对每个 `availability=active` 应用画像：
  - 至少 20 轮“迁移 + 回滚”成功，且无数据损坏
2. 强制中断测试：
  - 在迁移每一步中断后都能恢复到一致状态
3. 外接盘离线测试：
  - 在一个健康检测周期内提示异常并给出恢复引导
4. 日志可追溯测试：
  - 每次迁移都有完整 trace id 与状态流转记录
