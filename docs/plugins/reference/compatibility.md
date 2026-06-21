# 插件兼容性

插件兼容性使用 SemVer 描述。宿主安装、启用和升级插件时，会同时检查插件版本、插件 API 版本、应用版本、平台和运行时 ABI。

Manifest 中的关键字段：

- `version`：插件自身发布版本。
- `apiVersion`：该 manifest 使用的插件 API 版本。
- `hostCompatibility.app`：兼容的 AIO Coding Hub 应用版本范围。
- `hostCompatibility.pluginApi`：兼容的 pluginApi 版本范围。
- `hostCompatibility.platforms`：可选的平台 allowlist。

WASM 插件还需要声明 WASM ABI 版本：

```json
{ "kind": "wasm", "abiVersion": "1.0.0" }
```

宿主会拒绝不支持的主版本。未来插件 API 变更必须保持向后兼容；无法兼容时，需要提升主版本并让旧插件继续按旧契约运行或被明确标记为不兼容。

## 0.62 Internal Platform Kernel

Plugin API v1 remains externally compatible in 0.62. 这个版本重组的是宿主内部平台边界：contract metadata、hook descriptors、runtime policy、runtime cache lifecycle 和 provider adapter facades。

0.62 does not add public provider plugin APIs. Provider adapter 仍是 host-internal 设计，用于先降低 gateway/provider 分支扩散和维护成本；未来是否公开 provider 插件 API，需要另行设计版本化契约。

0.62 keeps third-party JavaScript and WebView plugin execution unsupported. 社区插件继续使用 `declarativeRules`；WASM 仍受宿主策略控制，未启用时安装或执行会被拒绝。
