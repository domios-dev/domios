<p align="center">
  <img src="assets/logo.png" alt="CodeTerm" width="128" />
</p>

<h1 align="center">CodeTerm</h1>
> Die Übersetzung von README.md kann hinterherhinken. Die englische Version ist maßgeblich.

<p align="center">
  <strong>KI-orientierter Terminal-Multiplexer</strong><br>
  Entwickelt für Entwickler, die KI-Coding-Agenten in ihren Workflow integrieren.
</p>

<p align="center">
  <a href="#features">Features</a> &bull;
  <a href="#installation">Installation</a> &bull;
  <a href="#tastenkürzel">Tastenkürzel</a> &bull;
  <a href="#api">API</a> &bull;
  <a href="#konfiguration">Konfiguration</a> &bull;
  <a href="#aus-dem-quellcode-bauen">Build</a>
</p>

<p align="center">
  <a href="README.md">English</a> &bull;
  <a href="README.zh.md">中文</a> &bull;
  <a href="README.ja.md">日本語</a> &bull;
  <a href="README.ko.md">한국어</a> &bull;
  <a href="README.es.md">Español</a> &bull;
  <a href="README.fr.md">Français</a> &bull;
  <a href="README.pt.md">Português</a> &bull;
  <a href="README.ru.md">Русский</a>
</p>

<!-- SCREENSHOT PLACEHOLDER: Add screenshots here -->
<!-- ![CodeTerm Screenshot](assets/screenshot.png) -->
<!-- TRANSLATION-TODO (2026-04-29 review): translation lags README.md — please mirror these sections from English:
     1. The two top-of-features GIF subsections (`### Agent Orchestration`, `### Cron Jobs`) shown right after the language nav.
     2. Per-OS install subsections instead of one combined "Download": `### macOS (Homebrew)`, `### Windows (Scoop)`, `### Windows (installer)`, `### Linux`.
     3. Rename the "Integrierter Browser" section to match English `### Canvas Tabls` (the feature was renamed). -->

---

## Was ist CodeTerm?

CodeTerm ist ein plattformübergreifender Terminal-Multiplexer, der von Grund auf für KI-gestützte Entwicklung konzipiert wurde. Er vereint die Leistungsfähigkeit eines Terminal-Multiplexers (wie tmux) mit erstklassiger Unterstützung für KI-Coding-Agenten wie Claude Code.

**Der wesentliche Unterschied zu anderen Terminals:** CodeTerm bietet eine vollständige REST-API, ein CLI-Tool und eine Socket-API — sodass KI-Agenten Workspaces und Tabs erstellen, Befehle ausführen, Ausgaben lesen und das gesamte Terminal programmatisch steuern können. Ihr KI-Assistent wird zum vollwertigen Teilnehmer in Ihrem Terminal-Workflow.

Gebaut mit **Tauri v2** (Rust) + **React** + **xterm.js**.

## Features

### Terminal-Multiplexer
- **Workspaces & Tabs** — Projekte in separate Workspaces organisieren, jeweils mit mehreren Tabs
- **Tab-Layout** — Arbeitsbereiche und Terminal-Tabs übersichtlich organisieren
- **Sitzungspersistenz** — Workspaces, Tabs, Layout und Arbeitsverzeichnisse werden über Neustarts hinweg gespeichert und wiederhergestellt
- **Räumliche Tab-Navigation** — zwischen Tabs mit Alt+Pfeiltasten wechseln

### KI-Agenten-Integration
- **Agenten-Erkennung** — erkennt automatisch Claude Code in einem Tab (orangener Indikator)
- **Sitzungswiederaufnahme** — speichert Claude-Code-Session-IDs und nimmt sie beim Neustart mit `--resume` wieder auf
- **Berechtigungsbenachrichtigungen** — zeigt Claude-Code-Berechtigungsanfragen als Desktop-Benachrichtigungen an
- **Webhook-System** — Webhooks für Agenten-Events registrieren (Berechtigungsanfragen, Aufgabenabschluss)
- **Programmatische Steuerung** — Agenten können Tabs erstellen, Befehle ausführen und Ausgaben über die API lesen
- **Agenten-Neustart** — `codeterm agent restart --tab ID` führt einen geordneten `/exit` mit Auto-Resume in jedem Tab aus (`--force` killt einen hängenden Agenten); derselbe Mechanismus wie beim Self-Update

### Vollständige API
- **REST-API** — über 30 Endpunkte für vollständige Terminalsteuerung (Port 7685)
- **CLI-Tool** — `codeterm` Kommandozeilen-Tool für Scripting und Automatisierung
- **Socket-API** — JSON-RPC v2 über Named Pipes (Windows) / Unix-Sockets
- **Echtzeit-Streaming** — SSE-Endpunkt für Live-Terminal-Ausgaben
- **Ausgabeerfassung** — cursorbasierte Paginierung, ANSI-Stripping, Bildschirm-Snapshots

