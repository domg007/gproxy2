# ClaudeCode CCH

`claudecode` 渠道在发送到 `/v1/messages` 前,对最终请求 body 计算 CCH
(`x-anthropic-billing-header` 里的校验和),并注入 `metadata.user_id`。
实现见 [`src/channel/bulletins/claudecode/cch.rs`](../src/channel/bulletins/claudecode/cch.rs),
由 [`mod.rs`](../src/channel/bulletins/claudecode/mod.rs) 的 `prepare` 调用。

## metadata.user_id

真实 `claude-cli` 把三元组以 **JSON 字符串**塞进 `metadata.user_id`:

```json
"metadata": { "user_id": "{\"device_id\":\"<64-hex>\",\"account_uuid\":\"\",\"session_id\":\"<uuidv4>\"}" }
```

- `device_id`:取凭证 `secret.device_id`(operator 字段),无则空串。
- `account_uuid`:取 `secret.account_uuid`(cookie 引导时写入),无则空串。
- `session_id`:每请求生成的 UUIDv4,**与 `x-claude-code-session-id` 头同值**。

## CCH 算法

1. 在 `system[0]` 插入 billing header text block,先写占位值:

```text
x-anthropic-billing-header: cc_version=2.1.162.553; cc_entrypoint=cli; cch=00000;
```

(`cc_version` = CLI 版本 `2.1.162` + build suffix `553`;`cc_entrypoint` 纯终端为 `cli`,VSCode 入口为 `claude-vscode`。)

2. 用最终要发送的完整 JSON body 计算:

```text
cch = xxh64(body_bytes_with_cch_00000, seed=0x4d659218e32a3268) & 0xfffff
```

3. 格式化为 5 位小写 hex,**原地字节替换** header 里的 `cch=00000;`。

CCH 覆盖整个最终 body,所以 `tools`、`system`、`messages`、`model` 等字段都会影响结果。HTTP header、URL、OAuth token 不参与计算。校验和算在**网关自己序列化的最终 bytes** 上(自洽:服务端重算收到的 body 即匹配)。

> 已用真实 `claude-cli` 2.1.162 wire body 验证:`real cch = fab34` == 重算值;单测 `cch_matches_known_vector` 锁定算法(已知向量 `b3b78`)。

## 旁注

- `x-client-request-id`:Stainless SDK 每请求加,**仅直连 `api.anthropic.com`** 时注入(走自建 base_url 不加)。
- 仅 `POST /v1/messages*` 触发 CCH;其它路径(如 count_tokens GET)原样转发。非 JSON-object body 也原样转发。
