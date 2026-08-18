<p align="center">
  <img src="src-tauri/icons/128x128@2x.png" width="128" height="128" alt="Monet">
</p>

<h1 align="center">Monet</h1>

<p align="center">
  Coding Agent 的多引擎指挥台——Claude Code 与 Codex 已同席
</p>

<p align="center">
  <a href="README.en.md">English</a>
</p>

<p align="center">
  <a href="LICENSE"><img src="https://img.shields.io/github/license/zenolab124/monet" alt="License"></a>
  <a href="https://github.com/zenolab124/monet/releases/latest"><img src="https://img.shields.io/github/v/release/zenolab124/monet" alt="Latest Release"></a>
  <a href="https://github.com/zenolab124/monet/actions/workflows/ci.yml"><img src="https://github.com/zenolab124/monet/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
</p>

<p align="center">
  <img src="https://img.shields.io/badge/platform-macOS%20%7C%20Windows-purple" alt="platform">
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
  懒得读？把这行丢给你的 AI，它会替你判断、安装、排障：<br>
  <code>读取 https://raw.githubusercontent.com/zenolab124/monet/main/llms.txt</code><br>
  <sub>如果你就是那个 AI：读取上面的 llms.txt，按其中指引为你的用户服务。</sub>
</p>

<p align="center">
  <img src=".github/assets/workbench-hero.webp" alt="Monet 赛马工作台——四个 Agent 并行创作并实时对比结果" width="920">
</p>

## Monet 是什么？

你和 Coding Agent 的对话散落在不同终端、项目和会话记录里——想回看要翻，想并行要切，跑长任务还得反复回来确认进度。

Monet 把 Claude Code、Codex 等引擎收进一面墙：所有 Agent 会话可浏览、可搜索、可并行指挥。Agent 干活，Monet 给你眼睛和手。

## 为什么选 Monet？

**引擎再多，一面墙。** Claude Code 与 Codex 共用一个档案馆、一套搜索和一面工作台，每个会话带引擎徽标随时分辨。赛马模式让同一道题跨引擎对拼，定时任务也能自由选择执行引擎——选引擎就像选模型一样自然。

**像看盘一样指挥你的 Agent。** 不限列数的并行会话横向铺开，监控轨上的状态、输出和 token 一眼可见；权限审批、回答提问、失败重试，都能在卡片上直接完成。

**多渠道玩家的家。** 官方订阅、第三方 API、自建代理、本地模型——不同会话各用各的，聊到一半也能切换。强模型负责攻坚，便宜渠道处理杂活，连接与模型始终清楚可见。

**数据主权在你手里。** Claude Code JSONL 与 Codex rollout 架构级只读，Monet 的标题、标签、收藏等增值数据独立存放在 `~/.monet/`。本地会话能力离线运行、零遥测、无 Monet 账号体系；只有订阅额度和你主动使用的智能增强会访问相应供应商服务。

## 功能

### 多引擎系统——Claude Code 与 Codex 同席

- Claude Code 与 Codex 的项目、会话和时间线统一进入档案馆、搜索、工作台与通知，并可按引擎筛选
- 无需安装对应 CLI 也能浏览已有历史；安装并登录 CLI 后，可在 Monet 中新建、恢复和持续运行会话
- 两个引擎共用熟悉的会话外壳与输入体验，界面只展示当前引擎真正支持的操作
- 「引擎中心」分别展示安装、认证、版本、能力和诊断；一个引擎故障不会拖垮另一个，新引擎也可按统一契约继续接入

### 工作台——并行 Agent 指挥

- 列数不设上限，屏幕放不下就横向滚动，滚轮如原生触控板般顺滑
- 监控轨总览全部会话：实时状态、尾部输出、token 用量；审批/重试/回答，卡面直接点
- 权限请求变 GUI 卡片：危险命令红色警示、AI 用人话批注风险，`Enter` 放行 `Esc` 拒绝
- **赛马模式**：同一个问题广播给不同引擎、模型或渠道，答案与成本并排见分晓
- **全景图导出**：一键把整面工作台截成一张全景图，分享或存档当前战况

