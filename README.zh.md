<p align="center">
  <img src="assets/logo.png" alt="CodeTerm" width="128" />
</p>

<h1 align="center">CodeTerm</h1>

<p align="center">
  <strong>AI 优先的终端复用器</strong><br>
  专为将 AI 编程代理纳入工作流程的开发者而构建。
</p>

<p align="center">
  <a href="#功能特性">功能特性</a> &bull;
  <a href="#安装">安装</a> &bull;
  <a href="#键盘快捷键">快捷键</a> &bull;
  <a href="#api">API</a> &bull;
  <a href="#配置">配置</a> &bull;
  <a href="#从源码构建">构建</a>
</p>

<p align="center">
  <a href="README.md">English</a> &bull;
  <a href="README.ja.md">日本語</a> &bull;
  <a href="README.ko.md">한국어</a> &bull;
  <a href="README.es.md">Español</a> &bull;
  <a href="README.de.md">Deutsch</a> &bull;
  <a href="README.fr.md">Français</a> &bull;
  <a href="README.pt.md">Português</a> &bull;
  <a href="README.ru.md">Русский</a>
</p>

<!-- SCREENSHOT PLACEHOLDER: Add screenshots here -->
<!-- ![CodeTerm Screenshot](assets/screenshot.png) -->

---

## 什么是 CodeTerm？

CodeTerm 是一款从零开始为 AI 辅助开发而设计的跨平台终端复用器。它将终端复用器（如 tmux）的强大功能与对 Claude Code 等 AI 编程代理的一流支持相结合。

**与其他终端的关键区别：** CodeTerm 提供完整的 REST API、CLI 和 Socket API——AI 代理可以创建工作区、拆分窗格、运行命令、读取输出，并以编程方式管理整个终端。您的 AI 助手成为终端工作流程中的一等公民。

基于 **Tauri v2**（Rust）+ **React** + **xterm.js** 构建。

## 功能特性

### 终端复用器
- **工作区与标签页** — 将项目组织到独立的工作区中，每个工作区包含多个标签页
- **窗格拆分** — 支持水平和垂直拆分，带可拖拽分隔条
- **会话持久化** — 工作区、标签页、窗格布局和工作目录在重启后保存并恢复
- **空间窗格导航** — 使用 Alt+方向键在窗格之间移动

### AI 代理集成
- **代理检测** — 自动检测在窗格中运行的 Claude Code（橙色指示器）
- **会话恢复** — 保存 Claude Code 会话 ID，并在重启时通过 `--resume` 恢复
- **权限通知** — 将 Claude Code 的权限请求作为桌面通知呈现
- **Webhook 系统** — 为代理事件（权限请求、任务完成）注册 webhook
- **编程控制** — 代理可以通过 API 创建窗格、运行命令、读取输出

### 完整 API
- **REST API** — 30+ 个端点，提供完整的终端控制（端口 7685）
- **CLI 工具** — `codeterm` 命令行工具，用于脚本编写和自动化
- **Socket API** — 基于命名管道（Windows）/ Unix 套接字的 JSON-RPC v2
- **实时流式传输** — SSE 端点，用于实时终端输出
- **输出捕获** — 基于游标的分页、ANSI 剥离、屏幕快照

### 主题
- **8 个内置主题** — GitHub Dark、Dracula、Catppuccin Mocha、One Dark、Nord、GitHub Light、Solarized Light、Pip-Boy
- **Pip-Boy CRT 模式** — 扫描线、荧光辉光、屏幕闪烁（自动激活）
- **完整 UI 主题化** — 主题应用于整个界面，而非仅限终端
- **自定义颜色** — 在 `config.toml` 中覆盖单个颜色
- **浅色主题支持** — 自动颜色推导适用于浅色和深色主题

### 内置浏览器
- **浏览器窗格** — 在终端旁并排打开网页
- **本地文件服务** — 在浏览器窗格中渲染本地 HTML/可视化内容
- **导航控件** — 后退、前进、刷新、地址栏

### 开发者体验
- **macOS 风格滚动条** — 纤细、圆角、不显眼
- **拖放** — 将文件拖入终端（文本文件粘贴内容，图片保存到磁盘）
- **Tab 补全** — 内置 Shell 补全引擎
- **搜索** — Ctrl+Shift+F 搜索终端输出
- **视觉响铃** — 屏幕闪烁代替音频
- **工作目录跟踪** — 通过 OSC 7 转义序列跟踪工作目录

## 安装

### 下载

