<p align="center">
  <img src="assets/logo.png" alt="CodeTerm" width="128" />
</p>

<h1 align="center">CodeTerm</h1>
> Перевод README.md может отставать. Английская версия является канонической.

<p align="center">
  <strong>AI-first терминальный мультиплексор</strong><br>
  Создан для разработчиков, использующих AI-агентов в своём рабочем процессе.
</p>

<p align="center">
  <a href="#возможности">Возможности</a> &bull;
  <a href="#установка">Установка</a> &bull;
  <a href="#горячие-клавиши">Горячие клавиши</a> &bull;
  <a href="#api">API</a> &bull;
  <a href="#конфигурация">Конфигурация</a> &bull;
  <a href="#сборка-из-исходников">Сборка</a>
</p>

<p align="center">
  <a href="README.md">English</a> &bull;
  <a href="README.zh.md">中文</a> &bull;
  <a href="README.ja.md">日本語</a> &bull;
  <a href="README.ko.md">한국어</a> &bull;
  <a href="README.es.md">Español</a> &bull;
  <a href="README.de.md">Deutsch</a> &bull;
  <a href="README.fr.md">Français</a> &bull;
  <a href="README.pt.md">Português</a>
</p>

<!-- SCREENSHOT PLACEHOLDER: Add screenshots here -->
<!-- ![CodeTerm Screenshot](assets/screenshot.png) -->
<!-- TRANSLATION-TODO (2026-04-29 review): translation lags README.md — please mirror these sections from English:
     1. The two top-of-features GIF subsections (`### Agent Orchestration`, `### Cron Jobs`) shown right after the language nav.
     2. Per-OS install subsections instead of one combined "Скачать": `### macOS (Homebrew)`, `### Windows (Scoop)`, `### Windows (installer)`, `### Linux`.
     3. Rename the "Встроенный браузер" section to match English `### Canvas Panels` (the feature was renamed). -->

---

## Что такое CodeTerm?

CodeTerm — это кроссплатформенный терминальный мультиплексор, спроектированный с нуля для разработки с помощью AI. Он сочетает мощь терминального мультиплексора (вроде tmux) с полноценной поддержкой AI-агентов, таких как Claude Code.

**Ключевое отличие от других терминалов:** CodeTerm предоставляет полноценный REST API, CLI и Socket API — AI-агенты могут создавать рабочие пространства, разделять панели, выполнять команды, читать вывод и управлять терминалом программно. Ваш AI-ассистент становится полноправным участником рабочего процесса в терминале.

Построен на **Tauri v2** (Rust) + **React** + **xterm.js**.

## Возможности

### Терминальный мультиплексор
- **Рабочие пространства и вкладки** — организуйте проекты в отдельные рабочие пространства, каждое с несколькими вкладками
- **Разделение панелей** — горизонтальное и вертикальное разделение с перетаскиваемыми разделителями
- **Сохранение сессий** — рабочие пространства, вкладки, раскладка панелей и рабочие директории сохраняются и восстанавливаются при перезапуске
- **Пространственная навигация по панелям** — переключение между панелями с помощью Alt+стрелки

### Интеграция с AI-агентами
- **Обнаружение агентов** — автоматическое обнаружение Claude Code, работающего в панели (оранжевый индикатор)
- **Возобновление сессий** — сохраняет ID сессий Claude Code и возобновляет их при перезапуске с `--resume`
- **Уведомления о разрешениях** — запросы разрешений от Claude Code отображаются как системные уведомления
- **Система вебхуков** — регистрация вебхуков для событий агента (запросы разрешений, завершение задач)
- **Программное управление** — агенты могут создавать панели, выполнять команды, читать вывод через API
- **Перезапуск агента** — `codeterm agent restart --pane ID` выполняет корректный `/exit` с авто-возобновлением для любой панели (`--force` принудительно завершает зависший агент); тот же механизм используется при самообновлении

### Полноценный API
- **REST API** — 30+ эндпоинтов для полного управления терминалом (порт 7685)
- **CLI-утилита** — инструмент командной строки `codeterm` для скриптов и автоматизации
- **Socket API** — JSON-RPC v2 через именованные каналы (Windows) / Unix-сокеты
- **Потоковая передача в реальном времени** — SSE-эндпоинт для живого вывода терминала
- **Захват вывода** — пагинация на основе курсора, очистка ANSI, снимки экрана

