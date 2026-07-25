<p align="center">
  <img src="src-tauri/icons/128x128@2x.png" width="128" height="128" alt="Monet">
</p>

<h1 align="center">Monet</h1>

<p align="center">
  <a href="https://docs.anthropic.com/en/docs/claude-code">Claude Code</a> 的任务控制中心
</p>

<p align="center">
  <a href="README.md">English</a>
</p>

<p align="center">
  <a href="LICENSE"><img src="https://img.shields.io/github/license/zenolab124/monet" alt="License"></a>
  <a href="https://github.com/zenolab124/monet/releases/latest"><img src="https://img.shields.io/github/v/release/zenolab124/monet" alt="Latest Release"></a>
  <a href="https://github.com/zenolab124/monet/actions/workflows/ci.yml"><img src="https://github.com/zenolab124/monet/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/platform-macOS-purple" alt="platform">
  <img src="https://img.shields.io/badge/built_with-Tauri_2-24c8db" alt="Tauri">
  <img src="https://img.shields.io/badge/Vue-3.5-42b883" alt="Vue">
  <img src="https://img.shields.io/badge/Rust-stable-e57324" alt="Rust">
  <img src="https://img.shields.io/badge/TypeScript-6-3178c6" alt="TypeScript">
</p>

<p align="center">
  <a href="#功能">功能</a> •
  <a href="#安装">安装</a> •
  <a href="#从源码构建">构建</a> •
  <a href="#数据与隐私">隐私</a> •
  <a href="#常见问题">常见问题</a>
</p>

<p align="center">
  <img src=".github/assets/workbench-hero.webp" alt="Monet 工作台——多列会话、会话监控、内嵌 HTML 渲染与实时后台任务" width="920">
</p>

## Monet 是什么？

