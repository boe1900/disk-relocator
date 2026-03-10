# V1 Freeze Review（2026-03-05）

> 注：本文件是 V1 冻结记录。当前运行时画像已升级到 v2.1（`availability + units` 模型），不再以 `tier/source_paths/health_checks/rollback_rules` 作为主结构。

## 评审范围

1. 画像分级与结构字段：`app-profiles.json`
2. 错误码字典：`error-codes.md`
3. 状态流转：`state-machine.md`

## 评审结论

1. 覆盖主线完整：
   - 首次引导模式（source 缺失时 bootstrap）
   - 已有数据迁移模式（复制/校验/切换）
   - 回滚（自动 + 手动入口语义）
   - 健康监控（healthy/degraded/broken）
2. 满足 V1 最小要求：
   - 画像分级包含 `supported/experimental/blocked`
   - `supported` 至少 3 项（Telegram、JetBrains Caches、Xcode DerivedData）
   - 每个可迁移画像均含 `process_names/source_paths/health_checks/rollback_rules`
3. 可作为下一步输入：
   - SQLite schema 设计
   - Tauri 命令 DTO 固化
   - 前端状态映射与错误文案映射

## 待后续验证（非冻结阻断项）

1. `xcode-derived-data` 实机兼容性需进入测试矩阵验证。
2. `experimental` 画像的二次确认文案与交互尚未定义。
3. 错误码到 UI 文案的 i18n 表尚未建立。