### Мосты и внешние сервисы
- **Мост Telegram** — двусторонний. Входящие сообщения попадают в панель General Agent как `[Telegram: USER_ID] text`; исходящие ответы отправляются через `codeterm bridge send` с HTML-форматированием (санитайзер автоматически экранирует случайные подстроки `<word>`, чтобы неподдерживаемые токены не ломали парсер)
- **Транскрипция голосовых** — при `integrations.voice_transcribe.enabled = true` голосовые и видео-сообщения транскрибируются локальным демоном Whisper (по умолчанию `http://127.0.0.1:7891`) до того, как достигнут любого агента — Claude, Codex и будущие клиенты охвачены одним апстрим-изменением
- **Мост вебхуков** — регистрация URL для событий агента (`agent.permission_request`, `agent.task_complete`, `agent.report`)

### Самообновление и устойчивость
- **Самообновление** — `codeterm update --check / --apply / --channel <stable|rc|nightly>`. Манифесты подписаны ed25519 и проверяются до любых операций с бинарником; авто-установка выполняет 9-этапное корректное завершение работы (чекпоинт mem WAL, остановка супервизора, подменa подписанного бинарника), после чего панели авто-возобновляются — контекст переживает обновление
- **Супервизор подпроцессов** — тяжёлые операции памяти (dreaming, извлечение в Librarian) выполняются как дочерние процессы с автоматическим выключателем (3 сбоя за 60 с → размыкание). Падение воркера не уносит API; `503` тайм-ауты на тяжёлых mem-запросах сохраняют отзывчивость демона
- **Внешний сторожевой таймер** — health-проба на `/api/health`; рецепты для Windows Task Scheduler, launchd и systemd — в [docs/RESILIENCE.md](docs/RESILIENCE.md)

### Темы оформления
- **8 встроенных тем** — GitHub Dark, Dracula, Catppuccin Mocha, One Dark, Nord, GitHub Light, Solarized Light, Pip-Boy
- **Pip-Boy CRT-режим** — линии развёртки, фосфорное свечение, мерцание экрана (активируется автоматически)
- **Полная стилизация интерфейса** — темы применяются ко всему интерфейсу, а не только к терминалу
- **Пользовательские цвета** — переопределение отдельных цветов в `config.toml`
- **Поддержка светлых тем** — автоматическая деривация цветов работает как для светлых, так и для тёмных тем

### Встроенный браузер
- **Браузерные панели** — открывайте веб-страницы рядом с терминалом
- **Локальная раздача файлов** — рендеринг локальных HTML/визуализаций в браузерных панелях
- **Элементы навигации** — назад, вперёд, обновить, адресная строка

### Удобство разработки
- **Скроллбары в стиле macOS** — тонкие, скруглённые, ненавязчивые
- **Drag & drop** — перетаскивание файлов в терминал (текстовые файлы вставляют содержимое, изображения сохраняются на диск)
- **Автодополнение** — встроенный движок автодополнения для оболочки
- **Поиск** — Ctrl+Shift+F для поиска по выводу терминала
- **Визуальный звонок** — вспышка экрана вместо звука
- **Отслеживание CWD** — отслеживание рабочей директории через escape-последовательности OSC 7

## Установка

### Скачать

