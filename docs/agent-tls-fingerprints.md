# Agent CLI TLS 指纹 + User-Agent 参考

> 用途:为 gproxy **TLS 伪装功能**(`providers.tls_fingerprint`,见架构设计 §7.4)提供真实客户端的指纹/UA 目标。
> 采集日期:2026-06-09 · 脚本:[`scripts/capture_tls_fp.py`](../scripts/capture_tls_fp.py)(被动抓 JA3/JA4)、[`scripts/capture_tls_ua.py`](../scripts/capture_tls_ua.py)(MITM 抓 UA + JA3/JA4)。
> 方法:本地 HTTP `CONNECT` 代理。指纹来自 ClientHello;UA 通过临时 CA 终止 TLS 后读 HTTP 头。
> 采集命令:`env HTTPS_PROXY/https_proxy/HTTP_PROXY/http_proxy=http://127.0.0.1:8888 no_proxy= NO_PROXY= NODE_EXTRA_CA_CERTS=/tmp/fpca.pem SSL_CERT_FILE=/tmp/fpca.pem <cli> <一次性提问>`

## 1. 指纹汇总

| 渠道 | TLS 栈 | ciphers | TLS1.3 位置 | ALPN | PQC | JA3 (MD5) | JA4 |
|---|---|---|---|---|---|---|---|
| **claude** | Node / OpenSSL(裁剪) | 17 | 最前 | `http/1.1` | — | `d871d02cecbde59abbf8f4806134addf` | `t13d1714h1_5b57614c22b0_43ade6aba3df` |
| **codex** ① | OpenSSL 3.5(Rust native-tls) | 30 | 最前 | 无 | ✓ | `27718d56688425cd36a401c66147c4ee` | `t13d301100_1d37bd780c83_8e6e362c5eac` |
| **codex** ② | rustls(同一进程另一客户端) | 10 | 最前 | `h2` | — | `bc73760f6b846b84e33ae3072ef4e9c1` | `t13d1011h2_61a7ad8aa9b6_3fcd1a44f3e3` |
| **gemini** | 系统 OpenSSL 3.5(全量) | 52 | 最前 | 无 | ✓ | `944d1e1858cd278718f8a46b65d3212f` | `t13d521100_b262b3658495_8e6e362c5eac` |
| **agy**(Antigravity) | Go `crypto/tls` | 13 | **最后** | `h2,http/1.1` | ✓×3 | `03117a8ed39ef02427ebbc39f121275c` | `t13d1312h2_f57a46bbacb6_ab7e3b40a677` |
| **copilot** | 系统 OpenSSL 3.5(全量) | 52 | 最前 | `http/1.1` | ✓ | `d67b094811e5145139d7cea5f014309f` | `t13d5212h1_b262b3658495_8e6e362c5eac` |
| **kiro-cli** | rustls / aws-lc | 10 | 最前 | 无 | — | `49ae0c94…` / `8ed2b010…`(不稳定,见注) | `t13d101000_61a7ad8aa9b6_3fcd1a44f3e3` |

## 2. User-Agent 汇总

| 渠道 | 产品 UA(CONNECT 请求) | 隧道内请求 UA | 采集端点 | 路径性质 |
|---|---|---|---|---|
| **claude** | — | `claude-code/2.1.162` | `api.anthropic.com` `/api/claude_cli/bootstrap` | CLI 真实 UA |
| **codex** | `codex_exec/0.137.0 (Debian 13.0.0; x86_64) xterm-256color` | 同左 | `chatgpt.com` `/backend-api/plugins/featured` | CLI 真实 UA |
| **gemini** | — | `google-api-nodejs-client/9.15.1` | `oauth2.googleapis.com` `/token` | **认证路径**(模型路径 UA 未确认) |
| **agy** | `Go-http-client/1.1` | `codeium-language-server` | `antigravity-unleash.goog` `/api/client/features` | 真实(Antigravity 基于 Codeium) |
| **copilot** | — | `undici` | `api.github.com` `/copilot_internal/user` | 内部校验(裸 undici,模型调用 UA 或不同) |
| **kiro-cli** | `kiro-cli-linux-x86_64-2.6.1` | `aws-sdk-rust/1.3.10 os/linux lang/rust/1.92.0` | `oidc.us-east-1.amazonaws.com` `/client/register` | **认证路径**(AWS SSO-OIDC) |

