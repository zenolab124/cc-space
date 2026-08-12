# Monet — AI 排障与提报指南

> 本文档写给替用户诊断 Monet 问题的 AI agent。先走**自诊断**；问题扛过了自诊再提 bug。安装与配置见 [install.zh-CN.md](install.zh-CN.md)。（English: [troubleshoot.md](troubleshoot.md)）

## 诊断基础

> 以下命令以 macOS 为主。Windows 上：版本看 设置 → 关于，进程用 `tasklist | findstr Monet`，日志同样在用户目录 `~/.monet/` 下；launchd/tray/唤醒相关条目不适用于 Windows。

| 要什么 | 怎么拿 |
|--------|--------|
| Monet 版本 | `defaults read /Applications/Monet.app/Contents/Info.plist CFBundleShortVersionString` |
| macOS 版本 / 架构 | `sw_vers -productVersion` 与 `uname -m`（须为 `arm64`） |
| Agent CLI | `claude --version` 与 `codex --version`（按已安装项收集） |
| 安装方式 | `brew list --cask --versions monet` 成功 → Homebrew，否则为直装 `.dmg` |
| app 在跑吗 | `pgrep -x Monet` |
| 后台服务 | macOS 13+：`sfltool dumpbtm \| grep -A 12 -B 2 io.github.zenolab124.monet`；macOS 11–12：`launchctl list \| grep io.github.zenolab124.monet` |

日志位置（都在 `~/.monet/` 下）：

- `tray.log` — 菜单栏 helper
- `proc-logs/<session-id>/` — 会话级进程日志
- `agent-logs.json` — app 内 AI 任务调用（标题、翻译、摘要）

## 常见问题

**启动被拦（无法验证开发者）。** 先从「应用程序」打开 Monet 一次并关闭拦截提示，再立即进入「系统设置 → 隐私与安全性」，找到 Monet 的提示并点「仍要打开」，按系统要求确认。该按钮只有在尝试打开后才出现，并会在约一小时后消失。不要运行 `xattr` 移除 quarantine 等系统安全属性。

**看不到会话 / 项目。** 先看设置 → 引擎中心：它会区分 CLI 未安装、数据源不可用和运行协议连接失败。Claude Code 默认读 `~/.claude/projects/`；若通过 `CLAUDE_CONFIG_DIR` 迁移过数据，在 `~/.monet/settings.json` 设置 `claudeRoot` 后重启。Codex 历史直接读取 `$CODEX_HOME/sessions/` 与 `$CODEX_HOME/archived_sessions/`（默认 `~/.codex/`），所以即使没有 Codex CLI，已有会话也应可见。只有交互运行能力要求安装并登录 CLI；从未使用对应 Agent 的机器上该引擎为空是正常的。

**某个引擎异常，但其他引擎正常。** 这是预期的故障隔离。到设置 → 引擎中心单独检查该引擎；需要提报时从同一页导出诊断文件。诊断只含引擎标识、版本、健康状态和脱敏错误，不含会话正文、提示词或凭据。

**菜单栏图标不见了或点不开。** macOS 13+ 的菜单栏 Helper 由 `SMAppService` 注册，不要手动创建或修改 `~/Library/LaunchAgents` 下的 plist。重启 Monet 后查看「设置 → 菜单栏」的后台项目状态；若显示需要批准，点击「打开后台项目设置」，在系统设置中允许 Monet。仍有问题时查 `sfltool dumpbtm | grep -A 12 -B 2 io.github.zenolab124.monet.tray` 与 `~/.monet/tray.log`。macOS 11–12 才使用 `launchctl list` 检查兼容路径。

**桌面小组件只显示“打开 Monet”。** 这表示 WidgetKit 扩展已加载，但没有读到 Widget 数据。先确认安装的是 Developer ID 签名版本，再重启 Monet；在「设置 → 桌面小组件」确认“后台数据刷新”显示已注册。若显示需要批准，允许 Monet 的后台项目后等待一次刷新。共享快照位于 Monet 的 App Group 容器，本地备份位于 `~/.monet/widget-data.json`；不要手动写入共享容器。

