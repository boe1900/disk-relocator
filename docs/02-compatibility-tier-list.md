# 兼容性与风险清单（Unit 模型）

## 1. 模型定义

- `profile.availability`
  - `active`: 可进入迁移流程
  - `blocked`: 禁止迁移
  - `deprecated`: 默认不允许新迁移，仅保留历史识别
- `unit.risk_level`
  - `stable`: 默认无需额外确认
  - `cautious`: 建议提示风险，可按产品策略要求确认
  - `high`: 必须强提醒 + 倒计时 + 二次确认

## 2. 当前策略（2026-03-16）

| 应用/数据类型 | 迁移单元（unit） | 处理策略 | 说明 |
|---|---|---|---|
| WeChat（非 MAS）核心目录 | `wechat-core-xwechat-files` | `active` + `high` | 整体迁移 `xwechat_files`，迁移前弹出高风险红色警告并强制倒计时确认 |

## 3. 画像最小要素

每个 profile 至少包含：

1. `app_id`、`display_name`
2. `process_names`（运行态拦截）
3. `units[]`（每个 unit 独立定义 `unit_id/source_path/target_path_template`）

## 4. 前端透明信息

界面应固定展示：

1. 可迁移的 unit 列表（支持多选）
2. 每个 unit 的风险/阻断原因
3. 迁移前置条件（进程退出、磁盘可写等）

## 5. 示例

```json
{
  "app_id": "wechat-non-mas",
  "display_name": "WeChat (Non-MAS)",
  "availability": "active",
  "process_names": ["WeChat"],
  "units": [
    {
      "unit_id": "wechat-core-xwechat-files",
      "display_name": "微信核心目录 (xwechat_files)",
      "category": "app-data",
      "source_path": "~/Library/Containers/com.tencent.xinWeChat/Data/Documents/xwechat_files",
      "target_path_template": "{target_root}/AppData/WeChat/xwechat_files",
      "risk_level": "high"
    }
  ]
}
```
