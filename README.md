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

## App Profiles 策略

当前版本仅使用内置 `specs/v1/app-profiles.json`，不支持远程自动更新。  
新增 app 画像的标准流程是：修改内置 profile 并发布新版本。

### 新增 App 画像流程

1. 修改 `specs/v1/app-profiles.json`。
2. 本地执行质量检查：
   - `npm run check:frontend`
   - `npm run check:rust`
   - `npm run check:release`
3. 更新版本号（需保持三处一致）：
   - `package.json`
   - `src-tauri/Cargo.toml`
   - `src-tauri/tauri.conf.json`
4. 提交并发布新版本（见下方发布流程）。

## 发布流程（GitHub Release）

- 触发方式 A（推荐）：推送语义化 tag（`vX.Y.Z`），自动触发 `.github/workflows/release.yml`。
- 触发方式 B：在 GitHub Actions 手动运行 `Release` 工作流（`workflow_dispatch`）。

发布工作流会自动执行 `npm run check:release`，并上传：
- `.dmg` 安装包
- `app-profiles.json`（与该版本二进制绑定）

## 使用流程（用户视角）

1. 打开应用并点击「刷新扫描」。
2. 在「应用列表」选择可迁移应用，点击「搬迁外存」。
3. 选择目标外接盘与目标路径，确认执行。
4. 迁移完成后在「健康检查」确认状态为健康。
5. 需要恢复时，在对应记录执行「回滚」。

## 安装与首次打开（当前未签名版本）

由于当前未使用 Apple Developer 签名/公证，macOS 可能拦截首次打开。按以下步骤操作即可使用。

1. 下载并挂载 `.dmg`，将 `Disk Relocator.app` 拖到 `/Applications`。
2. 首次打开时，先在「应用程序」中右键应用，选择「打开」。
3. 若仍被拦截，到「系统设置 -> 隐私与安全性」，在底部点击「仍要打开」。

如果系统依旧阻止启动，可在终端执行：

```bash
xattr -dr com.apple.quarantine /Applications/"Disk Relocator.app"
open -a "Disk Relocator"
```

注意：每次下载的新版本如果再次被标记隔离，可能需要重复一次上述步骤。