Готовые сборки будут доступны на странице [Releases](https://github.com/rollacode/codeterm/releases).

### Сборка из исходников

**Требования:**
- [Node.js](https://nodejs.org/) 18+
- [pnpm](https://pnpm.io/) 10+
- [Rust](https://rustup.rs/) 1.75+

**Windows:**
- [Visual Studio Build Tools 2022+](https://visualstudio.microsoft.com/visual-cpp-build-tools/) с компонентом «Разработка классических приложений на C++»
- WebView2 (предустановлен в Windows 10 1803+ и Windows 11)

**macOS:**
- Xcode Command Line Tools: `xcode-select --install`

**Linux (Debian/Ubuntu):**
```bash
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file \
  libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev
```

**Сборка:**
```bash
git clone https://github.com/rollacode/codeterm.git
cd codeterm
pnpm install

# Разработка
pnpm tauri dev

# Продакшн-сборка
pnpm tauri build

# CLI-утилита
cd cli && cargo build --release
```

## Горячие клавиши

> Сочетания используют Cmd на macOS и Ctrl на Windows/Linux. Настраиваются в Настройки → Сочетания клавиш.

| Сочетание | Действие |
|-----------|----------|
| `Cmd/Ctrl+T` | Новая вкладка |
| `Cmd/Ctrl+W` | Закрыть вкладку |
| `Cmd/Ctrl+Shift+→` | Разделить вправо |
| `Cmd/Ctrl+Shift+↓` | Разделить вниз |
| `Cmd/Ctrl+Shift+Q` | Закрыть панель |
| `Cmd/Ctrl+Shift+N` | Новое рабочее пространство |
| `Cmd/Ctrl+Shift+F` | Поиск в терминале |
| `Ctrl+Tab` | Следующая вкладка |
| `Ctrl+Shift+Tab` | Предыдущая вкладка |
| `Cmd/Ctrl+1-9` | Переключиться на вкладку N |
| `Cmd/Ctrl+,` | Настройки |
| `Cmd/Ctrl+=` / `Cmd/Ctrl+-` | Увеличить/уменьшить масштаб |
| `Cmd/Ctrl+0` | Сбросить масштаб |
| `Alt+Стрелки` | Навигация по панелям |

## API

CodeTerm предоставляет три API для интеграции:

### REST API (порт 7685)

```bash
# Список панелей
curl http://localhost:7685/api/panes

# Выполнить команду и получить вывод
curl -X POST http://localhost:7685/api/panes/{id}/exec \
  -H "Content-Type: application/json" \
  -d '{"command": "git status"}'

# Отправить текст в терминал
curl -X POST http://localhost:7685/api/panes/{id}/send \
  -H "Content-Type: application/json" \
  -d '{"text": "npm test", "enter": true}'

# Создать рабочее пространство
curl -X POST http://localhost:7685/api/workspaces \
  -H "Content-Type: application/json" \
  -d '{"name": "My Project"}'

# Запустить агента в новой панели рядом с родительской
curl -X POST http://localhost:7685/api/agents/spawn \
  -H "Content-Type: application/json" \
  -d '{"agent": "claude", "parent_pane_id": "PANE_ID"}'

# Потоковый вывод в реальном времени (SSE)
curl http://localhost:7685/api/panes/{id}/output/stream
```

### CLI-утилита

```bash
# Управление рабочими пространствами
codeterm workspace list
codeterm workspace create "Project"

# Управление терминалом
codeterm pane list
codeterm send-text PANE_ID "npm run build"
codeterm get-output PANE_ID --lines 50

# Уведомления
codeterm notify "Build complete" --level info

# Браузер
codeterm browser open "http://localhost:3000"
```

### Переменные окружения

CodeTerm устанавливает следующие переменные в каждой PTY-сессии:

| Переменная | Описание |
|------------|----------|
| `CODETERM` | Всегда `1` — определяет, запущен ли процесс внутри CodeTerm |
| `CODETERM_PANE_ID` | UUID текущей панели |
| `CODETERM_SOCKET_PATH` | Путь к сокету для JSON-RPC API |
| `TERM_PROGRAM` | `CodeTerm` |

Полную документацию по эндпоинтам см. в [Справочнике API](docs/API.md).

## Конфигурация

Файл конфигурации: `%APPDATA%/codeterm/config.toml` (Windows) или `~/.config/codeterm/config.toml`

```toml
[theme]
name = "github-dark"
# Доступные: github-dark, dracula, catppuccin-mocha, one-dark, nord,
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

Все параметры описаны в [Руководстве по конфигурации](docs/CONFIG.md).

## Архитектура

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

## Технологический стек

| Слой | Технология |
|------|-----------|
| Фреймворк | Tauri v2 |
| Бэкенд | Rust, Axum, tokio |
| Фронтенд | React 19, TypeScript, Tailwind CSS 4 |
| Терминал | xterm.js 6 (WebGL-рендерер) |
| PTY | portable-pty, ConPTY (Windows) |
| Сборка | pnpm, Cargo |

## Платформы

| Платформа | Статус |
|-----------|--------|
| Windows | Поддерживается |
| macOS | Поддерживается |
| Linux | Поддерживается |

## Лицензия

MIT

## Участие в разработке

Мы рады вкладу в проект. Пожалуйста, сначала создайте issue, чтобы обсудить предлагаемые изменения.