> 「产品 UA」= 客户端发给代理的 `CONNECT` 请求里的 UA(产品身份);「隧道内 UA」= 进入 TLS 后发给真实主机的 UA。两者不同的(agy/kiro/codex)都列出。

## 3. 伪装注意事项

- **优先用 JA4,不用 JA3**:`kiro` 的 JA3 两次采集不同(`49ae0c94` vs `8ed2b010`),因 rustls/aws-lc **随机化扩展顺序**;但 JA4 因排序而稳定。`codex` 单进程内存在 **两套 TLS 客户端**(native-OpenSSL 30 套件 + rustls/h2 10 套件),JA3 随抓到哪条连接而变。伪装匹配应以 JA4 为基准。
- **均不像浏览器**:六者全部无 GREASE、无 cipher 乱序,是 OpenSSL / Node / Go / rustls 原生画像。`wreq` 内置多为浏览器(Chrome/Firefox/Safari/OkHttp)preset,要伪装成这些 agent 需**自定义 TLS 配置**,而非现成 preset。
- **指纹会聚类**:`gemini` 与 `copilot` 的 `JA4_b/JA4_c`(`b262b3658495_8e6e362c5eac`)逐字节相同,仅 ALPN 不同;`codex②` 与 `kiro` 的 `JA4_b/JA4_c`(`61a7ad8aa9b6_3fcd1a44f3e3`)相同(都是 rustls)。同库即同尾巴。
- **UA 与指纹要配套伪装**:仅改 TLS 指纹而 UA 不符,或反之,都会露馅。注意部分 UA(gemini/kiro)抓自认证路径,模型路径可能不同;`copilot` 模型调用 UA 大概率不是裸 `undici`。

---

## 4. 各渠道明细

### claude — `2.1.162`(Node/OpenSSL · `api.anthropic.com`)
- **UA**: `claude-code/2.1.162`
- **JA3**: `d871d02cecbde59abbf8f4806134addf` · **JA4**: `t13d1714h1_5b57614c22b0_43ade6aba3df`
  ```
  771,4865-4866-4867-49195-49199-49196-49200-52393-52392-49161-49171-49162-49172-156-157-47-53,0-23-65281-10-11-35-16-5-13-18-51-45-43-21,29-23-24,0
  ```
- ciphers(17):`1301 1302 1303 c02b c02f c02c c030 cca9 cca8 c009 c013 c00a c014 009c 009d 002f 0035`
- extensions(14):`00 17 ff01 0a 0b 23 10 05 0d 12 33 2d 2b 15`(含 padding `0x15`) · curves:`x25519,P-256,P-384` · ec_pt_fmts:`0` · ALPN:`http/1.1`

### codex — `0.137.0`(双 TLS 栈 · `chatgpt.com`)
- **UA**: `codex_exec/0.137.0 (Debian 13.0.0; x86_64) xterm-256color`
- **① native-tls/OpenSSL 3.5** — JA3 `27718d56688425cd36a401c66147c4ee` · JA4 `t13d301100_1d37bd780c83_8e6e362c5eac`
  ```
  771,4866-4867-4865-49196-49200-159-52393-52392-52394-49195-49199-158-49188-49192-107-49187-49191-103-49162-49172-57-49161-49171-51-157-156-61-60-53-47,65281-0-11-10-35-22-23-13-43-45-51,4588-29-23-30-24-25-256-257,0-1-2
  ```
  ciphers(30):OpenSSL 默认前 30 · ext(11):`ff01 00 0b 0a 23 16 17 0d 2b 2d 33` · curves:`MLKEM768,x25519,P-256,x448,P-384,P-521,ffdhe2048,ffdhe3072` · ec_pt_fmts:`0,1,2` · ALPN:无
- **② rustls / h2** — JA3 `bc73760f6b846b84e33ae3072ef4e9c1` · JA4 `t13d1011h2_61a7ad8aa9b6_3fcd1a44f3e3`
  ciphers(10):`1302 1301 1303 c02c c02b cca9 c030 c02f cca8 00ff`(AEAD+SCSV) · ALPN:`h2`

