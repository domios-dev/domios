<p align="center">
  <img src="assets/logo.png" alt="CodeTerm" width="128" />
</p>

<h1 align="center">CodeTerm</h1>

<p align="center">
  <strong>AI 중심 터미널 멀티플렉서</strong><br>
  AI 코딩 에이전트를 워크플로우의 일부로 사용하는 개발자를 위해 만들어졌습니다.
</p>

<p align="center">
  <a href="#기능">기능</a> &bull;
  <a href="#설치">설치</a> &bull;
  <a href="#키보드-단축키">단축키</a> &bull;
  <a href="#api">API</a> &bull;
  <a href="#설정">설정</a> &bull;
  <a href="#소스에서-빌드하기">빌드</a>
</p>

<p align="center">
  <a href="README.md">English</a> &bull;
  <a href="README.zh.md">中文</a> &bull;
  <a href="README.ja.md">日本語</a> &bull;
  <a href="README.es.md">Español</a> &bull;
  <a href="README.de.md">Deutsch</a> &bull;
  <a href="README.fr.md">Français</a> &bull;
  <a href="README.pt.md">Português</a> &bull;
  <a href="README.ru.md">Русский</a>
</p>

<!-- SCREENSHOT PLACEHOLDER: Add screenshots here -->
<!-- ![CodeTerm Screenshot](assets/screenshot.png) -->

---

## CodeTerm이란?

CodeTerm은 AI 지원 개발을 위해 처음부터 설계된 크로스 플랫폼 터미널 멀티플렉서입니다. tmux와 같은 터미널 멀티플렉서의 강력함에 Claude Code와 같은 AI 코딩 에이전트에 대한 일급 지원을 결합했습니다.

**다른 터미널과의 핵심 차이점:** CodeTerm은 완전한 REST API, CLI 및 Socket API를 제공합니다 — AI 에이전트가 워크스페이스를 생성하고, 패널을 분할하고, 명령을 실행하고, 출력을 읽고, 전체 터미널을 프로그래밍 방식으로 관리할 수 있습니다. AI 어시스턴트가 터미널 워크플로우의 일급 시민이 됩니다.

**Tauri v2** (Rust) + **React** + **xterm.js**로 구축되었습니다.

## 기능

### 터미널 멀티플렉서
- **워크스페이스 & 탭** — 프로젝트를 별도의 워크스페이스로 구성하고, 각 워크스페이스에 여러 탭을 사용
- **패널 분할** — 드래그 가능한 구분선으로 수평 및 수직 분할
- **세션 유지** — 워크스페이스, 탭, 패널 레이아웃, 작업 디렉토리가 저장되어 재시작 시 복원
- **공간 패널 탐색** — Alt+화살표 키로 패널 간 이동

### AI 에이전트 통합
- **에이전트 감지** — 패널에서 실행 중인 Claude Code를 자동 감지 (주황색 표시기)
- **세션 재개** — Claude Code 세션 ID를 저장하고 재시작 시 `--resume`으로 재개
- **권한 알림** — Claude Code 권한 요청을 데스크톱 알림으로 표시
- **웹훅 시스템** — 에이전트 이벤트(권한 요청, 작업 완료)에 대한 웹훅 등록
- **프로그래밍 제어** — 에이전트가 API를 통해 패널 생성, 명령 실행, 출력 읽기 가능

### 완전한 API
- **REST API** — 터미널 전체 제어를 위한 30개 이상의 엔드포인트 (포트 7685)
- **CLI 도구** — 스크립팅 및 자동화를 위한 `codeterm` 커맨드라인 도구
- **Socket API** — 명명된 파이프(Windows) / Unix 소켓을 통한 JSON-RPC v2
- **실시간 스트리밍** — 실시간 터미널 출력을 위한 SSE 엔드포인트
- **출력 캡처** — 커서 기반 페이지네이션, ANSI 제거, 화면 스냅샷