把散落在终端里的 Claude Code 会话，收进一个可浏览、可搜索、可指挥的桌面工作空间。名字取自 [Claude Monet](https://zh.wikipedia.org/wiki/%E5%85%8B%E5%8A%B3%E5%BE%B7%C2%B7%E8%8E%AB%E5%A5%88)——对，就是那个 Claude。

Monet **绝不写入** Claude Code 的 JSONL 文件，所有增值数据存储在独立目录 `~/.monet/`。

## 为什么选 Monet？

- **是任务控制中心，不只是查看器。** 多列分屏 + 实时监控栏 + 就地权限审批，同时指挥多个并行 Agent——像交易员看盘一样掌控全局。
- **数据主权在你手里。** 对 Claude Code 的文件架构级只读、完全离线、零遥测、无账号，Monet 的增值数据全部住在自己的目录里。
- **睡觉时也在干活。** 定时任务由系统调度器执行，Monet 没开着也照跑——Mac 能自己醒来跑任务，跑完再睡回去。

## 功能

### 🖥 工作台 — 并行 Agent 指挥

- 多标签工作区 + 可拖拽分列，5 个会话同时流式输出也互不卡顿
- 监控栏：每个会话的状态、尾部输出、token 用量一眼可见——在卡片上就能批权限、点重试
- **赛马模式**：一键分叉出多条赛道，同一个问题广播给不同模型/渠道，答案质量和 token 消耗并排对比
- 权限请求变成 GUI 卡片：危险命令红色警示，AI 用人话批注风险；`Enter` 放行、`Esc` 拒绝
- 异步任务面板：子 Agent、Workflow、后台任务的实时进度
- 终端里启动的会话自动检测、实时跟看，借官方 CLI hooks 追踪状态（Turn Signal）

### 📖 阅读体验

- 真流式——基于 CLI 字符级增量事件渲染，打字机节奏自适应
- 18+ 种工具调用专属卡片：Edit 显示红绿 diff、Bash 显示命令和退出码，未知工具优雅降级
- 回复内直接渲染 HTML/SVG——对比卡片、表格、示意图，不再是文字墙
- 图片粘贴/拖拽输入，缩略图 + 全屏查看器
- 思考块带字数、长对话锚点圆点导航、提问吸顶、跨天日期分隔线

### 🗄 档案馆与搜索

- 三栏只读浏览：项目 → 会话列表 → 详情；流式中的会话可实时跟看不打扰
- 毫秒级跨项目全文搜索；记不清关键词？Agent 语义搜索听得懂自然语言描述
- AI 自动生成标题、标签、摘要——当然，都存在 JSONL 之外

### 📋 过程透明

- **文件账本**：这个会话到底碰了哪些文件、每次编辑的 diff 时间线、git 只读快照
- 每轮 token 统计、缓存命中率、上下文用量进度条——快满时提前预警

### ⚙️ 自动化与系统集成

- 定时任务由系统调度器（launchd）执行——Monet 没开也照跑
- 睡眠唤醒：过夜任务照常执行；一次授权，此后全程静默
- 菜单栏额度监控：Session/Weekly 用量百分比与重置倒计时实时可见
- 原生 WidgetKit 桌面小组件：连续活跃、Token 脉搏、作息节奏热力图、模型分布等
- 内置 MCP server：在任意 Claude Code CLI 会话里搜索历史、管理定时任务

> 本节的系统级集成多为 macOS 专属；Windows 版聚焦核心功能兼容。

### 🎨 质感与个性化

- Paper 设计语言——暖调、哑光、墨上纸质感，附 Ink 深色主题
- 内置 12 种语言，说出任意语言名，AI 把整个界面翻译过去
- 渠道系统：官方 API、自建代理、OpenAI 协议端点、甚至 Apple 本地模型——按会话切换
- 会话级模型、思考强度、权限模式，顶栏胶囊一点即换

## 安装

**Homebrew**：

```bash
brew tap zenolab124/tap
brew install --cask monet
```

或从 [Releases](../../releases) 下载最新 `.dmg`。

> macOS 11+（Apple Silicon）享受全部功能；Windows 支持策略见[常见问题](#常见问题)。

**首次打开**：Monet 使用稳定签名身份但尚未经过 Apple 公证，首次打开会触发 Gatekeeper 提示。右键应用 → **打开**（仅需一次），或执行：

```bash
xattr -cr /Applications/Monet.app
```

之后的版本更新由应用内静默完成，不再有任何提示。

**🤖 或者交给你的 AI**——它了解你的工作方式，能替你判断合不合适、并把一切装配好。还不确定 Monet 适不适合你？问你的 AI：

```
读取 https://raw.githubusercontent.com/zenolab124/monet/main/llms-discover.zh-CN.md，结合我使用 Claude Code 的习惯，告诉我 Monet 适不适合我。
```

决定要装？AI 会搞定 Homebrew、Gatekeeper，并按你的偏好预配好语言/主题：

```
读取 https://raw.githubusercontent.com/zenolab124/monet/main/llms-install.zh-CN.md 并帮我安装 Monet。
```

（给 Agent 的索引：[`llms.txt`](llms.txt)）

## 从源码构建

### 前置条件

- [Node.js](https://nodejs.org/) 22+
- [pnpm](https://pnpm.io/) 10+
- [Rust](https://rustup.rs/) 1.77+
- Xcode Command Line Tools — `xcode-select --install`

### 开发模式

```bash
git clone https://github.com/zenolab124/monet.git
cd monet
pnpm install
pnpm tauri dev
```

### 发布构建（含小组件 + 签名）

```bash
pnpm release
```

依次执行 `tauri build`、编译 macOS 小组件、嵌入 app bundle、签名、生成 `.dmg`。

建立本机签名身份（推荐——TCC 权限跨构建保持稳定）：

```bash
scripts/setup-signing.sh
```

不跑也能构建，会降级为 ad-hoc 签名——功能正常，但每次重新构建后 TCC 权限需重新授予，小组件可能不注册。

## 数据与隐私

| 内容 | 位置 | 访问方式 |
|------|------|---------|
| Claude Code 会话 | `~/.claude/projects/` | **只读** |
| Monet 增值数据（标题、标签、定时任务） | `~/.monet/` | 读写 |
| MCP 注册 | `~/.claude/settings.json` | 在 `mcpServers` 下添加 `monet` 条目 |

Monet 完全离线运行。无遥测、无账号、无网络请求（除非你主动通过 Claude Code CLI 发送流式消息）。

## 常见问题

> 🤖 遇到问题？把 [`llms-troubleshoot.zh-CN.md`](llms-troubleshoot.zh-CN.md) 丢给你的 AI——它能自诊常见故障；确认是 bug 的话，还会带上完整诊断数据帮你提 issue。

**Monet 会取代 Claude Code CLI 吗？**
不会——它是 CLI 的伴侣。干活的是 CLI，Monet 给你眼睛和双手。两边启动的会话互相可见。

**我的会话数据安全吗？**
Monet 绝不写入 Claude Code 的 JSONL 文件——这是架构保证，不是一个开关。标题、标签等增值数据都在 `~/.monet/`。删掉 Monet，你的 Claude Code 数据毫发无损。

**首次打开为什么有 Gatekeeper 警告？**
Monet 使用稳定签名身份但尚未经过 Apple 公证。右键 → 打开一次（或 `xattr -cr /Applications/Monet.app`），之后应用内更新全程静默。

**为什么需要那些系统权限？**
每项权限只服务对应功能：控制 Terminal 用于「在终端恢复会话」；辅助功能与屏幕录制只在 Agent 任务需要操作界面、截屏观察时用到。设置页有权限体检面板，什么权限、什么状态，一目了然。

**Windows 呢？**
macOS 是第一平台，享受 100% 完整功能。Windows 版走兼容路线：会话浏览、全文搜索、工作台对话这些主体功能保证可用；桌面小组件、菜单栏额度、睡眠唤醒等 macOS 系统集成不在其列。Linux 暂无近期计划。

**会支持 Claude Code 之外的工具吗？**
有可能。Monet 的会话解析与界面层是分开的，未来不排除支持 Codex、OpenCode 等更多 agentic CLI——如果你需要，欢迎开 issue 投票。

## 技术栈

- [Tauri 2](https://tauri.app/) — Rust 后端 + 系统 WebView
- [Vue 3](https://vuejs.org/) + TypeScript + Composition API
- [UnoCSS](https://unocss.dev/) — 原子化 CSS (preset-wind4 + preset-icons)
- [Shiki](https://shiki.style/) — 语法高亮
- [markdown-it](https://github.com/markdown-it/markdown-it) — Markdown 渲染
- [vue-i18n](https://vue-i18n.intlify.dev/) — 国际化
- [@dnd-kit/vue](https://dndkit.com/) — 拖拽
- [Swift WidgetKit](https://developer.apple.com/documentation/widgetkit) — macOS 小组件

## 开源协议

[MIT](LICENSE)