### gemini(系统 OpenSSL 3.5 · `oauth2.googleapis.com` — 认证路径)
- **UA**: `google-api-nodejs-client/9.15.1`(token 交换;模型路径需鉴权后另抓)
- **JA3**: `944d1e1858cd278718f8a46b65d3212f` · **JA4**: `t13d521100_b262b3658495_8e6e362c5eac`
  ```
  771,4866-4867-4865-49199-49195-49200-49196-158-49191-103-49192-107-163-159-52393-52392-52394-49325-49311-49245-49249-49239-49235-162-49324-49310-49244-49248-49238-49234-49188-106-49187-64-49162-49172-57-56-49161-49171-51-50-157-49309-49233-156-49308-49232-61-60-53-47,65281-0-11-10-35-22-23-13-43-45-51,4588-29-23-30-24-25-256-257,0-1-2
  ```
- ciphers(52):系统 OpenSSL **全量**(含 ARIA/CAMELLIA/CCM/DHE) · ext(11):同 codex① · ec_pt_fmts:`0,1,2` · ALPN:无
- ⚠️ 此为认证连接;52-cipher 全量画像疑来自原生 OpenSSL 组件,gemini 真正模型推理(Node fetch)可能更接近 claude 画像。

### agy — Antigravity(Go `crypto/tls` · `antigravity-unleash.goog`)
- **UA**: `codeium-language-server`(产品 UA / CONNECT:`Go-http-client/1.1`)
- **JA3**: `03117a8ed39ef02427ebbc39f121275c` · **JA4**: `t13d1312h2_f57a46bbacb6_ab7e3b40a677`
  ```
  771,49195-49199-49196-49200-52393-52392-49161-49171-49162-49172-4865-4866-4867,0-11-65281-23-18-5-10-13-50-16-43-51,4588-4587-4589-29-23-24-25,0
  ```
- ciphers(13):`c02b c02f c02c c030 cca9 cca8 c009 c013 c00a c014 1301 1302 1303`(TLS1.3 在**末尾** = Go 特征) · ext(12):`00 0b ff01 17 12 05 0a 0d 32 10 2b 33` · curves:`0x11ec/0x11eb/0x11ed(三个 PQC 混合),x25519,P-256,P-384,P-521` · ALPN:`h2,http/1.1`

### copilot — GitHub Copilot CLI(系统 OpenSSL 3.5 · `api.github.com`)
- **UA**: `undici`(`/copilot_internal/user` 内部校验;模型调用 UA 或不同)
- **JA3**: `d67b094811e5145139d7cea5f014309f` · **JA4**: `t13d5212h1_b262b3658495_8e6e362c5eac`
  ```
  771,4866-4867-4865-49199-49195-49200-49196-158-49191-103-49192-107-163-159-52393-52392-52394-49325-49311-49245-49249-49239-49235-162-49324-49310-49244-49248-49238-49234-49188-106-49187-64-49162-49172-57-56-49161-49171-51-50-157-49309-49233-156-49308-49232-61-60-53-47,65281-0-11-10-35-16-22-23-13-43-45-51,4588-29-23-30-24-25-256-257,0-1-2
  ```
- ciphers(52):与 gemini 完全相同 · ext(12):`ff01 00 0b 0a 23 10 16 17 0d 2b 2d 33`(比 gemini 多 ALPN `0x10`) · ec_pt_fmts:`0,1,2` · ALPN:`http/1.1`

### kiro-cli — `2.6.1` / Amazon Kiro·Q(rustls/aws-lc · `oidc.us-east-1.amazonaws.com`)
- **UA**: `aws-sdk-rust/1.3.10 os/linux lang/rust/1.92.0`(产品 UA / CONNECT:`kiro-cli-linux-x86_64-2.6.1`)
- **JA3**: `49ae0c94…` / `8ed2b010…`(扩展顺序随机,**不稳定**) · **JA4**: `t13d101000_61a7ad8aa9b6_3fcd1a44f3e3`(稳定)
  ```
  771,4866-4865-4867-49196-49195-52393-49200-49199-52392-255,5-51-0-43-23-13-11-10-35-45,29-23-24,0
  ```
