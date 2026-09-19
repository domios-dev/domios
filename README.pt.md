<p align="center">
  <img src="assets/logo.png" alt="CodeTerm" width="128" />
</p>

<h1 align="center">CodeTerm</h1>
> A tradução do README.md pode ficar atrasada. A versão em inglês é a canônica.

<p align="center">
  <strong>Multiplexador de terminal com foco em IA</strong><br>
  Feito para desenvolvedores que utilizam agentes de IA como parte do seu fluxo de trabalho.
</p>

<p align="center">
  <a href="#funcionalidades">Funcionalidades</a> &bull;
  <a href="#instalação">Instalação</a> &bull;
  <a href="#atalhos-de-teclado">Atalhos</a> &bull;
  <a href="#api">API</a> &bull;
  <a href="#configuração">Configuração</a> &bull;
  <a href="#compilando-a-partir-do-código-fonte">Compilar</a>
</p>

<p align="center">
  <a href="README.md">English</a> &bull;
  <a href="README.zh.md">中文</a> &bull;
  <a href="README.ja.md">日本語</a> &bull;
  <a href="README.ko.md">한국어</a> &bull;
  <a href="README.es.md">Español</a> &bull;
  <a href="README.de.md">Deutsch</a> &bull;
  <a href="README.fr.md">Français</a> &bull;
  <a href="README.ru.md">Русский</a>
</p>

<!-- SCREENSHOT PLACEHOLDER: Add screenshots here -->
<!-- ![CodeTerm Screenshot](assets/screenshot.png) -->
<!-- TRANSLATION-TODO (2026-04-29 review): translation lags README.md — please mirror these sections from English:
     1. The two top-of-features GIF subsections (`### Agent Orchestration`, `### Cron Jobs`) shown right after the language nav.
     2. Per-OS install subsections instead of one combined "Download": `### macOS (Homebrew)`, `### Windows (Scoop)`, `### Windows (installer)`, `### Linux`.
     3. Rename the "Navegador Integrado" section to match English `### Canvas Tabls` (the feature was renamed). -->

---

## O que é o CodeTerm?

CodeTerm é um multiplexador de terminal multiplataforma projetado desde o início para o desenvolvimento assistido por IA. Ele combina o poder de um multiplexador de terminal (como o tmux) com suporte nativo para agentes de codificação com IA, como o Claude Code.

**Diferencial em relação a outros terminais:** O CodeTerm expõe uma API REST completa, CLI e API de Socket — permitindo que agentes de IA criem workspaces, dividam painéis, executem comandos, leiam a saída e gerenciem o terminal inteiro de forma programática. Seu assistente de IA se torna um cidadão de primeira classe no seu fluxo de trabalho no terminal.

Construído com **Tauri v2** (Rust) + **React** + **xterm.js**.

## Funcionalidades

### Multiplexador de Terminal
- **Workspaces e abas** — organize projetos em workspaces separados, cada um com múltiplas abas
- **Divisão de painéis** — divida horizontalmente e verticalmente com divisores arrastáveis
- **Persistência de sessão** — workspaces, abas, layout de painéis e diretórios de trabalho são salvos e restaurados entre reinicializações
- **Navegação espacial de painéis** — mova-se entre painéis com Alt+Setas

### Integração com Agentes de IA
- **Detecção de agentes** — detecta automaticamente o Claude Code em execução em um painel (indicador laranja)
- **Retomada de sessão** — salva IDs de sessão do Claude Code e os retoma na reinicialização com `--resume`
- **Notificações de permissão** — exibe solicitações de permissão do Claude Code como notificações da área de trabalho
- **Sistema de webhooks** — registre webhooks para eventos de agentes (solicitações de permissão, conclusão de tarefas)
- **Controle programático** — agentes podem criar painéis, executar comandos, ler saída via API
- **Reinício de agente** — `codeterm agent restart --tab ID` executa um `/exit` ordenado com auto-retomada em qualquer aba (`--force` mata um agente travado); reutiliza o mesmo mecanismo da auto-atualização

### API Completa
- **API REST** — mais de 30 endpoints para controle completo do terminal (porta 7685)
- **Ferramenta CLI** — ferramenta de linha de comando `codeterm` para scripts e automação
- **API de Socket** — JSON-RPC v2 via named pipes (Windows) / Unix sockets
- **Streaming em tempo real** — endpoint SSE para saída ao vivo do terminal
- **Captura de saída** — paginação baseada em cursor, remoção de ANSI, snapshots de tela

