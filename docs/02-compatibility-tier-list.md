# 兼容性分级清单

## 1. 分级定义

- `Supported`：
  - 在测试矩阵中迁移成功率稳定
  - 迁移后应用功能正常
  - 回滚流程已验证
- `Experimental`：
  - 部分系统版本/安装来源可用
  - 需要额外风险提示和二次确认
- `Blocked`：
  - 已知高风险或平台策略冲突
  - 默认禁止迁移入口

## 2. V1 初始候选矩阵（草案）

| 应用/数据类型 | 典型路径 | 分级 | 说明 |
|---|---|---|---|
| 微信（非 MAS）数据 | `~/Library/Containers/com.tencent.xinWeChat` 或应用私有目录 | Experimental | 容器路径行为受版本与分发来源影响大 |
| Telegram（非 MAS） | `~/Library/Application Support/Telegram Desktop` | Supported（候选） | 路径明确，需保证迁移前完全退出 |
| JetBrains 缓存 | `~/Library/Caches/JetBrains/*` | Supported（候选） | 业务风险低，回滚简单 |
| Docker Desktop 镜像/VM 数据 | Docker 自管目录 | Blocked（软链接模式） | 应改为 Docker 原生 data-root 配置方案 |
| Apple 系统应用数据 | 受保护路径 | Blocked | SIP/系统保护导致风险高 |
| MAS 沙盒应用容器 | `~/Library/Containers/*` | Experimental 或 Blocked | 常见路径和权限校验约束严格 |

说明：
- 最终分级必须来自可复现测试结果，不靠经验判断。
- V1 优先上线高确定性的 `Supported` 项目。

## 3. 应用画像（Profile）检测要素

每个应用画像至少应定义：

1. 运行进程名（用于运行态拦截）
2. 源路径候选与优先级
3. 关键锁文件或高波动文件规则
4. 迁移后健康检查规则
5. 回滚规则

## 4. 用户侧透明策略

界面必须固定展示：

1. 当前分级
2. 风险摘要
3. 迁移前置条件
4. 已测试的应用版本范围
5. 最近验证日期

## 5. 兼容性元数据示例

```json
{
  "app_id": "telegram-desktop",
  "display_name": "Telegram Desktop",
  "tier": "supported",
  "source_paths": [
    "~/Library/Application Support/Telegram Desktop"
  ],
  "process_names": ["Telegram"],
  "health_checks": [
    {"type": "path_exists", "path": "~/Library/Application Support/Telegram Desktop"},
    {"type": "is_symlink", "path": "~/Library/Application Support/Telegram Desktop"}
  ],
  "notes": "迁移前必须关闭应用。"
}
```