### Bridges & externe Dienste
- **Telegram-Bridge** — bidirektional. Eingehende Nachrichten landen im General-Agent-Tab als `[Telegram: USER_ID] text`; ausgehende Antworten gehen über `codeterm bridge send` mit HTML-Formatierung (der Sanitizer escaped automatisch fremde `<word>`-Substrings, damit nicht unterstützte Tokens den Parser nicht brechen)
- **Sprach-Transkription** — bei `integrations.voice_transcribe.enabled = true` werden Sprach- und Video-Notizen von einem lokalen Whisper-Daemon (Standard `http://127.0.0.1:7891`) transkribiert, bevor sie irgendeinen Agenten erreichen — Claude, Codex und künftige Clients sind mit einer einzigen Upstream-Änderung abgedeckt
- **Webhook-Bridge** — URLs für Agenten-Events registrieren (`agent.permission_request`, `agent.task_complete`, `agent.report`)

### Self-Update & Resilienz
- **Self-Update** — `codeterm update --check / --apply / --channel <stable|rc|nightly>`. Manifeste sind ed25519-signiert und werden verifiziert, bevor irgendeine Binary angefasst wird; Auto-Install führt einen 9-stufigen geordneten Shutdown aus (mem-WAL-Checkpoint, Supervisor-Stop, signierter Binary-Tausch), danach werden die Tabs automatisch fortgesetzt — Kontext überlebt das Upgrade
- **Subprozess-Supervisor** — schwere Mem-Operationen (Dreaming, Librarian-Extraktion) laufen als Kindprozesse mit Circuit-Breaker (3 Fehler in 60 s → offen). Worker-Tod legt die API nicht lahm; `503`-Timeouts auf schweren Mem-Anfragen halten den Daemon reaktionsfähig
- **Externer Watchdog** — Health-Probe unter `/api/health`; Setup-Rezepte für Windows Task Scheduler, launchd und systemd in [docs/RESILIENCE.md](docs/RESILIENCE.md)

### Themes
- **8 integrierte Themes** — GitHub Dark, Dracula, Catppuccin Mocha, One Dark, Nord, GitHub Light, Solarized Light, Pip-Boy
- **Pip-Boy-CRT-Modus** — Scanlines, Phosphorleuchten, Bildschirmflackern (wird automatisch aktiviert)
- **Vollständiges UI-Theming** — Themes werden auf die gesamte Oberfläche angewendet, nicht nur auf das Terminal
- **Benutzerdefinierte Farben** — einzelne Farben in `config.toml` überschreiben
- **Helle Themes** — automatische Farbableitung funktioniert sowohl für helle als auch dunkle Themes

### Integrierter Browser
- **Browser-Tabs** — Webseiten neben dem Terminal öffnen
- **Lokale Dateibereitstellung** — lokale HTML-Dateien/Visualisierungen in Browser-Tabs rendern
- **Navigationssteuerung** — Zurück, Vorwärts, Aktualisieren, URL-Leiste

### Entwicklererfahrung
- **macOS-Stil Scrollbars** — dünn, abgerundet, dezent
- **Drag & Drop** — Dateien ins Terminal ziehen (Textdateien fügen Inhalt ein, Bilder werden auf der Festplatte gespeichert)
- **Tab-Vervollständigung** — integrierte Shell-Vervollständigungs-Engine
- **Suche** — Strg+Umschalt+F zum Durchsuchen der Terminal-Ausgabe
- **Visueller Signalton** — Bildschirmblitz statt Audiosignal
- **CWD-Tracking** — verfolgt das Arbeitsverzeichnis über OSC-7-Escape-Sequenzen

## Installation

### Download

