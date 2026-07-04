# 套餐模型能力表研究

日期：2026-07-04

状态：研究结论，不包含业务实现。

Draft PR：https://github.com/gaozuo/kiro.rs/pull/1

## 目标

为后续实现「按 Kiro 套餐过滤模型」建立可核验依据：

- 不拍脑袋维护模型权限。
- 先确认 fork 链和上游 PR/issue 是否已有可复用的套餐模型配置表。
- 找到权威套餐与模型矩阵后，再设计 `/v1/models` 和请求路由过滤。
- 未知或无法识别的套餐不参与模型能力判断；即未知套餐返回 0 个可用模型，也不能被选中处理模型请求。

## 当前项目事实

当前 `/v1/models` 在 `src/anthropic/handlers.rs` 中硬编码返回全量模型，不按凭据套餐过滤。

凭据模型已经有 `subscription_title` 字段，来源是 Kiro 用量/余额接口返回的订阅标题。现有 `supports_opus()` 只判断 Free 是否可用 Opus，且 `subscription_title == None` 时会暂时放行。这不满足本次要求，因为未知套餐不能识别时应过滤。

当前请求路由从 `KiroProvider::call_api_with_retry()` 调用 `token_manager.acquire_context_for_user_excluding(user_id, failed_ids)`，没有把请求模型传入选择器。因此即使 `/v1/models` 改成动态并集，实际请求仍可能被路由到不支持该模型的账号。

当前 Anthropic -> Kiro 模型映射只覆盖：

- Claude Sonnet 5
- Claude Sonnet 4.6
- Claude Sonnet 4.5
- Claude Opus 4.5
- Claude Opus 4.6
- Claude Opus 4.7
- Claude Opus 4.8
- Claude Haiku 4.5

官方表里的 Auto、Claude Sonnet 4.0、DeepSeek、MiniMax、GLM、Qwen3 Coder Next 当前没有本代理完整映射和 `/v1/models` 暴露支持，不能仅因为官方套餐支持就加入本代理返回值。

## fork 链扫描

扫描链路：

`gaozuo/kiro.rs -> Foxfishc/kiro.rs -> M-JYuan/kiro.rs -> BenedictKing/kiro.rs -> hank9999/kiro.rs`

本地 mirror：

- `/tmp/kiro-scan/gaozuo__kiro.rs.git`
- `/tmp/kiro-scan/Foxfishc__kiro.rs.git`
- `/tmp/kiro-scan/M-JYuan__kiro.rs.git`
- `/tmp/kiro-scan/BenedictKing__kiro.rs.git`
- `/tmp/kiro-scan/hank9999__kiro.rs.git`

关键词扫描：

- `subscription_title`
- `supports_opus`
- `enabledModels`
- `enabled_models`
- `availableModels`
- `supported_models`
- `subscription.*model`
- `model.*subscription`
- `KIRO FREE`
- `KIRO PRO`

结论：

- 没有找到完整的 `subscription -> model IDs` 配置表。
- hank9999 `refactor/v2` 仍然只在选择器里按 `model contains "opus"` + `supports_opus()` 过滤 Free，不是完整套餐矩阵。
- Foxfishc/gaozuo 当前分支保留了 `subscription_title`、固定 `/v1/models`、Admin 全局模型列表，但没有动态套餐过滤。

## 上游 PR / issue 扫描

相关项：

- hank9999/kiro.rs#77，已合并：`feat: 负载均衡模式下过滤 Free 账号使用 Opus 模型`
  - 只解决 Free 不走 Opus。
  - 不解决 Sonnet 5、Sonnet 4.6、Haiku 等按套餐过滤。
  - 不解决 `/v1/models` 动态返回。
- hank9999/kiro.rs#40，开放：`Feature/新增凭据验证 DTO/逻辑、验证专用 API 调用与上下文获取`
  - 是按用户选择模型进行凭据验活。
  - 不是固定套餐模型表。
- hank9999/kiro.rs#11，开放：包含 `enabledModels` 等手工配置方向。
  - 是凭据级手工启用模型。
  - 不是官方套餐映射。
- hank9999/kiro.rs#158，已关闭：`写死模型列表令人忍俊不禁`
  - 维护者回复：不同号的模型列表可能不一致，v2 重构中。
  - 没有提供可复用实现或配置表。
- hank9999/kiro.rs#184，开放：Sonnet 5 支持。
  - 说明 Sonnet 5 是 Experimental、us-east-1、1M context、1.3x credit multiplier。
  - 不包含套餐映射。

## 官方 Kiro 模型矩阵

来源：

- Kiro Models：https://kiro.dev/docs/models/
- 页面显示更新时间：July 1, 2026。
- Kiro Pricing：https://kiro.dev/pricing/

`kiro.dev/docs/models/` 的 Quick comparison 表：

