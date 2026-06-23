# 插件发布

插件包格式是 `.aio-plugin`。它本质上是一个 zip archive，`plugin.json` 必须位于压缩包根目录，或唯一顶层目录内。

发布检查清单：

- 校验 `plugin.json`。
- 控制 package size 和 entry count。
- 对 package bytes 计算 `sha256`。
- 通过可信 index 发布时，用 Ed25519 签名 release metadata。
- 对新增权限、runtime/hook/config 变化和 breaking update 写清升级与 rollback 说明。

当前实现支持本地/离线包导入、受约束的远程 `.aio-plugin` 下载、checksum/signature verification、更新时的 permission delta 检查、已撤销插件 quarantine，以及 rollback snapshots。

## 0.62.2 生命周期行为

0.62.2 把安装、更新、回滚和隔离相关信息收口为宿主侧 lifecycle explanation layer。它不改变 `plugin.json` v1 的字段形状，也不新增插件可调用 API。

### 安装预检

用户从 Plugins 页面导入本地 `.aio-plugin` 时，宿主会先读取包并展示安装预检。预检会说明插件身份、来源、runtime、hooks、permissions、兼容性、checksum/signature、已有安装覆盖关系、warnings 和 blocking reasons。

预检通过不等于安装已经完成。用户确认后，真实安装仍会重新执行包解压安全检查、manifest 校验、checksum/signature verification、host compatibility、runtime policy 和权限策略。发布者应该把预检视为“给用户解释将要发生什么”，而不是绕过安装校验的入口。

### 更新差异

本地更新前，宿主会比较当前已安装版本和待安装包，展示：

- version direction。
- runtime change。
- hook added/removed/changed。
- permission unchanged granted、unchanged pending、added pending、removed。
- `configVersion` change。
- compatibility 和 trust change。
- 当前版本是否可回滚。

新增权限必须进入 pending。发布者可以在 release notes 中提前说明新增权限的原因，但宿主不会因为插件升级而静默授权新权限。

### 回滚与隔离

更新会保留可回滚的历史快照。rollback 只允许回到仓库已记录且仍可用的历史版本；如果当前版本的安装目录已经缺失，宿主会在更新预览里标记不可回滚。

撤销或宿主判定危险的插件会进入 `quarantined`。隔离插件不能启用；用户需要卸载、回滚到可用版本，或等待可信来源发布新版本。隔离和回滚会保留 audit 记录，便于追踪生命周期状态变化。

远程包安装刻意保持窄能力：

- 下载 URL 必须是无凭据的 `https://` 或 `file://`。
- artifact path 必须以 `.aio-plugin` 结尾。
- 包在解压前会受到大小限制。
- remote 和 GitHub release install 必须提供 checksum。
- 如果同时提供 signature 和 trusted public key，宿主会校验 Ed25519 signature。

开发者工具输出 base64 编码的 Ed25519 signature。Public key 是原始 32-byte Ed25519 public key 的 base64 编码，和宿主 verifier 输入保持一致。