### Pontes e serviços externos
- **Ponte Telegram** — bidirecional. Mensagens recebidas chegam ao painel do General Agent como `[Telegram: USER_ID] text`; respostas saem via `codeterm bridge send` com formatação HTML (o sanitizador escapa automaticamente subcadeias `<word>` não suportadas para que tokens desconhecidos não quebrem o parser)
- **Transcrição de voz** — quando `integrations.voice_transcribe.enabled = true`, mensagens de voz / nota de vídeo são transcritas por um daemon Whisper local (por padrão `http://127.0.0.1:7891`) antes de chegarem a qualquer agente — Claude, Codex e clientes futuros são cobertos por uma única mudança upstream
- **Ponte de webhooks** — registre URLs para eventos de agente (`agent.permission_request`, `agent.task_complete`, `agent.report`)

### Auto-atualização e resiliência
- **Auto-atualização** — `codeterm update --check / --apply / --channel <stable|rc|nightly>`. Manifestos são assinados com ed25519 e verificados antes de qualquer toque no binário; a auto-instalação executa um desligamento ordenado em 9 etapas (checkpoint do WAL de mem, parada do supervisor, troca do binário assinado), após o que os painéis se auto-retomam — o contexto sobrevive à atualização
- **Supervisor de subprocessos** — operações pesadas de mem (dreaming, extração do Librarian) rodam como processos filhos com disjuntor (3 falhas em 60 s → aberto). A morte de um worker não derruba a API; timeouts `503` em requisições pesadas mantêm o daemon responsivo
- **Recuperação de falhas** — o contrato de saúde do daemon e a responsabilidade pela reinicialização estão resumidos em [essences/core.md](essences/core.md#crash-recovery-supervisor)

### Temas
- **8 temas integrados** — GitHub Dark, Dracula, Catppuccin Mocha, One Dark, Nord, GitHub Light, Solarized Light, Pip-Boy
- **Modo CRT Pip-Boy** — scanlines, brilho de fósforo, tremulação de tela (ativado automaticamente)
- **Temas completos para a UI** — os temas se aplicam a toda a interface, não apenas ao terminal
- **Cores personalizadas** — sobrescreva cores individuais em `config.toml`
- **Suporte a temas claros** — derivação automática de cores funciona para temas claros e escuros

### Navegador Integrado
- **Painéis de navegador** — abra páginas web lado a lado com seu terminal
- **Servidor de arquivos locais** — renderize HTML/visualizações locais em painéis do navegador
- **Controles de navegação** — voltar, avançar, atualizar, barra de URL

### Experiência do Desenvolvedor
- **Barras de rolagem estilo macOS** — finas, arredondadas, discretas
- **Arrastar e soltar** — solte arquivos no terminal (arquivos de texto colam o conteúdo, imagens são salvas no disco)
- **Autocompletar** — mecanismo de autocompletar integrado ao shell
- **Pesquisa** — Ctrl+Shift+F para pesquisar na saída do terminal
- **Alerta visual** — flash na tela ao invés de áudio
- **Rastreamento de CWD** — rastreia o diretório de trabalho via sequências de escape OSC 7

## Instalação

### Download

Binários pré-compilados estarão disponíveis na página de [Releases](https://github.com/rollacode/codeterm/releases).

### Compilando a partir do código-fonte

**Pré-requisitos:**
- [Node.js](https://nodejs.org/) 18+
- [pnpm](https://pnpm.io/) 10+
- [Rust](https://rustup.rs/) 1.75+

**Windows:**
- Visual Studio Build Tools 2022+ com a carga de trabalho "Desenvolvimento para desktop com C++"
- WebView2 (pré-instalado no Windows 10 1803+ e Windows 11)

**macOS:**
- Ferramentas de linha de comando do Xcode: `xcode-select --install`

**Linux (Debian/Ubuntu):**
```bash
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file \
  libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev
```

```bash
git clone https://github.com/rollacode/codeterm.git
cd codeterm
pnpm install

# Desenvolvimento
pnpm tauri dev

# Build de produção
pnpm tauri build

# Ferramenta CLI
cd cli && cargo build --release
```

## Atalhos de Teclado

> Os atalhos usam Cmd no macOS e Ctrl no Windows/Linux. Personalizáveis em Configurações → Atalhos de teclado.

| Atalho | Ação |
|--------|------|
| `Cmd/Ctrl+T` | Nova aba |
| `Cmd/Ctrl+W` | Fechar aba |
| `Cmd/Ctrl+Shift+→` | Dividir à direita |
| `Cmd/Ctrl+Shift+↓` | Dividir abaixo |
| `Cmd/Ctrl+Shift+Q` | Fechar painel |
| `Cmd/Ctrl+Shift+N` | Novo workspace |
| `Cmd/Ctrl+Shift+F` | Pesquisar no terminal |
| `Ctrl+Tab` | Próxima aba |
| `Ctrl+Shift+Tab` | Aba anterior |
| `Cmd/Ctrl+1-9` | Ir para aba N |
| `Cmd/Ctrl+,` | Configurações |
| `Cmd/Ctrl+=` / `Cmd/Ctrl+-` | Aumentar/diminuir zoom |
| `Cmd/Ctrl+0` | Redefinir zoom |
| `Alt+Setas` | Navegar entre painéis |

## API

O CodeTerm expõe três APIs para integração:

### API REST (porta 7685)

```bash
# Listar painéis
curl http://localhost:7685/api/tabs

# Executar um comando e obter a saída
curl -X POST http://localhost:7685/api/tabs/{id}/exec \
  -H "Content-Type: application/json" \
  -d '{"command": "git status"}'

# Enviar texto ao terminal
curl -X POST http://localhost:7685/api/tabs/{id}/send \
  -H "Content-Type: application/json" \
  -d '{"text": "npm test", "enter": true}'

# Criar workspace
curl -X POST http://localhost:7685/api/workspaces \
  -H "Content-Type: application/json" \
  -d '{"name": "Meu Projeto"}'

# Iniciar um agente em um novo painel ao lado do painel pai
curl -X POST http://localhost:7685/api/agents/spawn \
  -H "Content-Type: application/json" \
  -d '{"agent": "claude", "parent_tab_id": "TAB_ID"}'

# Transmitir saída em tempo real (SSE)
curl http://localhost:7685/api/tabs/{id}/output/stream
```

### Ferramenta CLI

```bash
# Gerenciamento de workspaces
codeterm workspace list
codeterm workspace create "Projeto"

# Controle do terminal
codeterm tab list
codeterm send --tab TAB_ID "npm run build"
codeterm get-output TAB_ID --lines 50

# Notificações
codeterm notify send "Build concluído" --level info

# Navegador
codeterm browser open "http://localhost:3000"
```

### Variáveis de Ambiente

O CodeTerm define estas variáveis em cada sessão PTY:

| Variável | Descrição |
|----------|-----------|
| `CODETERM` | Sempre `1` — detecta se está rodando dentro do CodeTerm |
| `CODETERM_TAB_ID` | UUID do painel atual |
| `CODETERM_SOCKET_PATH` | Caminho do socket para a API JSON-RPC |
| `TERM_PROGRAM` | `CodeTerm` |

Consulte a [Referência da API](prod-docs/API.md) para a documentação completa dos endpoints.

## Configuração

Arquivo de configuração: `%APPDATA%/codeterm/config.toml` (Windows) ou `~/.config/codeterm/config.toml`

```toml
[theme]
name = "github-dark"
# Disponíveis: github-dark, dracula, catppuccin-mocha, one-dark, nord,
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
# token = "seu-token-secreto"
```

Consulte o [Guia de Configuração](prod-docs/CONFIG.md) para todas as opções.

## Arquitetura

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

## Stack Tecnológica

| Camada | Tecnologia |
|--------|-----------|
| Framework | Tauri v2 |
| Backend | Rust, Axum, tokio |
| Frontend | React 19, TypeScript, Tailwind CSS 4 |
| Terminal | xterm.js 6 (renderizador WebGL) |
| PTY | portable-pty, ConPTY (Windows) |
| Build | pnpm, Cargo |

## Plataformas

| Plataforma | Status |
|------------|--------|
| Windows | Suportado |
| macOS | Suportado |
| Linux | Suportado |

## Licença

MIT

## Contribuindo

Contribuições são bem-vindas. Por favor, abra uma issue primeiro para discutir o que você gostaria de alterar.