| Model | Context | Cost | Region | Free | Pro | Pro+ | Power |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Claude Opus 4.8 | 1M | 2.2x | us-east-1, eu-central-1 |  | yes | yes | yes |
| Claude Opus 4.7 | 1M | 2.2x | us-east-1, eu-central-1 |  | yes | yes | yes |
| Claude Opus 4.6 | 1M | 2.2x | us-east-1, eu-central-1 |  | yes | yes | yes |
| Claude Opus 4.5 | 200K | 2.2x | us-east-1, eu-central-1 |  | yes | yes | yes |
| Claude Sonnet 5 | 1M | 1.3x | us-east-1 |  | yes | yes | yes |
| Claude Sonnet 4.6 | 1M | 1.3x | us-east-1, eu-central-1 |  | yes | yes | yes |
| Claude Sonnet 4.5 | 200K | 1.3x | us-east-1, eu-central-1 | yes | yes | yes | yes |
| Claude Sonnet 4.0 | 200K | 1.3x | us-east-1, eu-central-1 | yes | yes | yes | yes |
| Auto |  | 1.0x | us-east-1, eu-central-1 | yes | yes | yes | yes |
| Claude Haiku 4.5 | 200K | 0.4x | us-east-1, eu-central-1 |  | yes | yes | yes |
| DeepSeek 3.2 | 128K | 0.25x | us-east-1 | yes | yes | yes | yes |
| MiniMax M2.5 | 200K | 0.25x | us-east-1, eu-central-1 | yes | yes | yes | yes |
| GLM-5 | 200K | 0.5x | us-east-1 | yes | yes | yes | yes |
| MiniMax M2.1 | 200K | 0.15x | us-east-1, eu-central-1 | yes | yes | yes | yes |
| Qwen3 Coder Next | 256K | 0.05x | us-east-1, eu-central-1 | yes | yes | yes | yes |

`kiro.dev/pricing/` 说明：

- Free：50 credits/month，access to open weight models and Claude Sonnet 4.5。
- Pro / Pro+ / Pro Max / Power：access to premium models。
- Pricing 文案里同时有一处 FAQ/正文提到 Free includes open weight models and Claude Sonnet 4.6 with limits；但 Models 页面矩阵和 structured offer 都明确 Free 仅勾选 Claude Sonnet 4.5，不勾选 Sonnet 4.6。后续实现应优先采用 Models 页面矩阵，并把该差异记录为官方文案不一致。

## 本代理可实现的初始能力表

这个表是「官方矩阵 ∩ 当前代理支持的模型 ID」。

未知套餐：空集合。

`KIRO FREE`：

- `claude-sonnet-4-5-20250929`
- `claude-sonnet-4-5-20250929-thinking`
- `claude-sonnet-4-5-20250929-agentic`

`KIRO PRO`、`KIRO PRO+`、`KIRO PRO MAX`、`KIRO POWER`：

- `claude-sonnet-5`
- `claude-sonnet-5-thinking`
- `claude-sonnet-5-agentic`
- `claude-sonnet-4-6`
- `claude-sonnet-4-6-thinking`
- `claude-sonnet-4-6-agentic`
- `claude-sonnet-4-5-20250929`
- `claude-sonnet-4-5-20250929-thinking`
- `claude-sonnet-4-5-20250929-agentic`
- `claude-opus-4-5-20251101`
- `claude-opus-4-5-20251101-thinking`
- `claude-opus-4-5-20251101-agentic`
- `claude-opus-4-6`
- `claude-opus-4-6-thinking`
- `claude-opus-4-6-agentic`
- `claude-opus-4-7`
- `claude-opus-4-7-thinking`
- `claude-opus-4-7-agentic`
- `claude-opus-4-8`
- `claude-opus-4-8-thinking`
- `claude-opus-4-8-agentic`
- `claude-haiku-4-5-20251001`
- `claude-haiku-4-5-20251001-thinking`
- `claude-haiku-4-5-20251001-agentic`

说明：

- `Pro Max` 在官方 Models 表头未单独出现，但 Pricing 页把 Pro Max 归为 paid plan，且 paid plans have access to premium models。因此按 paid tier 处理。
- 当前用户实测 Free 凭据可用 `Claude Haiku 4.5`，但官方 Models 矩阵 Free 未勾选 Haiku。为了不拍脑袋，默认不把 Haiku 纳入 Free；如需支持，只能作为显式本地 override，而不是官方基础表。

## 推荐后续设计

推荐方案：集中模型 catalog + 套餐能力表 + 路由共享过滤。

组件边界：

- 模型 catalog：把当前 `get_models()` 的硬编码列表抽到单独模块，作为服务支持的模型全集。
- 套餐识别：把 `subscription_title` 标准化为 `Free | Pro | ProPlus | ProMax | Power | Unknown`。
- 套餐能力表：从标准套餐返回模型 ID 集合；Unknown 返回空集合。
- `/v1/models`：从启用凭据中读取已识别套餐，合并所有账号可用模型，按 catalog 顺序返回。
- 请求路由：从请求 payload 中提取模型，映射到 Kiro 模型或规范模型族后，选择支持该模型的凭据；不支持的凭据不参与该请求。
- 错误行为：如果没有任何凭据支持请求模型，应返回清晰错误，不再把请求发给随机账号等待上游 `INVALID_MODEL_ID`。

不推荐方案：

- 仅修改 `/v1/models`：展示变准，但请求仍会被错误账号处理。
- 每次动态探测所有模型：慢、消耗 credits、容易触发限制，而且结果会受区域/临时灰度影响。
- 手工每凭据维护 enabledModels 作为主方案：适合 override，不适合作为官方套餐默认能力来源。

## 下一步 gate

进入实现计划前需要确认：

- Free 是否严格按官方矩阵，只返回 Sonnet 4.5。
- 是否允许添加显式本地 override 来记录“某些 Free 凭据实测可用 Haiku 4.5”。

默认建议：严格按官方矩阵；未知套餐过滤；Haiku Free 不默认放行。