- ciphers(10):`1302 1301 1303 c02c c02b cca9 c030 c02f cca8 00ff`(AEAD+SCSV `0x00ff`) · ext(10):`05 33 00 2b 17 0d 0b 0a 23 2d` · curves:`x25519,P-256,P-384`(无 PQC) · ALPN:无

---

## 5. `tls_fingerprint` JSON 草案(wreq 映射)

> 落库到 `providers.tls_fingerprint`(JSON 文本)。当前客户端为 **`wreq 6.0.0-rc.28`(BoringSSL)**;字段对应 `wreq::tls::TlsOptions` 与默认头。

**Schema / wreq 字段对应**

| JSON 键 | wreq 映射 | 说明 |
|---|---|---|
| `headers.user-agent` | `default_headers` | UA,**可精确伪装** |
| `tls.alpn_protocols` | `alpn_protocols` | ALPN,可精确 |
| `tls.grease_enabled` | `grease_enabled` | 这些客户端**均为 `false`**(无 GREASE) |
| `tls.min/max_tls_version` | `min_tls_version`/`max_tls_version` | 均 `tls1.2`~`tls1.3` |
| `tls.cipher_list` | `cipher_list` | BoringSSL cipher token,`:` 分隔 |
| `tls.curves_list` | `curves_list` | 椭圆曲线/组 |
| `tls.sigalgs_list` | `sigalgs_list` | 签名算法 |
| `tls.extension_permutation` | `extension_permutation` | 扩展顺序(仅能排列 BoringSSL 已发的扩展) |
| `tls.preserve_tls13_cipher_list` | `preserve_tls13_cipher_list` | 控制 TLS1.3 套件顺序 |
| `_reference` / `_fidelity` / `_unsupported` | — | 仅注释,加载时忽略 |

> ⚠️ **BoringSSL 保真度上限(重要)**:wreq 用 BoringSSL,**无法逐字节复刻**非 BoringSSL 栈的握手。可精确的:**UA、ALPN、关闭 GREASE、AEAD 套件顺序、标准曲线/sigalgs**。**无法复刻的**:OpenSSL 全量套件(ARIA/CAMELLIA/CCM/DHE/静态 RSA-CBC)、`x448`、`ffdhe`、`ed448` 及 `0x081x` 实验 sigalgs、`agy` 的自定义 PQC 组(`0x11eb/0x11ed`)、OpenSSL 特有扩展(`padding` 之外 BoringSSL 不发的)。因此 **gemini/copilot/codex① 的 JA3/JA4 基本不可复刻**;agy/kiro/codex②/claude 可做到 JA4_a/_b 接近,JA4_c 可能有差。**结论:伪装以「UA 精确 + ALPN + 关 GREASE + 套件近似」为目标,JA3/JA4 完全对齐需换非 BoringSSL 后端。**

### claude(保真度:高)
```json
{
  "headers": { "user-agent": "claude-code/2.1.162" },
  "tls": {
    "alpn_protocols": ["http/1.1"],
    "grease_enabled": false,
    "min_tls_version": "tls1.2", "max_tls_version": "tls1.3",
    "cipher_list": "TLS_AES_128_GCM_SHA256:TLS_AES_256_GCM_SHA384:TLS_CHACHA20_POLY1305_SHA256:ECDHE-ECDSA-AES128-GCM-SHA256:ECDHE-RSA-AES128-GCM-SHA256:ECDHE-ECDSA-AES256-GCM-SHA384:ECDHE-RSA-AES256-GCM-SHA384:ECDHE-ECDSA-CHACHA20-POLY1305:ECDHE-RSA-CHACHA20-POLY1305:ECDHE-ECDSA-AES128-SHA:ECDHE-RSA-AES128-SHA:ECDHE-ECDSA-AES256-SHA:ECDHE-RSA-AES256-SHA:AES128-GCM-SHA256:AES256-GCM-SHA384:AES128-SHA:AES256-SHA",
    "curves_list": "X25519:P-256:P-384",
    "sigalgs_list": "ecdsa_secp256r1_sha256:rsa_pss_rsae_sha256:rsa_pkcs1_sha256:ecdsa_secp384r1_sha384:rsa_pss_rsae_sha384:rsa_pkcs1_sha384:rsa_pss_rsae_sha512:rsa_pkcs1_sha512:rsa_pkcs1_sha1"
  },
  "_reference": { "ja4": "t13d1714h1_5b57614c22b0_43ade6aba3df", "ja3_md5": "d871d02cecbde59abbf8f4806134addf" },
  "_fidelity": "high",
  "_unsupported": "claude 的 padding/SCT 扩展 BoringSSL 也发,套件全为 BoringSSL 已知;JA4_c 可能小差。"
}
```

