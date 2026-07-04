# Claude Sonnet 5 支持说明

Kiro 于 2026-06-30 上线 Claude Sonnet 5（Experimental）。本代理支持基础模型映射、`/v1/models` 列表和 `-thinking` 后缀的 adaptive thinking 配置。

## 上游行为

参考：[What's new in Claude Sonnet 5](https://platform.claude.com/docs/en/about-claude/models/whats-new-sonnet-5)、[Kiro Models](https://kiro.dev/docs/models/)

| 属性 | 值 |
|------|-----|
| API / Kiro 模型 ID | `claude-sonnet-5` |
| 上下文窗口 | 1M |
| 区域 | 仅 `us-east-1`（实验性） |
| 积分倍率 | 1.3x |

## 当前实现

| 场景 | 行为 |
|------|------|
| `claude-sonnet-5` | 映射为 `claude-sonnet-5` |
| `claude-sonnet-5-thinking` | 覆写为 `adaptive` thinking，并设置 `output_config.effort = "high"` |
| `claude-sonnet-5-agentic` | 映射为 `claude-sonnet-5` |
| 上下文窗口 | `get_context_window_size` 返回 `1_000_000` |

## Thinking 行为

| 请求配置 | Sonnet 5 行为 |
|----------|---------------|
| 不传 `thinking` | 上游默认开启 adaptive thinking |
| `thinking: { "type": "adaptive" }` | 推荐方式，可配合 `output_config.effort` |
| `thinking: { "type": "enabled", "budget_tokens": N }` | 上游不支持 manual extended thinking |
| `thinking: { "type": "disabled" }` | 关闭 thinking |

## 使用注意

- 配置 `region` / `apiRegion` 为 `us-east-1`。
- Sonnet 5 使用 1M 上下文窗口。
- `claude-sonnet-5-thinking` 默认使用 high effort。
