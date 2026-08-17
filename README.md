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
  <img src=".github/assets/workbench-hero.webp" alt="Monet 工作台——多列会话、会话监控、内嵌 HTML 渲染与实时后台任务" width="920">
</p>

## Monet 是什么？

你和 coding agent 的每一次对话，都散落在终端窗口里——关掉就找不回，并行就顾不过来，跑长任务就得干等。

Monet 把 Claude Code、Codex 等引擎收进一面墙：所有 Agent 会话可浏览、可搜索、可并行指挥。Agent 干活，Monet 给你眼睛和手。

## 为什么选 Monet？

**引擎再多，一面墙。** Claude Code 与 Codex 只是开始：同一个档案馆、同一套搜索、同一面工作台墙，每个 Agent 会话带引擎徽标随时分辨。赛马模式能让同一道题跨引擎对拼，定时任务想交给哪个 Agent 跑就交给谁——你选引擎像选模型一样自然。引擎层生来就是为接纳更多引擎设计的，新引擎入席不改变你已有的一切习惯。

**像看盘一样指挥你的 Agent。** 不限列数的并行会话横向铺开，监控轨上每个会话的状态、输出、token 一眼可见；权限审批、回答提问、失败重试，在卡片上一键完成。你不再是「切换终端窗口的人」，而是坐在指挥席上的人。

**多渠道玩家的家。** 官方订阅、第三方 API、自建代理、本地模型——不同会话各用各的，聊到一半随时热切：这一轮用强模型攻坚，下一轮换便宜渠道跑杂活。「跟随 CLI」与「官方直连」并立，CLI 配置指向哪里，设置页看得清清楚楚。

**数据主权在你手里。** 对各引擎的原始会话架构级只读：Claude Code JSONL 与 Codex 会话文件都不会被写入。Codex 的运行操作在安装 CLI 时通过本机官方 App Server 完成，从不改写 rollout。零遥测、无 Monet 账号体系；本地会话能力离线运行，只有订阅额度和你主动使用的智能增强会访问相应供应商的官方服务。Monet 的标题、标签、收藏、软删除等数据独立住在 `~/.monet/`，卸载后原始会话毫发无损。

**睡觉时也在干活。** 定时任务由系统调度器执行，Monet 没开也照跑；Mac 能按点自己醒来，跑完任务再睡回去。系统通知随时把你叫回来——人可以走开，事情不会停。

## 功能

### 多引擎系统——Claude Code 与 Codex 同席

- 档案馆、搜索、工作台和通知可同时承载 Claude Code 与 Codex，会话始终带引擎徽标并可按引擎筛选
- Codex 直接读取本机会话文件中的已有历史；安装 CLI 后，再通过本机 `codex app-server` 提供新建/恢复、流式输出、运行中继续发送、中断、三类审批以及动态模型/思考强度选择
- 设置中的「引擎中心」分别展示安装、认证、版本、能力和诊断；一个引擎故障不会拖垮另一个
- 内部 Engine Adapter 契约统一了身份、历史源、时间线、运行时、能力和可选 facet；新增引擎无需增加顶层 IPC 或改动共享存储 schema
- Claude Code 保留成熟的专属工作台、渠道、工坊和自动化能力；其他引擎按自身 capability 显示可用操作，不出现注定失败的空按钮

### 工作台——并行 Agent 指挥

- 列数不设上限，屏幕放不下就横向滚动，滚轮如原生触控板般顺滑
- 监控轨总览全部会话：实时状态、尾部输出、token 用量；审批/重试/回答，卡面直接点
- 权限请求变 GUI 卡片：危险命令红色警示、AI 用人话批注风险，`Enter` 放行 `Esc` 拒绝
- **赛马模式**：同一个问题广播给不同模型/渠道，答案与成本并排见分晓
- **全景图导出**：一键把整面工作台截成一张全景图，分享或存档当前战况

<p align="center">
  <img src=".github/assets/race-mode.webp" alt="赛马模式——同一问题广播给四个模型，思考过程与答案并排对比" width="920">
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
| Codex 会话 | `$CODEX_HOME/sessions/` 与 `$CODEX_HOME/archived_sessions/`（默认 `~/.codex/`） | **只读；安装 CLI 后运行操作走官方 App Server** |
| Monet 增值数据（标题、标签、定时任务） | `~/.monet/` | 读写 |
| MCP 注册 | `~/.claude/settings.json` | 在 `mcpServers` 下添加 `monet` 条目 |

本地会话能力离线运行、零遥测、无 Monet 账号体系。订阅额度只访问 Claude/Codex 官方服务，并复用官方客户端已有登录态；Monet 不主动刷新或回写 OAuth 凭据。凭据纪律：API token 不进命令行参数、临时文件即用即删。

升级时，旧版 `metadata.json` 会被幂等迁移到按引擎隔离的 `metadata-v2.json`；旧文件不会删除或覆盖。搜索缓存会在 `search/v2/` 按引擎和项目冷重建。回滚旧版仍可继续使用原 Claude 数据，再次升级也不会覆盖已存在的新格式状态。

## 常见问题

> 遇到问题先别翻文档——把这句丢给你的 AI：`读取 https://raw.githubusercontent.com/zenolab124/monet/main/llms.txt 帮我排查 Monet 的问题`。它能自诊常见故障，确认是 bug 还会带上诊断数据帮你提报（无需 GitHub 账号）。

**Monet 会取代 Claude Code CLI 吗？**
不会——它是伴侣。干活的是 CLI，Monet 给你眼睛和手。两边启动的会话互相可见。

**我的会话数据安全吗？**
架构级只读，代码开源可查。删掉 Monet，你的 Claude Code 数据毫发无损。

**装完还要配什么吗？**
不用。CLI 能跑 Monet 就能跑；多渠道、AI 增值全是可选进阶。

**首次打开有 Gatekeeper 警告？**
不应该有——正式版安装包已经 Apple 公证，下载即开。如果仍被拦截，说明装的是早期未公证版本，建议直接下载最新版；也可以到「系统设置 → 隐私与安全性」点击「仍要打开」完成一次性授权。

**Windows / Linux？**
Windows 已支持（核心功能完整，macOS 系统集成除外）；Linux 暂无近期计划。

**支持哪些 Agent？**
当前正式支持 Claude Code 与 Codex。Monet 已经是多引擎系统，而不是两个平台分支的拼接；新的生产 adapter 只需实现自身协议与能力声明。内部接入说明见 [Engine Adapter 指南](ENGINE_ADAPTERS.zh-CN.md)。

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
