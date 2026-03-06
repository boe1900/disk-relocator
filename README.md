# Disk Relocator

Disk Relocator 是一个基于 Vue 3 + Tauri + Rust 的 macOS 桌面工具，用于把大体积应用数据从系统盘迁移到外接盘，并通过软链接保持应用继续可用。

## 适用范围

- 平台：macOS（当前以 Apple Silicon 为目标）
- 典型场景：微信等应用数据占用系统盘空间，需要迁移到外接 SSD

## 核心能力

- 应用数据目录扫描（基于 profile + 文件系统实际检测）
- 一键迁移（真实文件操作，不是纯 Mock）
- 软链接切换与持久化记录
- 一键回滚
- 健康检查（挂载状态、链接可用性）
- 操作日志（用户可读的迁移/回滚结果）

## 快速开始（开发）

1. 安装依赖：
   - `npm install`
2. 启动桌面开发模式：
   - `npm run tauri dev`
3. 质量检查：
   - `npm run check:frontend`
   - `npm run check:rust`
   - `npm run check:release`

## 使用流程（用户视角）

1. 打开应用并点击「刷新扫描」。
2. 在「应用列表」选择可迁移应用，点击「搬迁外存」。
3. 选择目标外接盘与目标路径，确认执行。
4. 迁移完成后在「健康检查」确认状态为健康。
5. 需要恢复时，在对应记录执行「回滚」。

## Release 与安装说明

### 免费发布模式（默认）

- GitHub Actions `Release` 默认不做 Apple 签名/公证，可直接生成 `.dmg`。
- 用户安装可能遇到 Gatekeeper 提示，需要右键打开或移除隔离属性。

### 付费签名公证模式（开关）

- 在 `Release` workflow 中设 `sign_and_notarize=true`，或仓库变量 `RELEASE_SIGN_AND_NOTARIZE=true`。
- 需配置 Apple Developer 相关 secrets（证书 + 公证凭据）。

详细见：[docs/github-release-workflow.md](./docs/github-release-workflow.md)

## 文档索引

- 方案与边界：[docs/01-feasibility-and-boundary.md](./docs/01-feasibility-and-boundary.md)
- 兼容性分级：[docs/02-compatibility-tier-list.md](./docs/02-compatibility-tier-list.md)
- 迁移回滚流程：[docs/03-migration-and-rollback-flow.md](./docs/03-migration-and-rollback-flow.md)
- MVP 验收：[docs/04-mvp-spec-and-acceptance.md](./docs/04-mvp-spec-and-acceptance.md)
- 风险与测试计划：[docs/05-risk-and-test-plan.md](./docs/05-risk-and-test-plan.md)
- 三阶段路线图：[docs/ROADMAP.md](./docs/ROADMAP.md)
- 任务清单：[docs/TODO.md](./docs/TODO.md)
- 微信手工验证：[docs/validation-wechat.md](./docs/validation-wechat.md)
- 健康修复与回滚说明：[docs/health-fix-and-rollback-guide.md](./docs/health-fix-and-rollback-guide.md)
- 发布流程：[docs/github-release-workflow.md](./docs/github-release-workflow.md)