### agy / Antigravity(保真度:中)
```json
{
  "headers": { "user-agent": "codeium-language-server" },
  "tls": {
    "alpn_protocols": ["h2", "http/1.1"],
    "grease_enabled": false,
    "min_tls_version": "tls1.2", "max_tls_version": "tls1.3",
    "cipher_list": "ECDHE-ECDSA-AES128-GCM-SHA256:ECDHE-RSA-AES128-GCM-SHA256:ECDHE-ECDSA-AES256-GCM-SHA384:ECDHE-RSA-AES256-GCM-SHA384:ECDHE-ECDSA-CHACHA20-POLY1305:ECDHE-RSA-CHACHA20-POLY1305:ECDHE-ECDSA-AES128-SHA:ECDHE-RSA-AES128-SHA:ECDHE-ECDSA-AES256-SHA:ECDHE-RSA-AES256-SHA:TLS_AES_128_GCM_SHA256:TLS_AES_256_GCM_SHA384:TLS_CHACHA20_POLY1305_SHA256",
    "curves_list": "X25519MLKEM768:X25519:P-256:P-384:P-521",
    "sigalgs_list": "rsa_pss_rsae_sha256:ecdsa_secp256r1_sha256:ed25519:rsa_pss_rsae_sha384:rsa_pss_rsae_sha512:rsa_pkcs1_sha256:rsa_pkcs1_sha384:rsa_pkcs1_sha512:ecdsa_secp384r1_sha384:ecdsa_secp521r1_sha512",
    "preserve_tls13_cipher_list": true
  },
  "_reference": { "ja4": "t13d1312h2_f57a46bbacb6_ab7e3b40a677", "ja3_md5": "03117a8ed39ef02427ebbc39f121275c" },
  "_fidelity": "medium",
  "_unsupported": "Go 把 TLS1.3 套件放末尾,BoringSSL 排序不同;自定义 PQC 组 0x11eb/0x11ed 无法复刻。"
}
```

### kiro-cli(保真度:中,rustls)
```json
{
  "headers": { "user-agent": "aws-sdk-rust/1.3.10 os/linux lang/rust/1.92.0" },
  "tls": {
    "alpn_protocols": [],
    "grease_enabled": false,
    "min_tls_version": "tls1.2", "max_tls_version": "tls1.3",
    "cipher_list": "TLS_AES_256_GCM_SHA384:TLS_AES_128_GCM_SHA256:TLS_CHACHA20_POLY1305_SHA256:ECDHE-ECDSA-AES256-GCM-SHA384:ECDHE-ECDSA-AES128-GCM-SHA256:ECDHE-ECDSA-CHACHA20-POLY1305:ECDHE-RSA-AES256-GCM-SHA384:ECDHE-RSA-AES128-GCM-SHA256:ECDHE-RSA-CHACHA20-POLY1305",
    "curves_list": "X25519:P-256:P-384",
    "sigalgs_list": "ecdsa_secp384r1_sha384:ecdsa_secp256r1_sha256:ed25519:rsa_pss_rsae_sha512:rsa_pss_rsae_sha384:rsa_pss_rsae_sha256:rsa_pkcs1_sha512:rsa_pkcs1_sha384:rsa_pkcs1_sha256"
  },
  "_reference": { "ja4": "t13d101000_61a7ad8aa9b6_3fcd1a44f3e3", "ja3_md5": "随机(扩展乱序),以 JA4 为准" },
  "_fidelity": "medium",
  "_unsupported": "rustls 扩展集极简,BoringSSL 会多发 session_ticket/status_request/SCT 等 → JA4_c 偏差;UA 多为认证路径,模型路径或不同。"
}
```

