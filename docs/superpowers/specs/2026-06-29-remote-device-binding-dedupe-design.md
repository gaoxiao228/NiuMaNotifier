# 远程设备绑定去重设计

## 背景

当前远程控制台会出现同一台本机设备被多次绑定后的重复记录，并且这些记录都显示离线。排查后确认，重复记录不是前端渲染问题，而是服务端 `devices` 表中确实存在多条 active 设备。

根因分为两部分：

- 本机每次发起桌面登录绑定时都会重新生成 `DeviceInstallId`，导致每次计算出的 `device_fingerprint` 都不同。服务端虽然按 `user_id + fingerprint_hash + active` 做 upsert，但不同指纹会被当作不同设备插入。
- 控制台在线状态来自 Redis presence。绑定成功只代表账号和设备记录建立，不代表本机 agent 已经连接 `/ws/device` 并发送 `device.hello` / `device.heartbeat`。如果 agent 还在 30 秒轮询等待中，控制台会短暂显示离线。

## 目标

- 同一台本机设备重复登录绑定时，只更新同一条服务端设备记录，不再生成多条 active 设备。
- 服务端通过数据库约束兜底，避免并发绑定插入重复 active 设备。
- 本机绑定成功后尽快触发 remote agent 连接，让控制台在线状态更快刷新。
- 对已有重复历史记录提供手动清理脚本，由用户确认后执行，不自动猜测合并。

## 非目标

- 不在 Web 控制台前端按名称隐藏重复设备。前端去重会掩盖真实数据问题，且可能导致连接旧设备 ID。
- 不自动合并所有同名设备。不同机器可能使用相同默认名称，自动合并存在误伤风险。
- 不引入完整设备管理后台。本次只处理重复绑定、在线状态刷新和历史数据清理。

## 方案概览

采用四段式修复：

1. 本机持久化稳定的 `DeviceInstallId`。
2. 服务端新增 active 设备唯一约束。
3. 绑定成功后通知 remote agent 立即重试连接。
4. 新增手动历史清理脚本，默认 dry-run，显式 `--apply` 才修改数据库。

## 本机稳定设备身份

本机新增一个设备安装 ID 存储能力：

- 第一次绑定时，如果本地没有安装 ID，则生成 32 字节随机值并保存。
- 后续绑定时读取同一个安装 ID。
- `device_fingerprint` 继续由 `server_url + install_id` 派生。这样同一台机器在不同自托管服务端仍是不同设备，在同一个服务端则稳定为同一设备。

建议存储位置复用现有应用配置目录，文件名为 `remote-device-install-id.json`。内容只保存安装 ID 的 hex 字符串，不保存 token。

示例结构：

```json
{
  "version": 1,
  "install_id": "64-byte-hex-string"
}
```

代码边界：

- `crates/niuma-core/src/remote/device_identity.rs` 负责 `DeviceInstallId` 的解析、序列化和指纹派生。
- `crates/niuma-core/src/store/config_files.rs` 或相邻小模块负责从配置目录读写安装 ID。
- `src-tauri/src/remote/login_flow.rs` 发起登录时调用“读取或创建安装 ID”，不再每次 `DeviceInstallId::generate()`。

## 服务端绑定幂等

服务端当前 `upsertDevice` 会先查询 active 设备再更新，没有数据库唯一约束。需要增加数据库兜底：

- 对 active 设备建立部分唯一索引：`user_id + fingerprint_hash`。
- 如果插入时遇到唯一冲突，应更新已有 active 设备的 token、名称、公钥、能力和更新时间。

PostgreSQL 目标索引：

```sql
CREATE UNIQUE INDEX devices_active_user_fingerprint_unique
ON devices (user_id, fingerprint_hash)
WHERE status = 'active';
```

仓储层应避免“先查再插”的竞态窗口。实现时优先使用 Drizzle 支持的 `onConflictDoUpdate`；如果部分索引无法通过 ORM 表达，则使用一条参数化 SQL 完成 upsert。

## 绑定后立即上线

当前 remote agent 循环会在未配置或失败后 sleep 30 秒。绑定成功写入 config 和 credential 后，agent 可能不会立刻连接。

设计上给 remote agent 增加轻量唤醒机制：

- `RemoteAgentStatusHandle` 或新建 `RemoteAgentControl` 持有一个通知通道。
- agent 在 sleep 时同时等待通知。
- `poll_remote_login` 成功应用绑定后发送 wake。
- wake 只表示“重新读取配置并尝试连接”，不携带 token。

这样绑定成功后，设备应在数秒内建立 `/ws/device`，服务端写入 Redis presence，Web 控制台刷新后显示在线。

## 历史重复设备清理脚本

新增手动脚本用于清理已经存在的重复设备。脚本面向开发/运维，不挂 Web 管理接口。

命令形态：

```bash
npm run devices:dedupe -- --user-email user@example.com --keep latest --dry-run
npm run devices:dedupe -- --user-email user@example.com --keep latest --apply
```

默认行为：

- 只处理指定用户。
- 只处理 `status = 'active'` 的设备。
- 按设备 `name` 分组。
- 每组保留 `created_at` 最新的一条。
- 其他记录标记为 `revoked`，写入 `revoked_at` 和 `updated_at`。
- 不删除物理记录，避免破坏历史外键。
- 没有 `--apply` 时只打印计划，不写数据库。

输出应包含：

- 用户邮箱和用户 ID。
- 每个重复设备组的设备名。
- 保留的设备 ID。
- 将 revoke 的设备 ID 列表。
- dry-run/apply 状态。

## Web 控制台表现

修复后：

- 重复点击本机设置页“登录并绑定”，Web 控制台设备列表仍只出现一条对应设备。
- 绑定成功后短时间内刷新列表，应看到该设备在线。
- 如果 agent 连接失败，控制台仍显示离线；此时本机设置页的 remote agent 状态应给出 `server_unreachable`、`reconnecting` 或 `error` 等状态。

本次不新增前端去重逻辑。

## 数据迁移与兼容

迁移顺序：

1. 先修本机稳定 `DeviceInstallId`。
2. 再加服务端唯一索引和 upsert 兜底。
3. 对已有重复数据，先运行 dry-run 清理脚本。
4. 用户确认后运行 apply。

如果数据库中已经存在相同 `user_id + fingerprint_hash + active` 的重复数据，唯一索引迁移会失败。当前已知重复数据的 fingerprint 不同，但实现迁移前仍应检查并给出清晰错误。

## 测试与验收

自动测试：

- 本机 `DeviceInstallId` 首次创建后可重复读取，重新发起绑定生成相同 fingerprint。
- 不同 `server_url` 使用同一安装 ID 生成不同 fingerprint。
- 服务端同一用户同一 fingerprint 重复绑定只保留一条 active 设备，并更新 token/name/capabilities。
- 清理脚本 dry-run 不修改数据库。
- 清理脚本 apply 只 revoke 指定用户同名重复设备中的旧记录。
- agent wake 后会跳出等待并重新读取配置。

手动验收：

1. 启动 Docker 远程服务端。
2. 在本机设置页连续执行两次“登录并绑定”。
3. 打开 `http://127.0.0.1:27880/` 登录 Web 控制台。
4. 设备列表中同一台设备只出现一条。
5. 等待数秒后刷新，设备显示在线。
6. 对已有重复数据先运行 dry-run 清理脚本，确认输出后再运行 apply。