Vorgefertigte Binärdateien werden auf der [Releases](https://github.com/rollacode/codeterm/releases)-Seite verfügbar sein.

### Aus dem Quellcode bauen

**Voraussetzungen:**
- [Node.js](https://nodejs.org/) 18+
- [pnpm](https://pnpm.io/) 10+
- [Rust](https://rustup.rs/) 1.75+

**Windows:**
- Visual Studio Build Tools 2022+ mit der Workload „Desktopentwicklung mit C++"
- WebView2 (vorinstalliert unter Windows 10 1803+ und Windows 11)

**macOS:**
- Xcode Command Line Tools: `xcode-select --install`

**Linux (Debian/Ubuntu):**
```bash
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file \
  libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev
```

```bash
git clone https://github.com/rollacode/codeterm.git
cd codeterm
pnpm install

# Entwicklung
pnpm tauri dev

# Produktions-Build
pnpm tauri build

# CLI-Tool
cd cli && cargo build --release
```

## Tastenkürzel

> Tastenkürzel verwenden Cmd auf macOS und Strg auf Windows/Linux. Anpassbar unter Einstellungen → Tastenkürzel.

| Tastenkürzel | Aktion |
|----------|--------|
| `Cmd/Strg+T` | Neuer Tab |
| `Cmd/Strg+W` | Tab schließen |
| `Cmd/Strg+Umschalt+→` | Rechts teilen |
| `Cmd/Strg+Umschalt+↓` | Nach unten teilen |
| `Cmd/Strg+Umschalt+Q` | Tab schließen |
| `Cmd/Strg+Umschalt+N` | Neuer Workspace |
| `Cmd/Strg+Umschalt+F` | Im Terminal suchen |
| `Strg+Tab` | Nächster Tab |
| `Strg+Umschalt+Tab` | Vorheriger Tab |
| `Cmd/Strg+1-9` | Zu Tab N wechseln |
| `Cmd/Strg+,` | Einstellungen |
| `Cmd/Strg+=` / `Cmd/Strg+-` | Vergrößern/Verkleinern |
| `Cmd/Strg+0` | Zoom zurücksetzen |
| `Alt+Pfeiltasten` | Tabs navigieren |

## API

CodeTerm stellt drei APIs zur Integration bereit:

### REST-API (Port 7685)

```bash
# Tabs auflisten
curl http://localhost:7685/api/tabs

# Befehl ausführen und Ausgabe erhalten
curl -X POST http://localhost:7685/api/tabs/{id}/exec \
  -H "Content-Type: application/json" \
  -d '{"command": "git status"}'

# Text an das Terminal senden
curl -X POST http://localhost:7685/api/tabs/{id}/send \
  -H "Content-Type: application/json" \
  -d '{"text": "npm test", "enter": true}'

# Workspace erstellen
curl -X POST http://localhost:7685/api/workspaces \
  -H "Content-Type: application/json" \
  -d '{"name": "Mein Projekt"}'

# Agent in einem neuen Tab neben dem übergeordneten Tab starten
curl -X POST http://localhost:7685/api/agents/spawn \
  -H "Content-Type: application/json" \
  -d '{"agent": "claude", "parent_tab_id": "TAB_ID"}'

# Ausgabe in Echtzeit streamen (SSE)
curl http://localhost:7685/api/tabs/{id}/output/stream
```

### CLI-Tool

```bash
# Workspace-Verwaltung
codeterm workspace list
codeterm workspace create "Projekt"

# Terminal-Steuerung
codeterm tab list
codeterm send --tab TAB_ID "npm run build"
codeterm get-output TAB_ID --lines 50

# Benachrichtigungen
codeterm notify "Build abgeschlossen" --level info

# Browser
codeterm browser open "http://localhost:3000"
```

### Umgebungsvariablen

CodeTerm setzt diese in jeder PTY-Sitzung:

| Variable | Beschreibung |
|----------|-------------|
| `CODETERM` | Immer `1` — erkennen, ob CodeTerm aktiv ist |
| `CODETERM_TAB_ID` | UUID des aktuellen Tabs |
| `CODETERM_SOCKET_PATH` | Socket-Pfad für die JSON-RPC-API |
| `TERM_PROGRAM` | `CodeTerm` |

Siehe [API-Referenz](docs/API.md) für die vollständige Endpunkt-Dokumentation.

## Konfiguration

Konfigurationsdatei: `%APPDATA%/codeterm/config.toml` (Windows) oder `~/.config/codeterm/config.toml`

```toml
[theme]
name = "github-dark"
# Verfügbar: github-dark, dracula, catppuccin-mocha, one-dark, nord,
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

Siehe [Konfigurationsanleitung](docs/CONFIG.md) für alle Optionen.

## Architektur

```
┌─────────────────────────────────────────────┐
│                  CodeTerm                    │
│  ┌──────────┐  ┌──────────┐  ┌───────────┐  │
│  │ React UI │  │ xterm.js │  │  Browser   │  │
│  │ Sidebar  │  │ Terminal │  │    Tabs    │  │
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

## Technologie-Stack

| Schicht | Technologie |
|-------|-----------|
| Framework | Tauri v2 |
| Backend | Rust, Axum, tokio |
| Frontend | React 19, TypeScript, Tailwind CSS 4 |
| Terminal | xterm.js 6 (WebGL-Renderer) |
| PTY | portable-pty, ConPTY (Windows) |
| Build | pnpm, Cargo |

## Plattformen

| Plattform | Status |
|----------|--------|
| Windows | Unterstützt |
| macOS | Unterstützt |
| Linux | Unterstützt |

## Lizenz

MIT

## Mitwirken

Beiträge sind willkommen. Bitte eröffnen Sie zuerst ein Issue, um zu besprechen, was Sie ändern möchten.