### codex(保真度:中;②号 rustls/h2 客户端)
```json
{
  "headers": { "user-agent": "codex_exec/0.137.0 (Debian 13.0.0; x86_64) xterm-256color" },
  "tls": {
    "alpn_protocols": ["h2"],
    "grease_enabled": false,
    "min_tls_version": "tls1.2", "max_tls_version": "tls1.3",
    "cipher_list": "TLS_AES_256_GCM_SHA384:TLS_AES_128_GCM_SHA256:TLS_CHACHA20_POLY1305_SHA256:ECDHE-ECDSA-AES256-GCM-SHA384:ECDHE-ECDSA-AES128-GCM-SHA256:ECDHE-ECDSA-CHACHA20-POLY1305:ECDHE-RSA-AES256-GCM-SHA384:ECDHE-RSA-AES128-GCM-SHA256:ECDHE-RSA-CHACHA20-POLY1305",
    "curves_list": "X25519:P-256:P-384"
  },
  "_reference": { "ja4_rustls": "t13d1011h2_61a7ad8aa9b6_3fcd1a44f3e3", "ja4_openssl": "t13d301100_1d37bd780c83_8e6e362c5eac" },
  "_fidelity": "medium",
  "_unsupported": "codex 单进程含两套客户端:此处取 rustls/h2 一套(可近似);native-OpenSSL 30 套件一套含 x448/ffdhe,不可复刻。按主要出站连接选其一。"
}
```

### gemini / copilot / codex①(保真度:低 — 全量 OpenSSL,不可复刻)
```json
{
  "headers": { "user-agent": "google-api-nodejs-client/9.15.1" },
  "tls": {
    "alpn_protocols": [],
    "grease_enabled": false,
    "min_tls_version": "tls1.2", "max_tls_version": "tls1.3",
    "cipher_list": "TLS_AES_256_GCM_SHA384:TLS_AES_128_GCM_SHA256:TLS_CHACHA20_POLY1305_SHA256:ECDHE-ECDSA-AES128-GCM-SHA256:ECDHE-RSA-AES128-GCM-SHA256:ECDHE-ECDSA-AES256-GCM-SHA384:ECDHE-RSA-AES256-GCM-SHA384:ECDHE-ECDSA-CHACHA20-POLY1305:ECDHE-RSA-CHACHA20-POLY1305",
    "curves_list": "X25519MLKEM768:X25519:P-256:P-384:P-521"
  },
  "_reference": {
    "gemini": { "ua": "google-api-nodejs-client/9.15.1", "ja4": "t13d521100_b262b3658495_8e6e362c5eac" },
    "copilot": { "ua": "undici", "ja4": "t13d5212h1_b262b3658495_8e6e362c5eac" },
    "codex_openssl": { "ja4": "t13d301100_1d37bd780c83_8e6e362c5eac" }
  },
  "_fidelity": "low",
  "_unsupported": "原始握手为系统 OpenSSL 全量(52/30 套件,含 ARIA/CAMELLIA/CCM/DHE/静态RSA-CBC)+ x448/ffdhe + ed448/0x081x sigalgs + ec_point_formats(0,1,2),BoringSSL 均无法发出。此 JSON 仅为「能跑且 UA 正确」的近似;若必须对齐 JA3/JA4,需换 OpenSSL 后端。copilot 的 undici 多半只是内部校验 UA,模型调用 UA 待补。"
}
```

---

## 6. 备注

- SNI 随目标主机变化;JA3/JA4 的 cipher/extension/curve 才是栈特征。
- `gemini` 模型路径、`copilot` 模型调用 UA 未最终确认(认证/内部路径抓取)。如需补齐:gemini 需有效登录或可用 `GEMINI_API_KEY`;copilot 需登录后触发模型请求。
- 复现:`python3 scripts/capture_tls_ua.py 8888 /tmp/fpca.pem`,再用上文 env(含 CA 信任)启动对应 CLI 一次性提问。
