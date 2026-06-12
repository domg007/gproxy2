# Agent CLI TLS 指纹 + User-Agent 参考

> 用途:为 gproxy **TLS 伪装功能**(`providers.tls_fingerprint`,见架构设计 §7.4)提供真实客户端的指纹/UA 目标。
> 采集日期:2026-06-09 · 脚本:[`scripts/capture_tls_fp.py`](../scripts/capture_tls_fp.py)(被动抓 JA3/JA4)、[`scripts/capture_tls_ua.py`](../scripts/capture_tls_ua.py)(MITM 抓 UA)、[`scripts/capture_h2_fp.py`](../scripts/capture_h2_fp.py)(MITM 抓 HTTP/2 指纹)。
> 方法:本地 HTTP `CONNECT` 代理。指纹来自 ClientHello;UA / HTTP2 通过临时 CA 终止 TLS 后读 HTTP 头与帧。
> 采集命令:`env HTTPS_PROXY/https_proxy/HTTP_PROXY/http_proxy=http://127.0.0.1:8888 no_proxy= NO_PROXY= NODE_EXTRA_CA_CERTS=/tmp/fpca.pem SSL_CERT_FILE=/tmp/fpca.pem <cli> <一次性提问>`

## 1. 指纹汇总

| 渠道 | TLS 栈 | ciphers | TLS1.3 位置 | ALPN | PQC | JA3 (MD5) | JA4 |
|---|---|---|---|---|---|---|---|
| **claude** | Node / OpenSSL(裁剪) | 17 | 最前 | `http/1.1` | — | `d871d02cecbde59abbf8f4806134addf` | `t13d1714h1_5b57614c22b0_43ade6aba3df` |
| **codex** ①(辅助/认证) | OpenSSL 3.5(Rust native-tls) | 30 | 最前 | 无 | ✓ | `27718d56688425cd36a401c66147c4ee` | `t13d301100_1d37bd780c83_8e6e362c5eac` |
| **codex** ②(**模型路径**) | rustls/hyper(h2) | 10 | 最前 | `h2` | — | `bc73760f6b846b84e33ae3072ef4e9c1` | `t13d1011h2_61a7ad8aa9b6_3fcd1a44f3e3` |
| **gemini** | 系统 OpenSSL 3.5(全量) | 52 | 最前 | 无 | ✓ | `944d1e1858cd278718f8a46b65d3212f` | `t13d521100_b262b3658495_8e6e362c5eac` |
| **agy**(Antigravity) | Go `crypto/tls` | 13 | **最后** | `h2,http/1.1` | ✓×3 | `03117a8ed39ef02427ebbc39f121275c` | `t13d1312h2_f57a46bbacb6_ab7e3b40a677` |
| **copilot** ①(`api.github.com` 内部) | 系统 OpenSSL 3.5(全量) | 52 | 最前 | `http/1.1` | ✓ | `d67b094811e5145139d7cea5f014309f` | `t13d5212h1_b262b3658495_8e6e362c5eac` |
| **copilot** ②(**模型路径**) | rustls | 10 | 最前 | `http/1.1` | — | 随机(扩展乱序) | `t13d1011h1_61a7ad8aa9b6_*`(JA4_c 可变) |
| **kiro-cli** | rustls / aws-lc | 10 | 最前 | 无 | — | `49ae0c94…` / `8ed2b010…`(不稳定,见注) | `t13d101000_61a7ad8aa9b6_3fcd1a44f3e3` |

## 2. User-Agent 汇总

> 均在各自**模型路径**连接上抓取(claude `/v1/messages`、codex `/v1/responses`、gemini `cloudcode-pa…/v1internal:streamGenerateContent`、copilot `api.individual.githubcopilot.com`、kiro `codewhispererstreaming`)。gemini/copilot 经**转发型 MITM**([`scripts/capture_fwd_mitm.py`](../scripts/capture_fwd_mitm.py))抓到。