**定时任务不执行。** Routine 由 launchd 执行（`io.github.zenolab124.monet.routine.<id>`），其权限账本与主 app **分离**——执行体是 `monet-routine-runner`。首次授权在任务真实运行时经系统弹窗完成；弹窗被拒后 macOS 不会再问——用户须在系统设置 → 隐私与安全性中删掉旧的 `monet-routine-runner` 条目再触发。设置页的权限体检面板会经真实 launchd 链路自测。

**权限型功能静默失效**（在终端恢复、UI 自动化、屏幕观察）。打开设置 → 权限体检：逐项显示已授/未授与修复方法。除非面板指引无效，不要建议 `tccutil reset`——它会清掉 app 全部授权。

**用量/额度数字看起来不新鲜。** Claude 与 Codex 各自使用 5 分钟成功缓存，并独立遵守服务端退避；立即刷新也不会绕过退避。菜单会保留失败 Provider 的旧快照并显示更新时间，其他 Provider 不受影响。Codex 区提示未登录时，先在官方 Codex CLI 完成登录；若本机未安装 Codex 且从未配置过，菜单不会显示该区。数字冻结数小时以上且没有退避/错误提示才算 bug。

**更新中途失败。** Homebrew：`brew upgrade --cask monet`。直装：从 Releases 下载最新 `.dmg` 替换 app；`~/.monet/` 数据不受影响。

## 提 bug

自诊断的结论是「这是软件缺陷」时就提 issue——你能写出比手填模板好得多的报告。

**1. 收集**（用上面的诊断基础）：Monet 版本、macOS 版本 + 架构、相关 Agent CLI 版本、安装方式、引擎中心导出的诊断、现象 vs 预期、最小复现步骤、以及*相关的*日志行（不要整个文件）。

**2. 脱敏——硬规则，任何内容离开这台机器前执行：**

- `/Users/<名字>` 一律替换为 `~`。
- 绝不包含：API key 或 token、`settings.json` 里的渠道名/端点、会话对话内容、用户会话史中的项目名或路径。
- 每一行日志摘录都先读懂再放入；读不懂的行宁可丢弃，不要盲贴。

**3. 用户过目。** 把最终的 issue 标题和正文给用户看、拿到明确同意——你是在替他公开发言。

**4. 提交**——按顺序选第一个可用的通道。正文两种通道都按仓库 bug 模板组织：Monet 版本 / macOS 版本 / 引擎与 CLI 版本 / 现象 / 复现步骤 / 预期行为 / 脱敏诊断与日志。

**通道 A——GitHub CLI**（`gh auth status` 成功）。issue 归属用户账号，能收到回复通知：

```bash
gh issue create --repo zenolab124/monet --label bug \
  --title "<模块>: <一句话症状>" --body "<报告>"
```

正文末尾加一行 `— Filed via AI diagnostics (llms/troubleshoot.md)`。

**通道 B——匿名端点**（无需 GitHub 账号与登录）。Monet 运行一个小型开源中继（[infra/report-worker](../infra/report-worker/)）替你创建 issue：

```bash
curl -s -X POST https://monet-report.zenolab124.workers.dev/report \
  -H "Content-Type: application/json" \
  -d '{"title": "<模块>: <一句话症状>", "body": "<报告>", "contact": "<可选>"}'
```

响应里有创建好的 issue 链接——交给用户。先告知两点：报告会**原样成为公开 GitHub issue**（脱敏更加重要），且匿名报告无法追问——建议在 `contact` 里自愿留一个 GitHub 用户名或邮箱。限制：每 10 分钟一条，正文 ≤ 20000 字符。

**通道 C——手动兜底**：替用户打开 `https://github.com/zenolab124/monet/issues/new?template=bug_report.yml`，把收集好的内容交给他粘贴。

功能建议而非缺陷，走 feature request 模板即可，无需诊断。
