# Monet — AI 排障与提报指南

> 本文档写给替用户诊断 Monet 问题的 AI agent。先走**自诊断**；问题扛过了自诊再提 bug。安装与配置见 [install.zh-CN.md](install.zh-CN.md)。（English: [troubleshoot.md](troubleshoot.md)）

## 诊断基础

| 要什么 | 怎么拿 |
|--------|--------|
| Monet 版本 | `defaults read /Applications/Monet.app/Contents/Info.plist CFBundleShortVersionString` |
| macOS 版本 / 架构 | `sw_vers -productVersion` 与 `uname -m`（须为 `arm64`） |
| Claude Code CLI | `claude --version` |
| 安装方式 | `brew list --cask --versions monet` 成功 → Homebrew，否则为直装 `.dmg` |
| app 在跑吗 | `pgrep -x Monet` |
| 后台服务 | `launchctl list \| grep io.github.zenolab124.monet` |

日志位置（都在 `~/.monet/` 下）：

- `tray.log` — 菜单栏 helper
- `proc-logs/<session-id>/` — 会话级进程日志
- `agent-logs.json` — app 内 AI 任务调用（标题、翻译、摘要）

## 常见问题

**启动被拦（「已损坏」/ 无法验证开发者）。** Gatekeeper 对已签名未公证 app 的反应。修复：`xattr -cr /Applications/Monet.app` 后重开。一次性操作；之后应用内更新静默完成。

**看不到会话 / 项目。** Monet 默认读 `~/.claude/projects/`。确认目录存在且有内容。用户若迁移过 Claude Code 数据（`CLAUDE_CONFIG_DIR`），在 `~/.monet/settings.json` 设置 `claudeRoot`（见 install.zh-CN.md）后重启。从没跑过 Claude Code 的机器上档案馆是空的——正常。

**菜单栏图标不见了。** 查 `launchctl list | grep io.github.zenolab124.monet.tray`。重启 app（启动时会重新注册 tray）。查 `~/.monet/tray.log` 找报错。

**定时任务不执行。** Routine 由 launchd 执行（`io.github.zenolab124.monet.routine.<id>`），其权限账本与主 app **分离**——执行体是 `monet-routine-runner`。首次授权在任务真实运行时经系统弹窗完成；弹窗被拒后 macOS 不会再问——用户须在系统设置 → 隐私与安全性中删掉旧的 `monet-routine-runner` 条目再触发。设置页的权限体检面板会经真实 launchd 链路自测。

**权限型功能静默失效**（在终端恢复、UI 自动化、屏幕观察）。打开设置 → 权限体检：逐项显示已授/未授与修复方法。除非面板指引无效，不要建议 `tccutil reset`——它会清掉 app 全部授权。

**用量/额度数字看起来不新鲜。** 用量 API 限流很凶；Monet 选择退避（约 15 分钟）而不是硬怼。等待即是修复。数字冻结数小时以上才算 bug。

**更新中途失败。** Homebrew：`brew upgrade --cask monet`。直装：从 Releases 下载最新 `.dmg` 替换 app；`~/.monet/` 数据不受影响。

## 提 bug

自诊断的结论是「这是软件缺陷」时就提 issue——你能写出比手填模板好得多的报告。

**1. 收集**（用上面的诊断基础）：Monet 版本、macOS 版本 + 架构、CLI 版本、安装方式、现象 vs 预期、最小复现步骤、以及*相关的*日志行（不要整个文件）。

**2. 脱敏——硬规则，任何内容离开这台机器前执行：**

- `/Users/<名字>` 一律替换为 `~`。
- 绝不包含：API key 或 token、`settings.json` 里的渠道名/端点、会话对话内容、用户会话史中的项目名或路径。
- 每一行日志摘录都先读懂再放入；读不懂的行宁可丢弃，不要盲贴。

**3. 用户过目。** 把最终的 issue 标题和正文给用户看、拿到明确同意——你是在替他公开发言。

**4. 提交**——按顺序选第一个可用的通道。正文两种通道都按仓库 bug 模板组织：Monet 版本 / macOS 版本 / CLI 版本 / 现象 / 复现步骤 / 预期行为 / 日志。

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
