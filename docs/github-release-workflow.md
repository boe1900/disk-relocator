# GitHub Release 发布流程

本文描述仓库内 `Release` GitHub Actions 工作流的使用方式。  
当前目标是自动发布 **macOS Apple Silicon (`aarch64`) + Intel (`x86_64`) 的 `.dmg` 安装包**，并同步发布 `app-profiles.json`。

## 发布模式（免费/付费）

Release workflow 支持两种模式：

1. 免费模式（默认）：
   - 不做 Apple Developer 签名与公证。
   - 可正常产出 `.dmg`，但用户安装会触发 Gatekeeper 提示（需右键打开或去隔离属性）。
2. 付费模式（开关开启）：
   - 执行 `Developer ID` 签名 + Apple Notarization（公证）。
   - 更适合公开分发。

## 付费模式开关

你可以用任一方式开启付费模式：

1. 手动触发 `workflow_dispatch` 时，设置 `sign_and_notarize=true`。
2. 仓库变量 `RELEASE_SIGN_AND_NOTARIZE=true`（用于 tag push 自动发布）。

未开启时即为免费模式。

## 付费模式所需 Secrets

仅在 `sign_and_notarize=true` 时需要配置；否则不需要。

1. 必填（签名）：
   - `APPLE_CERTIFICATE`：`Developer ID Application` 证书导出的 `.p12` 内容（base64）
   - `APPLE_CERTIFICATE_PASSWORD`：`.p12` 密码
2. 可选但推荐（签名身份）：
   - `APPLE_SIGNING_IDENTITY`：例如 `Developer ID Application: Your Name (TEAMID1234)`
3. 公证（二选一）：
   - API Key 模式（推荐 CI）：`APPLE_API_KEY` + `APPLE_API_ISSUER` + `APPLE_API_KEY_P8`
   - Apple ID 模式：`APPLE_ID` + `APPLE_PASSWORD` + `APPLE_TEAM_ID`

注意：如果两套公证凭据都配置，workflow 会优先使用 API Key 模式。

## 触发方式

1. 自动触发：推送符合 `v*` 的 tag（例如 `v0.1.0`）。
2. 手动触发：在 GitHub Actions 页面运行 `Release` workflow，并填写：
   - `tag_name`（必填，例：`v0.1.0`）
   - `release_draft`（是否草稿）
   - `prerelease`（是否预发布）
   - `sign_and_notarize`（是否开启签名与公证，默认 `false`）

## 发布前校验（工作流内自动执行）

1. 版本一致性校验：
   - `package.json` 的 `version`
   - `src-tauri/tauri.conf.json` 的 `version`
   - `src-tauri/Cargo.toml` 的 `version`
   - 以及 tag 必须等于 `v<version>`
2. 发布门禁：`npm run check:release`
3. 打包目标：
   - `--target aarch64-apple-darwin --bundles dmg`
   - `--target x86_64-apple-darwin --bundles dmg`

如果上述任一步骤失败，Release 不会发布。

## Release 产物

工作流会通过 `tauri-apps/tauri-action` 自动上传双架构 `.dmg` 到 GitHub Release。  
同时会把 `specs/v1/app-profiles.json` 作为 `app-profiles.json` 资产上传到同一个 Release，供客户端按固定 URL 拉取。  
同时会把下列目录中的 `.dmg` 作为 workflow artifact 保存，便于排障和留档：
- `src-tauri/target/aarch64-apple-darwin/release/bundle/dmg/*.dmg`
- `src-tauri/target/x86_64-apple-darwin/release/bundle/dmg/*.dmg`

## 推荐操作顺序

1. 本地更新版本号（三处保持一致）：
   - `package.json`
   - `src-tauri/tauri.conf.json`
   - `src-tauri/Cargo.toml`
2. 本地先跑一次：
   - `npm run check:frontend`
   - `npm run check:rust`
   - `npm run check:release`
3. 提交并推送主分支。
4. 创建并推送 tag：
   - `git tag v0.1.0`
   - `git push origin v0.1.0`
5. 到 GitHub Actions 查看 `Release` workflow 执行状态，确认 Release 页面资产已生成。

## 常见失败原因

1. 版本号不同步：tag 与三处 version 不一致。
2. 目标架构错误：应产出 Apple Silicon + Intel 双架构资产，缺失任一架构都属于异常。
3. 门禁失败：`check:release` 任何一步失败都会阻断发布。
4. 付费模式开启但 secrets 缺失或不完整：workflow 会在打包前直接报错退出。
