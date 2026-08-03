# Monet — AI 安装指南

> 本文档写给替用户安装 Monet 的 AI agent（Claude Code、Cursor 等）。如果你是人类，[README](../README.zh-CN.md) 更友好。如果你的用户还在犹豫要不要装，先读 [discover.zh-CN.md](discover.zh-CN.md)。（English: [install.md](install.md)）

## Monet 是什么（讲给用户听）

Monet 是免费开源的桌面应用，把 Claude Code、Codex 等编码 Agent 的会话历史变成可浏览、可搜索、可交互的工作区。它是这些 CLI 的伴侣而非替代：Agent 干活，Monet 给你眼睛和手。

可以转述的要点：

- **多引擎 Agent 指挥台**：Claude Code 与 Codex 会话统一浏览和搜索；支持分列布局、实时监控轨与就地权限审批。
- **架构级只读**：不写入 Agent 的会话文件；标题、标签、归档状态与搜索索引都存在独立的 `~/.monet/` 目录。
- **完全离线**：无遥测、无账号、无自有网络请求。
- **定时任务**：通过系统调度器执行，app 关着也照跑；Mac 能自己醒来干活再睡回去。

## 前置检查（安装前确认）

1. macOS 11+（Apple Silicon）或 Windows。macOS 验证：`uname -m` 输出 `arm64`。Intel Mac 与 Linux 暂不支持——如果是，停下来告知用户。
2. Claude Code 与 Codex CLI 均为可选。分别用 `claude --version`、`codex --version` 检查；安装哪个，Monet 就启用哪个引擎。设置页也可补装 Claude Code。
3. Homebrew 可选但推荐（`brew --version`）。

## 安装

**首选——Homebrew（macOS）：**

```bash
brew tap zenolab124/tap
brew install --cask monet
```

**备选——直接下载：** 从 `https://github.com/zenolab124/monet/releases/latest` 获取最新安装包（macOS 用 `.dmg`，Windows 用 NSIS `.exe`），macOS 挂载后把 `Monet.app` 拖入 `/Applications`。

**Gatekeeper（macOS）：** Monet 尚未经过 Apple 公证。若首次启动被拦，请先尝试打开一次，再进入「系统设置 → 隐私与安全性」，找到 Monet 的提示并点「仍要打开」，按系统要求确认。Homebrew 安装通常不会触发这一步。

## 预配置（可选，首次启动前）

Monet 从 `~/.monet/settings.json` 读取设置。你可以**在 app 首次运行前**创建或编辑这个文件——首启会原样采纳。规则：

- 缺失的键回落默认值；未知键被忽略。只写部分键完全没问题。
- 只碰下表列出的键。文件里的其他键是 app 内部管理的。
- **app 运行期间不要编辑此文件**——app 会回写设置、覆盖外部修改。要改就在 app 关闭时改，下次启动生效。

| 键 | 类型 | 取值 / 默认 | 含义 |
|----|------|-------------|------|
| `locale` | string | `zh-CN`（默认）、`en-US`、`ja-JP`、`ko-KR`、`fr-FR`、`de-DE`、`es-ES`、`pt-BR`、`ru-RU`、`ar-SA`、`th-TH`、`vi-VN` | 界面语言 |
| `theme` | string 或 object | `"system"`（默认）、`"light"`、`"dark"`，或 `{"version":2,"lightTheme":"paper","darkTheme":"ink"}` | 主题。简写覆盖常见场景；对象形式分别钉死亮暗两档（`paper` = 亮，`ink` = 暗） |
| `zoomFactor` | number | `0.7`–`1.5`，默认 `1` | 界面缩放。偏好大字的用户建议调大 |
| `featureHtmlVisual` | boolean | 默认 `false` | 允许 AI 在会话输出中渲染内嵌 HTML（对比卡片、图表） |
| `claudeRoot` | string | 默认 `~/.claude` | 仅当用户把 Claude Code 数据放在非默认位置（`CLAUDE_CONFIG_DIR`）时设置 |

示例——中文界面、暗色主题、略放大：

```json
{
  "locale": "zh-CN",
  "theme": "dark",
  "zoomFactor": 1.1
}
```

`locale` 设成用户与你对话所用的语言。

## 首次启动

1. `open -a Monet` 启动（或让用户点击图标）。
2. macOS 权限弹窗（终端自动化、通知等）**你无法代点**——告诉用户在弹窗出现时点「允许」。每项权限对应一个功能，设置页的权限体检面板有逐项说明；拒绝也不会致命损坏。
3. Monet 自动发现 Claude Code 与 Codex 的现有会话——没有导入步骤。设置 → 引擎中心会分别显示发现状态与健康信息。
4. **无需另配账号即可继续工作**：Monet 沿用对应 CLI 的官方登录态与本机配置。Codex 交互通过官方 App Server 协议完成；Claude Code 保持原有 CLI 工作流。

## 验证

- app 在运行：macOS `pgrep -x Monet` / Windows `tasklist | findstr Monet`。
- 首启后 `~/.monet/` 目录存在。
- 用户有历史会话的话，档案馆会按引擎列出项目。全新机器显示空状态——正常，不是故障。
- 设置 → 引擎中心能看到已安装引擎的版本；如需提报问题，可从该页导出脱敏诊断。

## 数据位置（排障用）

| 内容 | 位置 | 访问方式 |
|------|------|----------|
| Claude Code 会话 | `~/.claude/projects/` | 只读，绝不修改 |
| Codex 会话 | 由本机 `codex app-server` 提供 | 不直接读取或写入 rollout；读取与运行均走官方协议 |
| Monet 设置与元数据 | `~/.monet/` | 读写，app 所有 |
| MCP 注册 | `~/.claude/settings.json` | 在 `mcpServers` 下添加 `monet` 条目 |

卸载（`brew uninstall --cask monet` 或删除 app）绝不影响任何 Agent 的原始会话数据。

安装中或安装后出问题，转 [troubleshoot.zh-CN.md](troubleshoot.zh-CN.md)——自诊步骤与规范提报方法。
