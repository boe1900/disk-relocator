# Release Gate

发布前门禁脚本：

```bash
bash tests/release/run-release-gate.sh
```

该脚本会执行：

1. 前端构建与 Rust 编译检查
2. 20 轮迁移 + 回滚
3. 中断恢复关键用例
4. 外接盘离线告警用例
5. 对账漂移检测与 safe-fix 用例
6. 发布文档完整性检查
