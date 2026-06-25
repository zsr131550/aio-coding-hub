# 插件运行时说明

这里解释插件运行时如何执行，以及当前哪些能力已经开放。普通社区插件优先使用 `declarativeRules`；只有确实需要代码执行时，才阅读 WASM 或进程运行时说明。

- [WASM 运行时](./wasm.md)：WASM ABI v1、`PLUGIN_RUNTIME_DISABLED`、资源限制和失败策略。
- [进程运行时 PoC](./process-poc.md)：默认关闭的 JSON-RPC over stdio 进程隔离设计。
- [流式响应插件](./streaming.md)：`gateway.response.chunk`、sliding window 和 `stream.modify` 的边界。

声明式规则属于插件作者最常用的社区运行时，契约文档在 [Declarative Rules](../reference/declarative-rules.md)。

## Host Runtime Lifecycle

This is a host-owned lifecycle. Plugins declare runtime type and hooks in Plugin API v1, but they do not create, retain, dispose, or inspect runtime instances directly.

0.62.3 treats runtime lifecycle as a host-owned internal contract:

1. **Load**: parse and validate runtime artifacts under package limits.
2. **Execute**: run a bounded hook invocation with timeout and mutation checks.
3. **Retain**: keep only runtime caches that correspond to the current enabled plugin snapshot.
4. **Dispose**: clear runtime caches when plugins are disabled, updated, uninstalled, or when the gateway plugin snapshot is replaced.

Community `declarativeRules` and official `native:privacyFilter` are the only runtimes wired into gateway execution. WASM remains policy-gated, and process runtime remains PoC-only until both are routed through the same lifecycle registry with memory, IO, timeout, and shutdown guarantees.

Hook execution evidence is written to `plugin_hook_execution_reports`. These reports let the GUI and developer workflow show duration, status, timeout, circuit state, budget rejection, mutation summary, and replayability without exposing a new plugin-callable diagnostics API.

The lifecycle boundary exists to prevent long-lived plugin state from outliving its installed snapshot. Any future runtime that allocates memory, opens handles, or starts child processes must implement the same Load / Execute / Retain / Dispose contract before it can participate in gateway execution.

## Release Guard

0.62.3 keeps Plugin API v1 externally stable while hardening host runtime internals. Run `pnpm check:plugin-hardening` before release branches that touch plugin runtime loading, hook context budgets, output mutation budgets, SDK validation, or manifest/runtime documentation.
