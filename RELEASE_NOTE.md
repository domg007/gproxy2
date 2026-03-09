# Release Notes

## v0.3.34

### English

#### Fixed

- Fixed ClaudeCode OAuth persistence so unlabeled OAuth credentials no longer create duplicate rows that can later fail `v1/usage` with `no eligible credential`.
- Fixed admin `Test All Credentials` for browser-driven sessions by dropping browser-only passthrough headers such as `origin`, `referer`, `cookie`, `sec-fetch-*`, and `sec-ch-ua*` before forwarding upstream requests.
- Fixed default credential naming during persistence so OAuth credentials prefer `user_email`/`client_email` when available instead of falling back to numeric placeholder names.

#### Changed

- Updated the release validation command to use workspace-wide clippy checks with `-D warnings` and `-A clippy::too_many_arguments`, matching the repository's current release expectations.

#### Compatibility

- No storage migration is required.
- Existing duplicated historical credentials are not removed automatically; they can be cleaned up from the admin credential list if needed.

### 中文

#### 修复

- 修复 ClaudeCode OAuth 持久化逻辑：未命名的 OAuth 凭证不再重复落库，避免后续 `v1/usage` 出现 `no eligible credential` 的异常。
- 修复后台 `测试所有凭证` 在浏览器场景下误伤 ClaudeCode 凭证的问题：转发上游请求前会过滤 `origin`、`referer`、`cookie`、`sec-fetch-*`、`sec-ch-ua*` 等浏览器专用请求头。
- 修复凭证默认命名逻辑：持久化时若可用，优先使用 `user_email` / `client_email` 作为默认名称，不再优先落成数字占位名。

#### 变更

- 发版脚本中的校验命令已调整为工作区级别的 clippy 检查，并使用 `-D warnings -A clippy::too_many_arguments`，与当前仓库发版要求保持一致。

#### 兼容性

- 无需执行存储迁移。
- 旧版本已经产生的重复历史凭证不会自动删除，如有需要可在后台凭证列表中手动清理。

## v0.3.33

### English

#### Added

- Added a `Test All Credentials` action in the admin credential workspace. It validates each credential by requesting the model list, picking the first available model, and sending a lightweight test chat request.

#### Changed

- Renamed the admin provider workspace entry from `渠道` to `渠道/凭证`, and renamed the provider config tab label to `渠道类型` to better distinguish channel type from credential management.
- Updated the bulk-delete copy in the credential workspace from `删除全部 dead` to `删除全部不可用凭证`.
- Provider-scoped OpenAI-compatible `v1/models` and `v1/chat/completions` routes now accept an optional `credential_id` selector so admin-side verification can target a specific credential directly.

#### Fixed

- Batch credential testing no longer marks transient upstream failures as dead immediately. `429/500/502/503/504` are now recorded as `partial` instead of `dead`.
- Codex `403` HTML edge pages, including common Cloudflare block/interstitial responses, no longer mark credentials as dead directly and are now treated as transient failures.

#### Compatibility

- No storage migration is required.
- Existing dead/partial credential state semantics remain unchanged; this update only makes admin-side verification and Codex edge-failure handling less aggressive.

### 中文

#### 新增

- 后台凭证工作区新增 `测试所有凭证` 按钮。该按钮会先请求模型列表，取第一个可用模型，再发送一条轻量测试消息，用于逐个校验凭证可用性。

#### 变更

- 后台 provider 工作区入口文案由 `渠道` 调整为 `渠道/凭证`，provider 配置页中的下拉项文案由 `渠道` 调整为 `渠道类型`，避免把渠道类型和凭证管理混在一起。
- 凭证工作区的批量删除文案由 `删除全部 dead` 调整为 `删除全部不可用凭证`。
- provider 作用域下的 OpenAI 兼容 `v1/models` 与 `v1/chat/completions` 路由新增可选 `credential_id` 定向参数，方便后台逐个校验凭证。

#### 修复

- 批量测试凭证时，不再把瞬时上游故障直接标记为 dead；`429/500/502/503/504` 现在会记录为 `partial`。
- 修复 Codex `403` HTML 边缘页（包括常见的 Cloudflare 拦截/过渡页）被直接判定为 dead 的问题，现统一按瞬时失败处理。

#### 兼容性

- 无需执行存储迁移。
- 已有 dead/partial 凭证状态语义保持不变；本次更新仅让后台校验与 Codex 边缘故障处理更温和。