| 渠道 | 模型路径 UA(**伪装用这个**) | 端点 |
|---|---|---|
| **claude** | `claude-cli/2.1.162 (external, claude-vscode, agent-sdk/0.3.173)` | `POST api.anthropic.com/v1/messages` |
| **codex** | `codex_exec/0.137.0 (Debian 13.0.0; x86_64) xterm-256color (codex_exec; 0.137.0)` | `POST …/v1/responses` |
| **gemini** | `GeminiCLI-tui/0.46.0/<model> (linux; x64; terminal) google-api-nodejs-client/9.15.1`(`<model>` = `gemini-2.5-pro`/`gemini-2.5-flash`) | `POST cloudcode-pa.googleapis.com/v1internal:streamGenerateContent` |
| **agy** | `codeium-language-server` | `antigravity-unleash.goog/api/client/*` |
| **copilot** | `copilot/1.0.61 (linux v24.16.0) term/unknown` | `api.individual.githubcopilot.com` |
| **kiro-cli** | `aws-sdk-rust/1.3.15 ua/2.1 api/codewhispererstreaming/0.1.16551 os/linux lang/rust/1.92.0 md/appVersion-2.6.1 app/AmazonQ-For-CLI` | `POST runtime.us-east-1.kiro.dev/` |

> 其它路径 UA(非模型):claude bootstrap `claude-code/2.1.162`、遥测 `axios/1.15.2`;copilot 内部 `api.github.com` 校验同 UA;gemini token 校验 `google-api-nodejs-client/9.15.1`;kiro 非流式 `api/codewhispererruntime/…`、遥测 `aws-sdk-rust/1.3.15`。

## 3. 伪装注意事项

- **优先用 JA4,不用 JA3**:`kiro` 的 JA3 两次采集不同(`49ae0c94` vs `8ed2b010`),因 rustls/aws-lc **随机化扩展顺序**;但 JA4 因排序而稳定。`codex` 单进程内存在 **两套 TLS 客户端**(native-OpenSSL 30 套件 + rustls/h2 10 套件),JA3 随抓到哪条连接而变。伪装匹配应以 JA4 为基准。
- **均不像浏览器**:六者全部无 GREASE、无 cipher 乱序,是 OpenSSL / Node / Go / rustls 原生画像。`wreq` 内置多为浏览器(Chrome/Firefox/Safari/OkHttp)preset,要伪装成这些 agent 需**自定义 TLS 配置**,而非现成 preset。
- **指纹会聚类**:`gemini` 与 `copilot` 的 `JA4_b/JA4_c`(`b262b3658495_8e6e362c5eac`)逐字节相同,仅 ALPN 不同;`codex②` 与 `kiro` 的 `JA4_b/JA4_c`(`61a7ad8aa9b6_3fcd1a44f3e3`)相同(都是 rustls)。同库即同尾巴。
- **UA 与指纹要配套伪装**:仅改 TLS 指纹而 UA 不符,或反之,都会露馅。注意部分 UA(gemini/copilot)抓自认证/内部路径,模型路径可能不同。
- **HTTP/2 层也要对齐**(详见 §6):`codex`(rustls/hyper)、`agy`(Go)走 h2,有独立的 SETTINGS/窗口/伪头序指纹;`claude`、`kiro` **只用 HTTP/1.1**(已在模型连接确认),没有 h2 层。伪装 h2 客户端时只对 TLS 不对 h2,Akamai 指纹一样会暴露。

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

### gemini(系统 OpenSSL 3.5 · 模型路径 `cloudcode-pa.googleapis.com`)
- **UA**: 模型 `GeminiCLI-tui/0.46.0/<model> (linux; x64; terminal) google-api-nodejs-client/9.15.1`;认证 `google-api-nodejs-client/9.15.1`
- **JA3**: `944d1e1858cd278718f8a46b65d3212f` · **JA4**: `t13d521100_b262b3658495_8e6e362c5eac`
  ```
  771,4866-4867-4865-49199-49195-49200-49196-158-49191-103-49192-107-163-159-52393-52392-52394-49325-49311-49245-49249-49239-49235-162-49324-49310-49244-49248-49238-49234-49188-106-49187-64-49162-49172-57-56-49161-49171-51-50-157-49309-49233-156-49308-49232-61-60-53-47,65281-0-11-10-35-22-23-13-43-45-51,4588-29-23-30-24-25-256-257,0-1-2
  ```
