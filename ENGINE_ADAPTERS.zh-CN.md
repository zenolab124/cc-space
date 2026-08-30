# Monet Engine Adapter 指南

[English](ENGINE_ADAPTERS.md)

Monet 的引擎层把 coding agent 的协议差异限制在 adapter 内。一个新引擎应只承担自身协议、进程和数据映射成本，不应迫使档案馆、搜索、工作台、通知或存储再增加平台分支。

## 核心模型

- `EngineInstanceId`：一个引擎的具体安装或数据源实例。
- `ProjectRef` / `SessionRef`：`EngineInstanceId + nativeId` 组成的全局身份；`nativeId` 永远按不透明字符串处理。
- `SessionSource`：项目、会话、时间线、搜索文档、附件、会话动作与变更通知。
- `AgentRuntime`：新建/恢复会话、开始 turn、在运行中接收输入、中断 turn、审批响应和关闭。
- `EngineCapabilities` / `SessionActions`：决定 UI 展示什么，不允许共享 UI 根据引擎名猜能力。
- Facet：资产、自动化、配置、配额、运行命令和模型目录等可选扩展。

中立时间线由 `ConversationRecord` 和 `Segment` 组成。adapter 必须映射成 `text`、`reasoning`、`toolCall`、`toolResult`、`commandExecution`、`fileChange`、`attachment` 或有界的 `unknown`，不得把供应商 wire format 直接穿透到前端。

## 新增 adapter

1. 在 `src-tauri/src/engines/<engine>/` 内实现 locator、协议 client/supervisor、source、runtime 和 adapter。
2. 为默认实例创建稳定的 `EngineInstanceId`，实现 `EngineAdapter::descriptor`、`health` 和 `session_source`；支持运行时或 facet 时再返回对应 provider。
3. 在 `src-tauri/src/engines/system.rs` 通过 `register_configured_adapter` 静态登记 descriptor 与惰性构造函数。生产构建不注册 `FixtureEngine`。
4. 将所有供应商对象映射为 Core 类型。共享 command 和事件已经存在，不新增 `<engine>_*` 顶层 IPC。
5. 为协议 schema、未知字段、分页、错误、身份隔离和运行时事件补测试。

最小骨架：

```rust
impl EngineAdapter for ExampleEngine {
    fn descriptor(&self) -> EngineDescriptor { self.descriptor.clone() }
    fn health(&self) -> EngineFuture<'_, EngineHealth> { /* ... */ }
    fn session_source(&self) -> &dyn SessionSource { &self.source }
    fn runtime(&self) -> Option<&dyn AgentRuntime> { Some(&self.runtime) }
}
```

## UI 集成

新增引擎默认使用结构化 UI 身份和标准会话界面：

```rust
ui: EngineUiIntegration {
    identity: UiIdentityMode::Structured,
    session_surface: SessionSurface::Standard,
    install_guide_url: Some("https://example.com/install".into()),
    configuration_guide_url: Some("https://example.com/config".into()),
}
```

标准界面只消费 descriptor、`SessionActions`、中立时间线与统一 runtime 事件。仅当一个第一方引擎已有不可替代的专属界面时才使用 `SessionSurface::Native`；这不是给共享组件增加引擎名判断的出口。

`sendWhileRunning` 只表示上层能否在运行中继续接收用户输入，不规定投递方式。adapter 可以把输入注入当前 turn，也可以排队到下一 turn；共享 UI 统一呈现为“发送”，不得暴露供应商协议术语。

`forkWithCwd` 仅在 adapter 能从既有历史创建新会话、并可靠地把新会话运行目录设为调用方指定的 cwd 时声明。普通 `fork` 不隐含跨目录能力。

引擎启用状态存于 Monet 自身设置。关闭的 adapter 仍出现在引擎清单中，但不会被构造，也不会订阅 source、启动 watcher、轮询或拉起常驻进程；重新启用在应用重启后生效。若 adapter 的变化来自外部旧 watcher，通过 `EngineAdapter::notify_source_change` 送回 adapter 自身的 `subscribe_changes` 通道，不要从 watcher 直接发顶层 Tauri 事件。

新增用户可见文本只需同步 `src/locales/zh-CN.json` 与 `src/locales/en-US.json`。引擎品牌名与安装/原生配置指引来自 descriptor，能力名和通用状态走 i18n。

## 数据与安全约束

- 原始 transcript、rollout 或数据库只读；标题、标签、收藏、软删除等全部写入 Monet 自己的 metadata。
- 外部进程必须使用对应 locator 或增强 PATH，不能依赖开发终端继承的 PATH。
- 本地协议优先使用 stdio，不额外开放网络监听。
- 未知审批请求不得自动允许；日志不得记录凭据、完整环境变量或未截断 payload。
- 附件按 opaque `AssetRef` 路由并按需读取；大 payload 必须设上限。
- source 变更要发 `SourceChange`，搜索分片才能精确失效。
- 健康诊断导出必须脱敏路径与错误文本；descriptor 只放稳定静态事实，安装、登录、版本和握手状态都放 `EngineHealth`。

## 验收清单

- 相同 native project/session ID 在两个实例间不串元数据、搜索、工作台、Runner、通知或附件。
- 另一个引擎未安装、协议损坏或进程退出时，现有引擎仍可使用。
- source 分页游标稳定；无 cwd 会话有稳定的未归类项目。
- runtime generation/sequence 有序；旧 generation 晚到事件被丢弃，sequence gap 可由快照收敛。
- 流式事件批量跨 IPC；空闲 supervisor 不高频唤醒；重试有上限和 jitter。
- 未知 item 安全降级，原始超大 payload 不进入 IPC。
- `FixtureEngine` 的 source/runtime 契约测试全部通过。
- 新 adapter 不增加顶层 command/event，不修改 metadata/search/workbench schema，不在共享组件中增加引擎名分支。

常用检查：

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib 'engines::' --locked
pnpm build
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets --locked -- -D warnings
```

若引擎依赖可生成的正式 schema，再增加显式 opt-in 的安装版 smoke test，并同时验证最低支持版本与当前版本。