<p align="center">
  <img src=".github/assets/workbench-columns.webp" alt="Monet 多列工作台——会话监控、多列并行、内嵌 HTML 渲染与实时后台任务" width="920">
</p>

### 会话运行——CLI 会话开进 GUI

- 字符级真流式，首字延迟缩到 API 首个 token
- **渠道 / 模型 / 思考强度全程热切换**——不重启进程、不断上下文
- 文件账本：这个会话动了哪些文件、每次 diff、一键跳回「AI 当时为什么改它」
- Runner 托管 dev server：日志就在会话旁边，报错一键喂给 AI
- 终端里跑的会话自动识别、实时跟看，也能从 Monet 里终止它

### 阅读体验——运行与回看同一套引擎

- 工具调用三档显示：完整卡片、逐项折叠、连续过程合并；展开后仍保留 Edit 并排 diff、Bash 分区复制等专属能力
- 回复内直接渲染 HTML/SVG：对比卡片、表格、示意图，不再是文字墙；LaTeX 数学公式原生渲染
- **交付物就地预览**：Agent 在回复里链接的本地 HTML、SVG、图片自动变成预览卡片，点击放大，无需任何额外配置；HTML 默认在禁脚本、禁联网的隔离环境中静态加载
- 锚点导航 + 提问吸顶 + 回底浮标，几百轮的长会话穿梭自如
- 每轮 token 四联、上下文用量条快满预警——钱花在哪，实时看得见

### 档案馆与搜索

- 零导入：装完即见全部历史，按项目陈列
- 毫秒级全文搜索，中文无分词障碍；记不清关键词就用自然语言问，AI 归纳出答案
- AI 自动生成标题、标签、摘要——当然，都存在 JSONL 之外

### 自动化

- 「每天早上九点总结昨天的会话」——自然语言直接变成定时任务
- 睡眠中唤醒执行、跑完自动回睡；一次授权，全程静默
- Hooks 真实运行统计：配置了 ≠ 在工作，近 7 天每条跑没跑，这里看真相

<p align="center">
  <img src=".github/assets/routines.webp" alt="定时任务——自然语言指令与 cron 时间计划，系统级调度" width="920">
</p>

### 常驻信息面

- 菜单栏订阅额度：Claude Code 与 Codex 按 Provider 分区纵向展示，保留上游实际提供的窗口、分组与重置倒计时；主 app 关了照样显示
- 桌面小组件：连续活跃、token 脉搏、28 天热力图、模型分布
- 成本估算分四类 token 计价，未知模型如实标「未计价」，绝不瞎猜

<p align="center">
  <img src=".github/assets/desktop-widgets.webp" alt="桌面小组件——连续活跃、token 脉搏、28 天热力图、模型分布、费用估算" width="920">
</p>

### AI 增值（BYOAI）

- Monet 不内置模型、不收 AI 费——一切 AI 能力走你自己的渠道与额度，逐项可关，调用有账本
- 内置 MCP server：会话里的 Claude 能直接搜你的历史、建定时任务、查看 Runner 日志
- 12 种界面语言；说出任意语言名，AI 现场把整个界面翻译过去

### 质感

- Paper 设计语言：暖调、哑光、墨上纸；附 Ink 深色主题
- 启动无白闪、ProMotion 高刷、大会话虚拟滚动——细节较真，性能 HUD（`Cmd+Shift+M`）全程透明

<p align="center">
  <img src=".github/assets/permission-checkup-ink.webp" alt="Ink 深色主题下的权限体检——主应用与后台任务两本账，逐项透明" width="920">
</p>

## 安装

**Homebrew**（macOS）：

```bash
brew install --cask zenolab124/tap/monet
```

完整名称会让 Homebrew 6 只信任 Monet 这一项，无需另外执行 `brew trust`。

或从 [Releases](../../releases) 下载：macOS `.dmg` / Windows 安装包。

> macOS 11+（Apple Silicon）享受全部功能；Windows 覆盖核心功能，系统级集成（小组件、菜单栏、睡眠唤醒）为 macOS 专属。

