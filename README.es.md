<p align="center">
  <img src="assets/logo.png" alt="CodeTerm" width="128" />
</p>

<h1 align="center">CodeTerm</h1>
> La traducción de README.md puede quedarse atrás. La versión en inglés es canónica.

<p align="center">
  <strong>Multiplexor de terminal orientado a IA</strong><br>
  Diseñado para desarrolladores que utilizan agentes de codificación con IA como parte de su flujo de trabajo.
</p>

<p align="center">
  <a href="#características">Características</a> &bull;
  <a href="#instalación">Instalación</a> &bull;
  <a href="#atajos-de-teclado">Atajos</a> &bull;
  <a href="#api">API</a> &bull;
  <a href="#configuración">Configuración</a> &bull;
  <a href="#compilación-desde-el-código-fuente">Compilar</a>
</p>

<p align="center">
  <a href="README.md">English</a> &bull;
  <a href="README.zh.md">中文</a> &bull;
  <a href="README.ja.md">日本語</a> &bull;
  <a href="README.ko.md">한국어</a> &bull;
  <a href="README.de.md">Deutsch</a> &bull;
  <a href="README.fr.md">Français</a> &bull;
  <a href="README.pt.md">Português</a> &bull;
  <a href="README.ru.md">Русский</a>
</p>

<!-- SCREENSHOT PLACEHOLDER: Add screenshots here -->
<!-- ![CodeTerm Screenshot](assets/screenshot.png) -->
<!-- TRANSLATION-TODO (2026-04-29 review): translation lags README.md — please mirror these sections from English:
     1. The two top-of-features GIF subsections (`### Agent Orchestration`, `### Cron Jobs`) shown right after the language nav.
     2. Per-OS install subsections instead of one combined "Descarga": `### macOS (Homebrew)`, `### Windows (Scoop)`, `### Windows (installer)`, `### Linux`.
     3. Rename the "Navegador Integrado" section to match English `### Canvas Tabls` (the feature was renamed). -->

---

## ¿Qué es CodeTerm?

CodeTerm es un multiplexor de terminal multiplataforma diseñado desde cero para el desarrollo asistido por IA. Combina la potencia de un multiplexor de terminal (como tmux) con soporte nativo para agentes de codificación con IA como Claude Code.

**Diferencia clave con otros terminales:** CodeTerm expone una API REST completa, CLI y API de sockets — para que los agentes de IA puedan crear espacios de trabajo, dividir tables, ejecutar comandos, leer la salida y gestionar todo el terminal de forma programática. Tu asistente de IA se convierte en un ciudadano de primera clase en tu flujo de trabajo en la terminal.

Construido con **Tauri v2** (Rust) + **React** + **xterm.js**.

## Características

### Multiplexor de Terminal
- **Espacios de trabajo y pestañas** — organiza proyectos en espacios de trabajo separados, cada uno con múltiples pestañas
- **División de tables** — divide horizontal y verticalmente con separadores arrastrables
- **Persistencia de sesión** — los espacios de trabajo, pestañas, diseño de tables y directorios de trabajo se guardan y restauran entre reinicios
- **Navegación espacial de tables** — muévete entre tables con Alt+Flechas

### Integración con Agentes de IA
- **Detección de agentes** — detecta automáticamente Claude Code ejecutándose en un tabl (indicador naranja)
- **Reanudación de sesión** — guarda los IDs de sesión de Claude Code y los reanuda al reiniciar con `--resume`
- **Notificaciones de permisos** — muestra las solicitudes de permisos de Claude Code como notificaciones de escritorio
- **Sistema de webhooks** — registra webhooks para eventos de agentes (solicitudes de permisos, finalización de tareas)
- **Control programático** — los agentes pueden crear tables, ejecutar comandos y leer la salida a través de la API
- **Reinicio de agente** — `codeterm agent restart --tab ID` ejecuta un `/exit` ordenado con auto-reanudación en cualquier pestaña (`--force` mata un agente colgado); reutiliza el mismo mecanismo que el auto-update

### API Completa
- **API REST** — más de 30 endpoints para control total del terminal (puerto 7685)
- **Herramienta CLI** — herramienta de línea de comandos `codeterm` para scripting y automatización
- **API de sockets** — JSON-RPC v2 a través de named pipes (Windows) / sockets Unix
- **Streaming en tiempo real** — endpoint SSE para salida del terminal en vivo
- **Captura de salida** — paginación basada en cursor, eliminación de ANSI, capturas de pantalla

