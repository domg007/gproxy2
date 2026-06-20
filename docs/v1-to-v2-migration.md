# gproxy v1 → v2 迁移指南

> 目标:**替换二进制即用**。把 v2 二进制覆盖 v1,按原方式启动,v2 自动把旧的 `data/gproxy.db` 升级成 v2 库,无需任何手动步骤。
>
> ⚠️ 本迁移是一次性临时能力,计划在 **2.1 版本移除**。请在升级到 2.1 之前完成迁移。

## 1. 它做什么

v2 默认使用 **db 后端**,默认库就是 `<data_dir>/gproxy.db`(与 v1 同名同路径)。启动时如果发现该文件是一个 **v1 数据库**(且 v2 store 尚未建立),v2 会:

1. 只读读取 v1 `gproxy.db`;
2. 在临时文件里建一个全新的 v2 库,导入 v1 的**配置**;
3. 原子换库:把 v1 库改名为 `gproxy.db.v1.bak`(连同 `-wal/-shm`),把新 v2 库放到 `gproxy.db`;
4. 继续正常启动并服务。

幂等:迁移完成后 `gproxy.db` 已是 v2,之后每次启动都自动跳过。v1 原库始终只读,保留为 `gproxy.db.v1.bak` 备份。

## 2. 标准升级步骤(推荐:开箱即用)

```bash
# 1) 停掉 v1 进程(确保旧库已落盘,无进程占用)
systemctl stop gproxy        # 或你的启动方式

# 2) 用 v2 二进制覆盖 v1 二进制
cp gproxy-v2 /usr/local/bin/gproxy

# 3) (可选但强烈建议)先手动备份一份
cp data/gproxy.db data/gproxy.db.manualbak

# 4) 像以前一样启动 v2(指向同一个 data 目录)
gproxy --data-dir ./data --host 0.0.0.0 --port 8787
#   或沿用环境变量:GPROXY_DATA_DIR / GPROXY_HOST / GPROXY_PORT
```

启动日志里会看到:

```
WARN v1 database detected — migrating to v2 in place (original backed up)
INFO v1 → v2 migration complete report=Report { providers: 6, credentials: 6, models: 400, ... }
WARN v1 database backed up; v2 database is now live at .../data/gproxy.db
INFO gproxy v2 listening on http://0.0.0.0:8787
```

完成。`data/` 下现在是 v2 的 `gproxy.db`,外加 `gproxy.db.v1.bak`(你的旧库)。

## 3. 加密库(设置过 `DATABASE_SECRET_KEY`)

如果 v1 用 `DATABASE_SECRET_KEY` 加密了凭证/密钥:

- 迁移时把同一个 `DATABASE_SECRET_KEY` 提供给 v2(env),v2 会用它**解密** v1 secrets。
- 如果你想让 v2 也加密存储,另外设置 v2 的主密钥 `GPROXY_MASTER_KEY`(base64 的 32 字节),v2 会用它**重新封装**。
- 不设 `GPROXY_MASTER_KEY` 则 v2 以明文存储(会有 WARN 提示)。

```bash
DATABASE_SECRET_KEY='<v1 的密钥>' \
GPROXY_MASTER_KEY='<v2 的 base64 32字节主密钥,可选>' \
gproxy --data-dir ./data
```

v1 未加密(未设 `DATABASE_SECRET_KEY`)则无需任何密钥。

## 4. 显式 / 离线迁移(可选)

不想走自动路径,或要迁到 Postgres/MySQL,用 `migrate-v1` 子命令:

```bash
# 先看会迁移多少(只读,不写)
gproxy migrate-v1 --from ./data/gproxy.db --dry-run

# 迁到指定目标库(默认 --to 为当前 db dsn)
gproxy migrate-v1 --from ./old/gproxy.db --to 'postgres://user:pass@host/gproxy'
```

`migrate-v1` 不做换文件动作,直接把 v1 配置导入 `--to` 指向的 v2 库;目标应为空库。

## 5. 迁移了什么

| 迁移 | 说明 |
|---|---|
| providers / credentials | credentials 的 secret 会解密→(可选)重新封装 |
| models | → v2 `provider_models`(含计价,见下) |
| 路由解析 | 每个 v1 model 合成一个 v2 route(route 名 = 模型名)+ 成员;聚合入口 `/v1/...` 按模型名解析与 v1 一致 |
| 转换规则 | 按 provider 的**渠道默认**重新生成 v2 `routing_rules`(见 §6 注意) |
| users / user_keys | 所有用户挂到一个合成的 `default` 组织(v2 用户需要 org);api key 原样保留 |
| user_quotas | → v2 `quotas`(scope=user) |
| user_model_permissions / user_rate_limits | → v2 `route_permissions` / `rate_limits`(model 通配符当 route 通配符) |
| global_settings | → v2 `instance_settings`(host/port/dsn 等运行期项改由 CLI/env) |

**不迁移**(运营数据):usage 计费历史、downstream/upstream 请求日志、文件记录、凭证健康状态。迁移后从 0 开始记账/记日志。

## 6. 已知差异与注意事项

- **未知渠道**:v2 没有的 v1 渠道(例如旧的 `chatgpt`)会照常迁移 provider/凭证,但**不会**生成路由规则,启动日志会 WARN 列出。需在 Console 里把该 provider 的 channel 改成 v2 支持的渠道后才能工作。
- **转换规则用渠道默认**:v1 `providers.routing_json` 的词表与 v2 不同,故迁移**不逐条翻译**,而是按渠道默认重建 v2 `routing_rules`。若你在 v1 手改过某 provider 的 routing,迁移后需在 Console 复核。
- **计价收敛**:v1 的分档/flex/scale/priority 计价被收敛为 v2 的单档 `{input,output,cache_read,cache_creation}`(取第一档基础价)。请在 Console 核对计价。
- **同名模型跨 provider**:v1 是"后加载者胜",v2 会合成一个**多成员 route**(自动负载均衡)——行为更强,不丢功能。

## 7. 验证

启动后在 Console 检查:providers / models / users / keys 数量与 v1 一致;用一个迁移过来的 user key 打一次聚合 `/v1/...` 模型调用看是否正常解析。

## 8. 回滚

迁移不可逆地把 `gproxy.db` 变成了 v2,但 v1 原库完好保留:

```bash
systemctl stop gproxy
rm -f data/gproxy.db data/gproxy.db-wal data/gproxy.db-shm
mv data/gproxy.db.v1.bak data/gproxy.db
# (如有)同时还原 .v1.bak-wal / .v1.bak-shm 为 gproxy.db-wal / -shm
# 换回 v1 二进制后启动
```

## 9. 升级到 2.1 之前

本迁移代码在 2.1 版本移除。务必在升级到 2.1 之前完成 v1→v2 迁移;2.1 起 v2 将不再识别 v1 库。