预构建的二进制文件将在 [Releases](https://github.com/rollacode/codeterm/releases) 页面提供。

### 从源码构建

**前置要求：**
- [Node.js](https://nodejs.org/) 18+
- [pnpm](https://pnpm.io/) 10+
- [Rust](https://rustup.rs/) 1.75+

**Windows：**
- Visual Studio Build Tools 2022+，需勾选「使用 C++ 的桌面开发」工作负载
- WebView2（Windows 10 1803+ 及 Windows 11 已预装）

**macOS：**
- Xcode 命令行工具：`xcode-select --install`

**Linux（Debian/Ubuntu）：**
```bash
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file \
  libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev
```

```bash
git clone https://github.com/rollacode/codeterm.git
cd codeterm
pnpm install

# 开发模式
pnpm tauri dev

# 生产构建
pnpm tauri build

# CLI 工具
cd cli && cargo build --release
```

## 键盘快捷键

| 快捷键 | 操作 |
|----------|--------|
| `Ctrl+Shift+T` | 新建标签页 |
| `Ctrl+Shift+W` | 关闭标签页 |
| `Ctrl+Shift+D` | 向右拆分 |
| `Ctrl+Shift+E` | 向下拆分 |
| `Ctrl+Shift+Q` | 关闭窗格 |
| `Ctrl+Shift+N` | 新建工作区 |
| `Ctrl+Shift+B` | 打开浏览器窗格 |
| `Ctrl+Shift+F` | 在终端中搜索 |
| `Ctrl+Shift+C` | 复制 |
| `Ctrl+Shift+V` | 粘贴 |
| `Ctrl+Tab` | 下一个标签页 |
| `Ctrl+Shift+Tab` | 上一个标签页 |
| `Ctrl+1-9` | 切换到第 N 个标签页 |
| `Ctrl+,` | 设置 |
| `Ctrl+=` / `Ctrl+-` | 放大/缩小 |
| `Ctrl+0` | 重置缩放 |
| `Alt+方向键` | 导航窗格 |

## API

CodeTerm 提供三种 API 用于集成：

### REST API（端口 7685）

```bash
# 列出窗格
curl http://localhost:7685/api/panes

# 执行命令并获取输出
curl -X POST http://localhost:7685/api/panes/{id}/exec \
  -H "Content-Type: application/json" \
  -d '{"command": "git status"}'

# 发送文本到终端
curl -X POST http://localhost:7685/api/panes/{id}/send \
  -H "Content-Type: application/json" \
  -d '{"text": "npm test", "enter": true}'

# 创建工作区
curl -X POST http://localhost:7685/api/workspaces \
  -H "Content-Type: application/json" \
  -d '{"name": "My Project"}'

# 拆分窗格
curl -X POST http://localhost:7685/api/panes/{id}/split \
  -H "Content-Type: application/json" \
  -d '{"direction": "right"}'

# 实时流式输出（SSE）
curl http://localhost:7685/api/panes/{id}/output/stream
```

### CLI 工具

```bash
# 工作区管理
codeterm workspace list
codeterm workspace create "Project"

# 终端控制
codeterm pane list
codeterm pane split PANE_ID --direction right
codeterm send-text PANE_ID "npm run build"
codeterm get-output PANE_ID --lines 50

# 通知
codeterm notify "Build complete" --level info

# 浏览器
codeterm browser open "http://localhost:3000"
```

### 环境变量

CodeTerm 在每个 PTY 会话中设置以下变量：

| 变量 | 描述 |
|----------|-------------|
| `CODETERM` | 始终为 `1` — 用于检测是否在 CodeTerm 内运行 |
| `CODETERM_PANE_ID` | 当前窗格的 UUID |
| `CODETERM_SOCKET_PATH` | JSON-RPC API 的套接字路径 |
| `TERM_PROGRAM` | `CodeTerm` |

完整的端点文档请参阅 [API 参考](docs/API.md)。

## 配置

配置文件：`%APPDATA%/codeterm/config.toml`（Windows）或 `~/.config/codeterm/config.toml`

```toml
[theme]
name = "github-dark"
# 可选: github-dark, dracula, catppuccin-mocha, one-dark, nord,
#        github-light, solarized-light, pip-boy

[font]
family = "JetBrains Mono, Cascadia Code, Fira Code, Consolas, monospace"
size = 14
ligatures = true

[terminal]
cursor_style = "block"    # block, underline, bar
cursor_blink = true
scrollback = 10000
copy_on_select = false

[api]
enabled = true
port = 7685
# token = "your-secret-token"
```

所有配置选项请参阅[配置指南](docs/CONFIG.md)。

## 架构

```
┌─────────────────────────────────────────────┐
│                  CodeTerm                    │
│  ┌──────────┐  ┌──────────┐  ┌───────────┐  │
│  │ React UI │  │ xterm.js │  │  浏览器    │  │
│  │  侧边栏  │  │   终端   │  │   窗格    │  │
│  └────┬─────┘  └────┬─────┘  └─────┬─────┘  │
│       │              │              │        │
│  ┌────┴──────────────┴──────────────┴────┐   │
│  │           Tauri v2 (Rust)             │   │
│  │  ┌────────┐ ┌─────┐ ┌─────────────┐  │   │
│  │  │  复用  │ │ PTY │ │  REST API   │  │   │
│  │  │  状态  │ │ 管理 │ │  (Axum)     │  │   │
│  │  └────────┘ └─────┘ └─────────────┘  │   │
│  │  ┌────────┐ ┌─────────┐ ┌─────────┐  │   │
│  │  │ 会话   │ │  代理   │ │ Socket  │  │   │
│  │  │ 持久化 │ │ 检测器  │ │JSON-RPC │  │   │
│  │  └────────┘ └─────────┘ └─────────┘  │   │
│  └───────────────────────────────────────┘   │
└─────────────────────────────────────────────┘
```

## 技术栈

| 层级 | 技术 |
|-------|-----------|
| 框架 | Tauri v2 |
| 后端 | Rust, Axum, tokio |
| 前端 | React 19, TypeScript, Tailwind CSS 4 |
| 终端 | xterm.js 6（WebGL 渲染器） |
| PTY | portable-pty, ConPTY (Windows) |
| 构建 | pnpm, Cargo |

## 平台支持

| 平台 | 状态 |
|----------|--------|
| Windows | 已支持 |
| macOS | 已支持 |
| Linux | 已支持 |

## 许可证

MIT

## 贡献

欢迎贡献。请先创建 issue 讨论您想要更改的内容。