### Puentes y servicios externos
- **Puente Telegram** — bidireccional. Los mensajes entrantes llegan al tabl del General Agent como `[Telegram: USER_ID] text`; las respuestas salen mediante `codeterm bridge send` con formato HTML (el sanitizador escapa automáticamente subcadenas `<word>` no soportadas para que tokens desconocidos no rompan el parser)
- **Transcripción de voz** — cuando `integrations.voice_transcribe.enabled = true`, los mensajes de voz / video-nota se transcriben mediante un daemon Whisper local (por defecto `http://127.0.0.1:7891`) antes de llegar a cualquier agente — Claude, Codex y futuros clientes quedan cubiertos con un único cambio aguas arriba
- **Puente de webhooks** — registra URLs para eventos de agente (`agent.permission_request`, `agent.task_complete`, `agent.report`)

### Auto-actualización y resiliencia
- **Auto-actualización** — `codeterm update --check / --apply / --channel <stable|rc|nightly>`. Los manifiestos están firmados con ed25519 y se verifican antes de tocar cualquier binario; la auto-instalación realiza un apagado ordenado de 9 pasos (checkpoint del WAL de mem, parada del supervisor, intercambio del binario firmado), tras lo cual los tables se auto-reanudan — el contexto sobrevive a la actualización
- **Supervisor de subprocesos** — las operaciones pesadas de mem (dreaming, extracción del Librarian) se ejecutan como procesos hijos con disyuntor (3 fallos en 60 s → abierto). La muerte de un worker no derriba la API; los timeouts `503` sobre solicitudes pesadas mantienen el daemon receptivo
- **Watchdog externo** — sonda de salud en `/api/health`; recetas de configuración para Windows Task Scheduler, launchd y systemd en [docs/RESILIENCE.md](docs/RESILIENCE.md)

### Temas
- **8 temas integrados** — GitHub Dark, Dracula, Catppuccin Mocha, One Dark, Nord, GitHub Light, Solarized Light, Pip-Boy
- **Modo CRT Pip-Boy** — líneas de escaneo, brillo de fósforo, parpadeo de pantalla (se activa automáticamente)
- **Tematización completa de la interfaz** — los temas se aplican a toda la interfaz, no solo a la terminal
- **Colores personalizados** — modifica colores individuales en `config.toml`
- **Soporte para temas claros** — la derivación automática de colores funciona tanto para temas claros como oscuros

### Navegador Integrado
- **Tables de navegador** — abre páginas web junto a tu terminal
- **Servicio de archivos locales** — renderiza HTML/visualizaciones locales en tables de navegador
- **Controles de navegación** — atrás, adelante, actualizar, barra de URL

### Experiencia de Desarrollo
- **Barras de desplazamiento estilo macOS** — finas, redondeadas, discretas
- **Arrastrar y soltar** — arrastra archivos a la terminal (los archivos de texto pegan el contenido, las imágenes se guardan en disco)
- **Autocompletado de pestañas** — motor de completado de shell integrado
- **Búsqueda** — Ctrl+Shift+F para buscar en la salida del terminal
- **Campanilla visual** — destello de pantalla en lugar de audio
- **Seguimiento de CWD** — rastrea el directorio de trabajo mediante secuencias de escape OSC 7

## Instalación

### Descarga

