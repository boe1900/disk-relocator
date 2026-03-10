# 微信手工验证（无需 DevTools）

适用场景：微信新安装、可接受直接试迁移（不做备份）。

## 前置条件

1. 微信已退出（Dock 中无运行状态）。
2. 外接盘已挂载并可写，例如：`/Volumes/M4_Ext_SSD`。
3. 启动工具：`npm run tauri dev`。

## A. 首次引导模式（bootstrap）

1. 打开「应用搬迁」页并选择 `wechat-non-mas`。
2. 在迁移弹窗中，勾选可迁移单元 `media-and-files`。
3. `目标盘根路径` 选择 `/Volumes/M4_Ext_SSD`（或你的挂载点）。
4. 当检测为 `bootstrap` 场景时点击「开始迁移」。
5. 等待完成，确认 `state=HEALTHY` 且 `health_state=healthy`。

预期结果：
- 原数据目录变为软链接，指向外接盘目标目录。
- 「迁移状态与回滚」卡片能看到该 relocation 记录。

## B. 健康监控验证

1. 打开「健康监控」卡片，点击检测按钮（如有）。
2. 预期状态为 `healthy`。
3. 暂时拔出外接盘后再次检测，预期变为 `degraded/broken`（离线告警）。
4. 重新插回外接盘后再次检测，预期恢复为 `healthy`。

## C. 回滚验证

1. 打开「迁移状态与回滚」卡片。
2. 选择刚才的 relocation 记录，保持 `强制回滚` 勾选。
3. 点击「执行回滚」。
4. 返回结果确认 `state=ROLLED_BACK`。

预期结果：
- 微信源路径恢复为本地普通目录（不再是软链接）。
- 健康检测不再报告该 relocation 的链路异常。

## D. 已有数据迁移模式（migrate）补充

如果后续你想验证“已有微信数据”路径：
1. 先确保 `FileStorage` 下有真实数据；
2. 在迁移弹窗保持勾选 `media-and-files` 并执行迁移；
3. 执行后检查 `HEALTHY`，然后按上面的 C 步骤验证回滚。
