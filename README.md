# Disk Relocator 文档

本目录是一个基于 Tauri + Rust 的 macOS 桌面工具方案文档，目标是将应用大体积数据从内置磁盘迁移到外接磁盘，并可视化管理软链接状态。

## 文档索引

1. [01-feasibility-and-boundary.md](./01-feasibility-and-boundary.md)
2. [02-compatibility-tier-list.md](./02-compatibility-tier-list.md)
3. [03-migration-and-rollback-flow.md](./03-migration-and-rollback-flow.md)
4. [04-mvp-spec-and-acceptance.md](./04-mvp-spec-and-acceptance.md)
5. [05-risk-and-test-plan.md](./05-risk-and-test-plan.md)
6. [docs/release-test-matrix.md](./docs/release-test-matrix.md)
7. [docs/release-known-limitations.md](./docs/release-known-limitations.md)
8. [docs/rollback-runbook.md](./docs/rollback-runbook.md)
9. [docs/faq.md](./docs/faq.md)
10. [docs/validation-wechat.md](./docs/validation-wechat.md)
11. [docs/health-fix-and-rollback-guide.md](./docs/health-fix-and-rollback-guide.md)
12. [docs/github-release-workflow.md](./docs/github-release-workflow.md)

## V1 冻结规范（执行输入）

1. [specs/v1/app-profiles.json](./specs/v1/app-profiles.json)
2. [specs/v1/error-codes.md](./specs/v1/error-codes.md)
3. [specs/v1/state-machine.md](./specs/v1/state-machine.md)
4. [specs/v1/freeze-review-2026-03-05.md](./specs/v1/freeze-review-2026-03-05.md)
5. [specs/v1/sqlite-schema.sql](./specs/v1/sqlite-schema.sql)
6. [specs/v1/tauri-command-contract.md](./specs/v1/tauri-command-contract.md)

## 一页结论

- 方向可行，且在 256GB Mac 用户群体里有明确需求。
- 成败关键不是 `mv + ln -s` 命令本身，而是兼容性边界和恢复机制。
- V1 必须采用兼容性分级：
  - `Supported`
  - `Experimental`
  - `Blocked`
- 产品核心价值是“安全可控”：
  - 迁移前预检
  - 事务化迁移
  - 健康监控
  - 一键回滚

## 建议开发顺序

1. 锁定兼容性清单与分级规则。
2. 先实现迁移事务和回滚内核。
3. 再接入磁盘在线检测和健康面板。
4. 最后用最小化 Vue 界面接 Tauri 命令。

## Scaffold 启动与校验

1. 安装依赖：`npm install`
2. 前端质量检查：`npm run check:frontend`
3. Rust 质量检查（fmt + clippy + test）：`npm run check:rust`
4. 一键 smoke：`bash tests/acceptance/run-smoke.sh`
5. 发布门禁：`npm run check:release`
6. GitHub 自动发布：见 [docs/github-release-workflow.md](./docs/github-release-workflow.md)

当前 scaffold 状态：
- 9 个 Tauri 命令已接入 SQLite 元数据读写（`relocations / operation_logs / health_snapshots`）。
- `migrate_app(mode=bootstrap)` 已执行真实“创建目标目录 + 建立软链接 + 后检失败回滚清理”。
- `migrate_app(mode=migrate)` 已执行真实事务流程（复制 -> 校验 -> 切换 -> 后检，失败自动回滚清理）。
- `rollback_relocation` 已执行真实回滚流程（删链接、恢复 `.bak` 或恢复 bootstrap 源路径、清理残留临时目录）。
- 应用启动时会自动扫描 unfinished 迁移记录并恢复到 `HEALTHY` 或 `ROLLED_BACK`，失败则标记 `ROLLBACK_FAILED`。
- `migrate_app` 已接入真实预检步骤（进程运行态、源路径权限/类型、目标盘在线可写、空间余量）并写入步骤日志。
- 操作记录页可直接查看迁移/回滚结果，失败时可展开关键步骤日志。
- `check_health` 已改为实时健康评估（断链/离线/只读区分），并落盘 `health_snapshots` 与健康状态。
- 启动后自动运行健康监控器（30s 轮询 + `/Volumes` 挂载事件触发）。
- 健康面板已支持“异常状态 -> 可执行恢复指引”（重新检测 / 一键回滚）并展示历史健康事件。
- 已提供 `reconcile_relocations` 对账能力（漂移扫描 + safe-fix 无损自动纠偏）。
- 启动后自动运行对账监控器（周期扫描并自动纠偏元数据 vs 文件系统漂移）。

## 直接试用（无需 DevTools）

1. 启动应用：`npm run tauri dev`
2. 在「迁移执行」卡片中直接选择应用、目标盘和模式，点击「执行迁移」。
3. 在「迁移状态与回滚」卡片中选择迁移记录，点击「执行回滚」。
4. 微信手工验证流程见：[docs/validation-wechat.md](./docs/validation-wechat.md)。