Los binarios precompilados estarán disponibles en la página de [Releases](https://github.com/rollacode/codeterm/releases).

### Compilación desde el Código Fuente

**Prerrequisitos:**
- [Node.js](https://nodejs.org/) 18+
- [pnpm](https://pnpm.io/) 10+
- [Rust](https://rustup.rs/) 1.75+

**Windows:**
- Visual Studio Build Tools 2022+ con la carga de trabajo "Desarrollo de escritorio con C++"
- WebView2 (preinstalado en Windows 10 1803+ y Windows 11)

**macOS:**
- Herramientas de línea de comandos de Xcode: `xcode-select --install`

**Linux (Debian/Ubuntu):**
```bash
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file \
  libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev
```

```bash
git clone https://github.com/rollacode/codeterm.git
cd codeterm
pnpm install

# Desarrollo
pnpm tauri dev

# Compilación de producción
pnpm tauri build

# Herramienta CLI
cd cli && cargo build --release
```

## Atajos de Teclado

> Los atajos usan Cmd en macOS y Ctrl en Windows/Linux. Se pueden personalizar en Configuración → Atajos de teclado.

| Atajo | Acción |
|-------|--------|
| `Cmd/Ctrl+T` | Nueva pestaña |
| `Cmd/Ctrl+W` | Cerrar pestaña |
| `Cmd/Ctrl+Shift+→` | Dividir a la derecha |
| `Cmd/Ctrl+Shift+↓` | Dividir hacia abajo |
| `Cmd/Ctrl+Shift+Q` | Cerrar tabl |
| `Cmd/Ctrl+Shift+N` | Nuevo espacio de trabajo |
| `Cmd/Ctrl+Shift+F` | Buscar en la terminal |
| `Ctrl+Tab` | Siguiente pestaña |
| `Ctrl+Shift+Tab` | Pestaña anterior |
| `Cmd/Ctrl+1-9` | Cambiar a pestaña N |
| `Cmd/Ctrl+,` | Configuración |
| `Cmd/Ctrl+=` / `Cmd/Ctrl+-` | Acercar/alejar |
| `Cmd/Ctrl+0` | Restablecer zoom |
| `Alt+Flechas` | Navegar entre tables |

## API

CodeTerm expone tres APIs para integración:

### API REST (puerto 7685)

```bash
# Listar tables
curl http://localhost:7685/api/tabs

# Ejecutar un comando y obtener la salida
curl -X POST http://localhost:7685/api/tabs/{id}/exec \
  -H "Content-Type: application/json" \
  -d '{"command": "git status"}'

# Enviar texto a la terminal
curl -X POST http://localhost:7685/api/tabs/{id}/send \
  -H "Content-Type: application/json" \
  -d '{"text": "npm test", "enter": true}'

# Crear espacio de trabajo
curl -X POST http://localhost:7685/api/workspaces \
  -H "Content-Type: application/json" \
  -d '{"name": "Mi Proyecto"}'

# Iniciar un agente en un nuevo tabl junto al tabl padre
curl -X POST http://localhost:7685/api/agents/spawn \
  -H "Content-Type: application/json" \
  -d '{"agent": "claude", "parent_tab_id": "TAB_ID"}'

# Streaming de salida en tiempo real (SSE)
curl http://localhost:7685/api/tabs/{id}/output/stream
```

### Herramienta CLI

```bash
# Gestión de espacios de trabajo
codeterm workspace list
codeterm workspace create "Project"

# Control de la terminal
codeterm tab list
codeterm send --tab TAB_ID "npm run build"
codeterm get-output TAB_ID --lines 50

# Notificaciones
codeterm notify "Build complete" --level info

# Navegador
codeterm browser open "http://localhost:3000"
```

### Variables de Entorno

CodeTerm establece estas variables en cada sesión PTY:

| Variable | Descripción |
|----------|-------------|
| `CODETERM` | Siempre `1` — detecta si se ejecuta dentro de CodeTerm |
| `CODETERM_TAB_ID` | UUID del tabl actual |
| `CODETERM_SOCKET_PATH` | Ruta del socket para la API JSON-RPC |
| `TERM_PROGRAM` | `CodeTerm` |

Consulta la [Referencia de API](docs/API.md) para la documentación completa de endpoints.

## Configuración

Archivo de configuración: `%APPDATA%/codeterm/config.toml` (Windows) o `~/.config/codeterm/config.toml`

```toml
[theme]
name = "github-dark"
# Disponibles: github-dark, dracula, catppuccin-mocha, one-dark, nord,
#              github-light, solarized-light, pip-boy

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

Consulta la [Guía de Configuración](docs/CONFIG.md) para todas las opciones.

## Arquitectura

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

## Stack Tecnológico

| Capa | Tecnología |
|------|-----------|
| Framework | Tauri v2 |
| Backend | Rust, Axum, tokio |
| Frontend | React 19, TypeScript, Tailwind CSS 4 |
| Terminal | xterm.js 6 (renderizador WebGL) |
| PTY | portable-pty, ConPTY (Windows) |
| Compilación | pnpm, Cargo |

## Plataformas

| Plataforma | Estado |
|------------|--------|
| Windows | Soportado |
| macOS | Soportado |
| Linux | Soportado |

## Licencia

MIT

## Contribuciones

Las contribuciones son bienvenidas. Por favor, abre un issue primero para discutir lo que te gustaría cambiar.