### 테마
- **8개의 내장 테마** — GitHub Dark, Dracula, Catppuccin Mocha, One Dark, Nord, GitHub Light, Solarized Light, Pip-Boy
- **Pip-Boy CRT 모드** — 스캔라인, 인광 효과, 화면 깜빡임 (자동 활성화)
- **전체 UI 테마** — 테마가 터미널뿐만 아니라 전체 인터페이스에 적용
- **커스텀 색상** — `config.toml`에서 개별 색상 오버라이드
- **밝은 테마 지원** — 밝은 테마와 어두운 테마 모두에 대한 자동 색상 도출

### 내장 브라우저
- **브라우저 패널** — 터미널 옆에 웹 페이지를 나란히 열기
- **로컬 파일 서빙** — 브라우저 패널에서 로컬 HTML/시각화 렌더링
- **탐색 컨트롤** — 뒤로, 앞으로, 새로고침, URL 바

### 개발자 경험
- **macOS 스타일 스크롤바** — 얇고, 둥글고, 눈에 띄지 않는
- **드래그 & 드롭** — 터미널에 파일 드롭 (텍스트 파일은 내용 붙여넣기, 이미지는 디스크에 저장)
- **탭 완성** — 내장 셸 완성 엔진
- **검색** — Ctrl+Shift+F로 터미널 출력 검색
- **시각적 벨** — 소리 대신 화면 깜빡임
- **CWD 추적** — OSC 7 이스케이프 시퀀스를 통한 작업 디렉토리 추적

## 설치

### 다운로드