- ciphers(52):系统 OpenSSL **全量**(含 ARIA/CAMELLIA/CCM/DHE) · ext(11):同 codex① · ec_pt_fmts:`0,1,2` · ALPN:无 · **HTTP/1.1**
- ✅ **已确认**:模型路径(`cloudcode-pa.googleapis.com/v1internal:streamGenerateContent`)与认证路径**同一 52-cipher OpenSSL 栈**——之前"疑非模型客户端"的存疑已排除。整条都是 http/1.1。

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
| `http2.*` | `Http2Options`(见 §6) | `enable_push`/`initial_window_size`/`initial_connection_window_size`/`max_frame_size`/`max_header_list_size`/`header_table_size`/`max_concurrent_streams`/`headers_pseudo_order`/`settings_order`/`priorities` |
| `http2: false` | `http1_only`(`claude`/`kiro`) | 只用 HTTP/1.1 的渠道,**不要**协商 h2 |
| `_reference` / `_fidelity` / `_unsupported` | — | 仅注释,加载时忽略 |

> ⚠️ **BoringSSL 保真度上限(重要)**:wreq 用 BoringSSL,**无法逐字节复刻**非 BoringSSL 栈的握手。可精确的:**UA、ALPN、关闭 GREASE、AEAD 套件顺序、标准曲线/sigalgs**。**无法复刻的**:OpenSSL 全量套件(ARIA/CAMELLIA/CCM/DHE/静态 RSA-CBC)、`x448`、`ffdhe`、`ed448` 及 `0x081x` 实验 sigalgs、`agy` 的自定义 PQC 组(`0x11eb/0x11ed`)、OpenSSL 特有扩展(`padding` 之外 BoringSSL 不发的)。因此 **gemini / copilot①(api.github.com 内部)/ codex① 的 JA3/JA4 基本不可复刻**;agy / kiro / codex② / **copilot②(模型路径,rustls)** / claude 可做到 JA4_a/_b 接近,JA4_c 可能有差。**结论:伪装以「UA 精确 + ALPN + 关 GREASE + 套件近似」为目标,JA3/JA4 完全对齐需换非 BoringSSL 后端。**

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
  "http2": false,
  "_reference": { "ja4": "t13d1714h1_5b57614c22b0_43ade6aba3df", "ja3_md5": "d871d02cecbde59abbf8f4806134addf", "model_ua": "claude-cli/2.1.162 (external, claude-vscode, agent-sdk/0.3.173)" },
  "_fidelity": "high",
  "_unsupported": "claude 全程 HTTP/1.1(http2:false);padding/SCT 扩展 BoringSSL 也发,套件全为 BoringSSL 已知,JA4_c 可能小差。注意模型路径 UA 是 claude-cli/...,与 bootstrap 的 claude-code/... 不同。"
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
  "http2": {
    "enable_push": false,
    "initial_window_size": 4194304,
    "initial_connection_window_size": 1073807359,
    "max_frame_size": 1048576,
    "max_header_list_size": 10485760,
    "headers_pseudo_order": [":authority", ":method", ":path", ":scheme"],
    "settings_order": [2, 4, 5, 6]
  },
  "_reference": { "ja4": "t13d1312h2_f57a46bbacb6_ab7e3b40a677", "ja3_md5": "03117a8ed39ef02427ebbc39f121275c", "h2_akamai": "2:0;4:4194304;5:1048576;6:10485760|1073741824|0|a,m,p,s" },
  "_fidelity": "medium",
  "_unsupported": "Go 把 TLS1.3 套件放末尾,BoringSSL 排序不同;自定义 PQC 组 0x11eb/0x11ed 无法复刻。h2 为 Go net/http2 典型指纹(伪头序 a,m,p,s,连接窗口增量 1GiB)。"
}
```

### kiro-cli(保真度:中,rustls)
```json
{
  "headers": { "user-agent": "aws-sdk-rust/1.3.15 ua/2.1 api/codewhispererstreaming/0.1.16551 os/linux lang/rust/1.92.0 md/appVersion-2.6.1 app/AmazonQ-For-CLI" },
  "tls": {
    "alpn_protocols": [],
    "grease_enabled": false,
    "min_tls_version": "tls1.2", "max_tls_version": "tls1.3",
    "cipher_list": "TLS_AES_256_GCM_SHA384:TLS_AES_128_GCM_SHA256:TLS_CHACHA20_POLY1305_SHA256:ECDHE-ECDSA-AES256-GCM-SHA384:ECDHE-ECDSA-AES128-GCM-SHA256:ECDHE-ECDSA-CHACHA20-POLY1305:ECDHE-RSA-AES256-GCM-SHA384:ECDHE-RSA-AES128-GCM-SHA256:ECDHE-RSA-CHACHA20-POLY1305",
    "curves_list": "X25519:P-256:P-384",
    "sigalgs_list": "ecdsa_secp384r1_sha384:ecdsa_secp256r1_sha256:ed25519:rsa_pss_rsae_sha512:rsa_pss_rsae_sha384:rsa_pss_rsae_sha256:rsa_pkcs1_sha512:rsa_pkcs1_sha384:rsa_pkcs1_sha256"
  },
  "http2": false,
  "_reference": { "ja4": "t13d101000_61a7ad8aa9b6_3fcd1a44f3e3", "ja3_md5": "随机(扩展乱序),以 JA4 为准" },
  "_fidelity": "medium",
  "_unsupported": "kiro 全程 HTTP/1.1(含 codewhispererstreaming 模型流式,http2:false);rustls 扩展集极简,BoringSSL 会多发 session_ticket/status_request/SCT 等 → JA4_c 偏差。模型 UA 为 codewhispererstreaming;非流式 runtime 用 api/codewhispererruntime;遥测用裸 aws-sdk-rust/1.3.15。"
}
```

### codex(保真度:中;模型路径=②号 rustls/hyper,走 h2)
```json
{
  "headers": { "user-agent": "codex_exec/0.137.0 (Debian 13.0.0; x86_64) xterm-256color (codex_exec; 0.137.0)" },
  "tls": {
    "alpn_protocols": ["h2"],
    "grease_enabled": false,
    "min_tls_version": "tls1.2", "max_tls_version": "tls1.3",
    "cipher_list": "TLS_AES_256_GCM_SHA384:TLS_AES_128_GCM_SHA256:TLS_CHACHA20_POLY1305_SHA256:ECDHE-ECDSA-AES256-GCM-SHA384:ECDHE-ECDSA-AES128-GCM-SHA256:ECDHE-ECDSA-CHACHA20-POLY1305:ECDHE-RSA-AES256-GCM-SHA384:ECDHE-RSA-AES128-GCM-SHA256:ECDHE-RSA-CHACHA20-POLY1305",
    "curves_list": "X25519:P-256:P-384"
  },
  "http2": {
    "enable_push": false,
    "initial_window_size": 2097152,
    "initial_connection_window_size": 5242880,
    "max_frame_size": 16384,
    "max_header_list_size": 16384,
    "headers_pseudo_order": [":method", ":scheme", ":authority", ":path"],
    "settings_order": [2, 4, 5, 6]
  },
  "_reference": { "ja4": "t13d1011h2_61a7ad8aa9b6_3fcd1a44f3e3", "ja3_md5": "bc73760f6b846b84e33ae3072ef4e9c1", "h2_akamai": "2:0;4:2097152;5:16384;6:16384|5177345|0|m,s,a,p", "model_path": "/v1/responses" },
  "_fidelity": "medium",
  "_unsupported": "codex 模型路径(/v1/responses)= rustls/hyper + h2(此处采用)。另有一套 native-OpenSSL 30 套件(含 x448/ffdhe)仅用于辅助/认证,不可复刻、也非模型路径。"
}
```

### gemini(模型路径已确认 · 保真度:低 — 全量 OpenSSL,不可复刻)
```json
{
  "headers": { "user-agent": "GeminiCLI-tui/0.46.0/gemini-2.5-pro (linux; x64; terminal) google-api-nodejs-client/9.15.1" },
  "tls": {
    "alpn_protocols": [],
    "grease_enabled": false,
    "min_tls_version": "tls1.2", "max_tls_version": "tls1.3",
    "cipher_list": "TLS_AES_256_GCM_SHA384:TLS_AES_128_GCM_SHA256:TLS_CHACHA20_POLY1305_SHA256:ECDHE-ECDSA-AES128-GCM-SHA256:ECDHE-RSA-AES128-GCM-SHA256:ECDHE-ECDSA-AES256-GCM-SHA384:ECDHE-RSA-AES256-GCM-SHA384:ECDHE-ECDSA-CHACHA20-POLY1305:ECDHE-RSA-CHACHA20-POLY1305",
    "curves_list": "X25519MLKEM768:X25519:P-256:P-384:P-521"
  },
  "http2": false,
  "_reference": { "ja4": "t13d521100_b262b3658495_8e6e362c5eac", "model_endpoint": "cloudcode-pa.googleapis.com/v1internal:streamGenerateContent", "model_ua_var": "<model> = gemini-2.5-pro | gemini-2.5-flash" },
  "_fidelity": "low",
  "_unsupported": "模型路径已确认 = 系统 OpenSSL 全量 52 套件(含 ARIA/CAMELLIA/CCM/DHE/静态RSA-CBC)+ x448/ffdhe + ed448/0x081x sigalgs + ec_point_formats(0,1,2),BoringSSL 无法发出 → JA3/JA4 不可复刻;UA 与 http1 可精确。模型路径 = 认证路径同一 TLS 栈。"
}
```

### copilot(模型路径已确认 · 保真度:中 — rustls,与早前 52 套件 OpenSSL 不同)
```json
{
  "headers": { "user-agent": "copilot/1.0.61 (linux v24.16.0) term/unknown" },
  "tls": {
    "alpn_protocols": ["http/1.1"],
    "grease_enabled": false,
    "min_tls_version": "tls1.2", "max_tls_version": "tls1.3",
    "cipher_list": "TLS_AES_256_GCM_SHA384:TLS_AES_128_GCM_SHA256:TLS_CHACHA20_POLY1305_SHA256:ECDHE-ECDSA-AES256-GCM-SHA384:ECDHE-ECDSA-AES128-GCM-SHA256:ECDHE-ECDSA-CHACHA20-POLY1305:ECDHE-RSA-AES256-GCM-SHA384:ECDHE-RSA-AES128-GCM-SHA256:ECDHE-RSA-CHACHA20-POLY1305",
    "curves_list": "X25519:P-256:P-384"
  },
  "http2": false,
  "_reference": { "ja4": "t13d1011h1_61a7ad8aa9b6_(可变)", "model_endpoint": "api.individual.githubcopilot.com", "note": "JA4_c 随扩展乱序变化,以 JA4_a+JA4_b 为准;ja4_a=t13d1011h1, ja4_b=61a7ad8aa9b6(rustls)" },
  "_fidelity": "medium",
  "_unsupported": "copilot 双栈:模型路径(api.individual.githubcopilot.com)= rustls/http1(此处采用,JA4 t13d1011h1_61a7ad8aa9b6,JA4_c 随机);内部 api.github.com 校验路径才是 52 套件系统 OpenSSL。rustls 扩展乱序 → JA3 不稳定,以 JA4_a/_b 为准。"
}
```

### codex①-OpenSSL(辅助/认证路径,非模型路径 · 保真度:低 — 仅参考)
```json
{
  "_reference": { "ja4": "t13d301100_1d37bd780c83_8e6e362c5eac", "note": "codex 的 native-OpenSSL 30 套件,仅辅助/认证连接;模型路径见上文 codex(rustls/h2),此项一般无需伪装。" },
  "_fidelity": "low"
}
```

---

## 6. HTTP/2 指纹层

> Akamai 格式:`SETTINGS|WINDOW_UPDATE|PRIORITY|伪头序`。脚本:[`scripts/capture_h2_fp.py`](../scripts/capture_h2_fp.py)(服务端 ALPN 给 `h2`,解析裸帧 + HPACK 伪头序)。均在**登录后的模型路径**连接上抓取。

| 渠道 | 走 h2? | Akamai HTTP/2 指纹 | 伪头序 | 备注 |
|---|---|---|---|---|
| **claude** | ❌ 仅 HTTP/1.1 | — | — | 模型 `/v1/messages`、bootstrap、遥测、MCP 全部 http/1.1(直连 api.anthropic.com 亦同) |
| **codex** | ✅ h2(rustls/hyper) | `2:0;4:2097152;5:16384;6:16384\|5177345\|0\|m,s,a,p` | :method :scheme :authority :path | 模型 `/v1/responses`;无 push/无 priority,2MiB 初始窗口 |
| **agy** | ✅ h2(Go net/http2) | `2:0;4:4194304;5:1048576;6:10485760\|1073741824\|0\|a,m,p,s` | :authority :method :path :scheme | 连接窗口增量 1GiB,Go 典型 |
| **gemini** | ❌ 仅 HTTP/1.1 | — | — | 模型 `cloudcode-pa…/v1internal:streamGenerateContent` 为 http/1.1 |
| **copilot** | ❌ 仅 HTTP/1.1 | — | — | 模型 `api.individual.githubcopilot.com` 为 http/1.1(rustls 栈) |
| **kiro-cli** | ❌ 仅 HTTP/1.1 | — | — | 含 `codewhispererstreaming` 模型流式调用也是 http/1.1 |

**解读**
- **codex** = Rust `hyper`/`h2`:`SETTINGS` 只发 `ENABLE_PUSH=0 / INITIAL_WINDOW_SIZE=2MiB / MAX_FRAME_SIZE=16KiB / MAX_HEADER_LIST_SIZE=16KiB`(无 header_table_size、无 max_concurrent_streams),伪头序 `m,s,a,p`。
- **agy** = Go `net/http2`:`INITIAL_WINDOW_SIZE=4MiB / MAX_FRAME_SIZE=1MiB / MAX_HEADER_LIST_SIZE=10MiB`,连接级 `WINDOW_UPDATE=1GiB`,伪头序 `a,m,p,s`——这是 Go 的标志性 h2 指纹。
- **claude、gemini、copilot、kiro 的模型路径都只用 HTTP/1.1**(均已在模型连接确认),没有 h2 层指纹 → JSON 里用 `"http2": false`(`http1_only`)。**只有 codex、agy 走 h2。**
- gemini/copilot 的模型路径经**转发型 MITM**([`scripts/capture_fwd_mitm.py`](../scripts/capture_fwd_mitm.py),转发真上游 + 被动嗅探 + 镜像客户端 ALPN)抓到;copilot 需 `GH_TOKEN`(`gh auth token`)跳过其绕代理的 OAuth 校验。
- wreq `Http2Options` 可完整复刻 codex/agy 的 h2 层(`headers_pseudo_order` + `settings_order` + 各窗口/帧值),见 §5 两份草案的 `http2` 块。

---

## 7. 备注

- SNI 随目标主机变化;JA3/JA4 的 cipher/extension/curve 才是栈特征。**指纹与目标主机无关**:claude 对 `api.anthropic.com`、`gproxy.lin.pub`、`api.githubcopilot.com` 的握手完全一致(JA4 `t13d1714h1`),只有 SNI 不同。
- 复现:`python3 scripts/capture_h2_fp.py 8888 /tmp/fpca.pem`(终止式,抓 h2/UA)或 `scripts/capture_fwd_mitm.py 8889 /tmp/fpca.pem`(转发式,适合需真上游响应才往下走的渠道,如 gemini/copilot);被动 JA3/JA4 用 `scripts/capture_tls_fp.py 8888 proxy`。详见各脚本头注释。