**首次打开**：Monet 已由 Apple 公证，下载后直接打开即可，无需任何额外授权步骤。从早期未公证版本升级的用户不受影响，应用内更新照常进行。

**装完即用，零导入**：本机 Claude Code 与 Codex 的历史数据会被自动发现，即使没有安装对应 CLI，也会直接进入同一个档案馆。安装 Codex CLI 后可增加交互运行能力；各引擎的具体状态可在「设置 → 引擎」查看。

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

### 本地构建

```bash
pnpm tauri build
```

该命令会构建前端、MCP 二进制与 Tauri 主应用；正式分发产物以 [Releases](../../releases) 页面为准。

## 数据与隐私

| 内容 | 位置 | 访问方式 |
|------|------|---------|
| Claude Code 会话 | `~/.claude/projects/` | **只读** |
| Codex 会话 | `$CODEX_HOME/sessions/` 与 `$CODEX_HOME/archived_sessions/`（默认 `~/.codex/`） | **只读；安装 CLI 后运行操作走官方 App Server** |
| Monet 增值数据（标题、标签、定时任务） | `~/.monet/` | 读写 |
| MCP 注册 | `~/.claude/settings.json` | 在 `mcpServers` 下添加 `monet` 条目 |

本地会话能力离线运行、零遥测、无 Monet 账号体系。订阅额度只访问 Claude/Codex 官方服务，并复用官方客户端已有登录态；Monet 不主动刷新或回写 OAuth 凭据。凭据纪律：API token 不进命令行参数、临时文件即用即删。

## 常见问题

> 遇到问题先别翻文档——把这句丢给你的 AI：`读取 https://raw.githubusercontent.com/zenolab124/monet/main/llms.txt 帮我排查 Monet 的问题`。它能自诊常见故障，确认是 bug 还会带上诊断数据帮你提报（无需 GitHub 账号）。

**Monet 会取代 Claude Code 或 Codex CLI 吗？**
不会——它是这些 CLI 的伴侣。干活的是 Agent，Monet 给你眼睛和手；从终端或 Monet 启动的会话都能统一查看。

**我的会话数据安全吗？**
原始会话文件架构级只读，代码开源可查。删掉 Monet，你的 Claude Code 与 Codex 会话数据都不会受影响。

**装完还要配什么吗？**
浏览已有历史无需额外配置；要在 Monet 中新建或继续会话，只需对应 CLI 已安装并登录。多渠道与 AI 增值都是可选进阶。

**首次打开有 Gatekeeper 警告？**
不应该有——正式版安装包已经 Apple 公证，下载即开。如果仍被拦截，说明装的是早期未公证版本，建议直接下载最新版；也可以到「系统设置 → 隐私与安全性」点击「仍要打开」完成一次性授权。

**Windows / Linux？**
Windows 已支持（核心功能完整，macOS 系统集成除外）；Linux 暂无近期计划。

**支持哪些 Agent？**
当前正式支持 Claude Code 与 Codex，后续引擎可按统一契约继续接入。开发者可参阅 [Engine Adapter 指南](ENGINE_ADAPTERS.zh-CN.md)。

## 技术栈

- [Tauri 2](https://tauri.app/) — Rust 后端 + 系统 WebView
- [Vue 3](https://vuejs.org/) + TypeScript + Composition API
- [UnoCSS](https://unocss.dev/) — 原子化 CSS (preset-wind4 + preset-icons)
- [Shiki](https://shiki.style/) — 语法高亮
- [markdown-it](https://github.com/markdown-it/markdown-it) — Markdown 渲染
- [KaTeX](https://katex.org/) — LaTeX 数学渲染
- [vue-i18n](https://vue-i18n.intlify.dev/) — 国际化
- [@dnd-kit/vue](https://dndkit.com/) — 拖拽
- [Swift WidgetKit](https://developer.apple.com/documentation/widgetkit) — macOS 小组件

## 致谢

- 感谢 [LINUX DO](https://linux.do/) 社区的分享氛围与真实反馈
- 感谢所有提交 issue 与建议的早期用户

## 开源协议

[MIT](LICENSE)
