# Disk Relocator

把聊天软件等大目录搬到外接盘，释放 macOS 系统盘空间；需要时可恢复回系统盘。

## 背景定位（先看这个）

这个工具主要面向：

- 丐版 Mac mini / 小容量 Mac（系统盘长期紧张）
- 外接 SSD 长期挂载在同一台机器上使用
- 希望把微信、Telegram、钉钉、飞书、Discord 等目录迁到外盘

不建议用于：

- 经常拔插硬盘、经常移动办公的场景
- 需要“随时脱离外接盘也完全不受影响”的场景

一句话：**更适合固定工位、长期外挂盘，不适合高频移动场景。**

## 使用前准备

1. 准备一个稳定的外接 SSD（建议 APFS）。
2. 迁移前先关闭目标应用（尤其是微信）。
3. 迁移过程中不要拔盘，使用中也尽量保持外盘已挂载。

## 5 分钟上手

1. 打开应用，点击`刷新扫描`。
2. 在`应用列表`里找到目标应用，点击`搬迁外存`。
3. 在弹窗里确认迁移目录、选择目标外接盘，点击`开始迁移`。
4. 迁移后到`健康检查`确认状态正常。
5. 需要恢复时，在应用卡片点击`恢复到系统`。

## 当前支持的应用

- WeChat（Non-MAS）
- Telegram（macOS 原生版）
- 钉钉（DingTalk）
- 飞书（Feishu / Lark）
- Discord

说明：实际可用应用列表以线上发布的 `app-profiles.json` 为准。
如需新增应用支持，欢迎在 [Issues](https://github.com/boe1900/disk-relocator/issues) 提交应用名称、版本号、数据目录路径与使用场景。

## 微信特别说明（务必执行）

微信迁移完成后：

1. 先彻底退出微信（`Command + Q`）
2. 在终端执行：

```bash
sudo codesign --sign - --force --deep /Applications/WeChat.app
```

微信每次升级后，建议再执行一次上面命令，避免微信在 `app_data` 下重新初始化 `xwechat_files` 导致历史记录不可见。

### 微信截图权限重置（重点）

为了让微信能够突破苹果极其严格的沙盒限制，稳定运行在外接硬盘上，Data Dock 在底层会对微信的系统安全签名进行重构。  
这会触发 macOS 隐私保护机制，系统可能重置微信的截图权限（屏幕录制权限）。

修复只需约 1 分钟：

1. 打开 `系统设置 -> 隐私与安全性 -> 屏幕录制（Screen Recording）`。
2. 在右侧列表中找到`微信（WeChat）`。
3. **关键一步：不要只是关掉再打开。请选中微信，点击列表下方的 `-`（减号）把它彻底删掉。**
4. 打开微信，随便触发一次截图（按截图快捷键，或点击聊天框的剪刀图标）。
5. 系统会重新弹窗提示“微信想录制您的屏幕”，点击去设置里重新授权，并把微信开关打开即可。

## 截图预览

应用列表（扫描与迁移入口）：

![应用列表](docs/screenshots/app-list.png)

迁移弹窗（选择目标盘并确认迁移单元）：

![迁移弹窗](docs/screenshots/migration-dialog.png)

微信高风险提醒（迁移前）：

![微信高风险提醒](docs/screenshots/wechat-risk-warning.png)

健康检查（查看挂载与软链接状态）：

![健康检查](docs/screenshots/health-check.png)

操作记录（查看迁移/回滚结果）：

![操作记录](docs/screenshots/operation-log.png)

## 下载与安装

1. 在 [Releases](https://github.com/boe1900/disk-relocator/releases) 下载最新 `.dmg`。
2. 拖动 `Disk Relocator.app` 到 `/Applications`。
3. 首次打开如被拦截：右键应用 -> `打开`。

如果仍被阻止，可执行：

```bash
xattr -dr com.apple.quarantine /Applications/"Disk Relocator.app"
open -a "Disk Relocator"
```

## 常见问题

1. 迁移后应用打不开或数据异常怎么办？
   - 先确认外接盘已挂载；
   - 到`健康检查`查看异常；
   - 必要时执行`恢复到系统`。
2. 外接盘临时断开会怎样？
   - 软链接目标不可用时，应用可能异常；
   - 重新挂载后可恢复，或直接回滚到系统盘。
3. 这个工具是否适合笔记本经常移动场景？
   - 不建议。它更适合固定工位 + 长期外接盘。

## 文档

- [FAQ](docs/faq.md)
- [健康检查、修复与回滚指南](docs/health-fix-and-rollback-guide.md)
- [微信验证说明](docs/validation-wechat.md)