사전 빌드된 바이너리는 [릴리스](https://github.com/rollacode/codeterm/releases) 페이지에서 제공될 예정입니다.

### 소스에서 빌드하기

**사전 요구사항:**
- Node.js 18+
- pnpm 10+
- Rust 1.75+

**Windows:**
- "C++를 사용한 데스크톱 개발" 워크로드가 포함된 Visual Studio Build Tools 2022 이상
- WebView2 (Windows 10 1803 이상 및 Windows 11에 사전 설치됨)

**macOS:**
- Xcode 커맨드 라인 도구: `xcode-select --install`

**Linux (Debian/Ubuntu):**
```bash
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file \
  libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev
```

```bash
git clone https://github.com/rollacode/codeterm.git
cd codeterm
pnpm install

# 개발
pnpm tauri dev

# 프로덕션 빌드
pnpm tauri build

# CLI 도구
cd cli && cargo build --release
```

## 키보드 단축키

| 단축키 | 동작 |
|----------|--------|
| `Ctrl+Shift+T` | 새 탭 |
| `Ctrl+Shift+W` | 탭 닫기 |
| `Ctrl+Shift+D` | 오른쪽 분할 |
| `Ctrl+Shift+E` | 아래로 분할 |
| `Ctrl+Shift+Q` | 패널 닫기 |
| `Ctrl+Shift+N` | 새 워크스페이스 |
| `Ctrl+Shift+B` | 브라우저 패널 열기 |
| `Ctrl+Shift+F` | 터미널에서 검색 |
| `Ctrl+Shift+C` | 복사 |
| `Ctrl+Shift+V` | 붙여넣기 |
| `Ctrl+Tab` | 다음 탭 |
| `Ctrl+Shift+Tab` | 이전 탭 |
| `Ctrl+1-9` | N번째 탭으로 전환 |
| `Ctrl+,` | 설정 |
| `Ctrl+=` / `Ctrl+-` | 확대/축소 |
| `Ctrl+0` | 확대/축소 초기화 |
| `Alt+Arrows` | 패널 탐색 |

## API

CodeTerm은 통합을 위한 세 가지 API를 제공합니다:

### REST API (포트 7685)

```bash
# 패널 목록
curl http://localhost:7685/api/panes

# 명령 실행 및 출력 받기
curl -X POST http://localhost:7685/api/panes/{id}/exec \
  -H "Content-Type: application/json" \
  -d '{"command": "git status"}'

# 터미널에 텍스트 보내기
curl -X POST http://localhost:7685/api/panes/{id}/send \
  -H "Content-Type: application/json" \
  -d '{"text": "npm test", "enter": true}'

# 워크스페이스 생성
curl -X POST http://localhost:7685/api/workspaces \
  -H "Content-Type: application/json" \
  -d '{"name": "My Project"}'

# 패널 분할
curl -X POST http://localhost:7685/api/panes/{id}/split \
  -H "Content-Type: application/json" \
  -d '{"direction": "right"}'

# 실시간 출력 스트리밍 (SSE)
curl http://localhost:7685/api/panes/{id}/output/stream
```

### CLI 도구

```bash
# 워크스페이스 관리
codeterm workspace list
codeterm workspace create "Project"

# 터미널 제어
codeterm pane list
codeterm pane split PANE_ID --direction right
codeterm send-text PANE_ID "npm run build"
codeterm get-output PANE_ID --lines 50

# 알림
codeterm notify "Build complete" --level info

# 브라우저
codeterm browser open "http://localhost:3000"
```

### 환경 변수

CodeTerm은 모든 PTY 세션에서 다음을 설정합니다:

| 변수 | 설명 |
|----------|-------------|
| `CODETERM` | 항상 `1` — CodeTerm 내부에서 실행 중인지 감지 |
| `CODETERM_PANE_ID` | 현재 패널의 UUID |
| `CODETERM_SOCKET_PATH` | JSON-RPC API용 소켓 경로 |
| `TERM_PROGRAM` | `CodeTerm` |

전체 엔드포인트 문서는 [API 레퍼런스](docs/API.md)를 참조하세요.

## 설정

설정 파일: `%APPDATA%/codeterm/config.toml` (Windows) 또는 `~/.config/codeterm/config.toml`

```toml
[theme]
name = "github-dark"
# 사용 가능: github-dark, dracula, catppuccin-mocha, one-dark, nord,
#            github-light, solarized-light, pip-boy

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

모든 옵션은 [설정 가이드](docs/CONFIG.md)를 참조하세요.

## 아키텍처

```
┌─────────────────────────────────────────────┐
│                  CodeTerm                    │
│  ┌──────────┐  ┌──────────┐  ┌───────────┐  │
│  │ React UI │  │ xterm.js │  │  Browser   │  │
│  │ Sidebar  │  │ Terminal │  │   Panes    │  │
│  └────┬─────┘  └────┬─────┘  └─────┬─────┘  │
│       │              │              │        │
│  ┌────┴──────────────┴──────────────┴────┐   │
│  │           Tauri v2 (Rust)             │   │
│  │  ┌────────┐ ┌─────┐ ┌─────────────┐  │   │
│  │  │  Mux   │ │ PTY │ │  REST API   │  │   │
│  │  │ State  │ │ Mgr │ │  (Axum)     │  │   │
│  │  └────────┘ └─────┘ └─────────────┘  │   │
│  │  ┌────────┐ ┌─────────┐ ┌─────────┐  │   │
│  │  │Session │ │  Agent  │ │ Socket  │  │   │
│  │  │Persist │ │Detector │ │JSON-RPC │  │   │
│  │  └────────┘ └─────────┘ └─────────┘  │   │
│  └───────────────────────────────────────┘   │
└─────────────────────────────────────────────┘
```

## 기술 스택

| 레이어 | 기술 |
|-------|-----------|
| 프레임워크 | Tauri v2 |
| 백엔드 | Rust, Axum, tokio |
| 프론트엔드 | React 19, TypeScript, Tailwind CSS 4 |
| 터미널 | xterm.js 6 (WebGL 렌더러) |
| PTY | portable-pty, ConPTY (Windows) |
| 빌드 | pnpm, Cargo |

## 플랫폼

| 플랫폼 | 상태 |
|----------|--------|
| Windows | 지원 |
| macOS | 지원 |
| Linux | 지원 |

## 라이선스

MIT

## 기여하기

기여를 환영합니다. 변경하고 싶은 내용에 대해 먼저 이슈를 열어 논의해 주세요.
