<p align="center">
  <img src="assets/logo.png" alt="CodeTerm" width="128" />
</p>

<h1 align="center">CodeTerm</h1>

<p align="center">
  <strong>Multiplexeur de terminal orienté IA</strong><br>
  Conçu pour les développeurs qui utilisent des agents IA dans leur workflow.
</p>

<p align="center">
  <a href="#fonctionnalités">Fonctionnalités</a> &bull;
  <a href="#installation">Installation</a> &bull;
  <a href="#raccourcis-clavier">Raccourcis</a> &bull;
  <a href="#api">API</a> &bull;
  <a href="#configuration">Config</a> &bull;
  <a href="#compilation-depuis-les-sources">Compilation</a>
</p>

<p align="center">
  <a href="README.md">English</a> &bull;
  <a href="README.zh.md">中文</a> &bull;
  <a href="README.ja.md">日本語</a> &bull;
  <a href="README.ko.md">한국어</a> &bull;
  <a href="README.es.md">Español</a> &bull;
  <a href="README.de.md">Deutsch</a> &bull;
  <a href="README.pt.md">Português</a> &bull;
  <a href="README.ru.md">Русский</a>
</p>

<!-- SCREENSHOT PLACEHOLDER: Add screenshots here -->
<!-- ![CodeTerm Screenshot](assets/screenshot.png) -->

---

## Qu'est-ce que CodeTerm ?

CodeTerm est un multiplexeur de terminal multiplateforme conçu dès le départ pour le développement assisté par IA. Il combine la puissance d'un multiplexeur de terminal (comme tmux) avec une prise en charge native des agents de programmation IA tels que Claude Code.

**Différence clé par rapport aux autres terminaux :** CodeTerm expose une API REST complète, un CLI et une API Socket — permettant aux agents IA de créer des espaces de travail, diviser des panneaux, exécuter des commandes, lire la sortie et gérer l'ensemble du terminal de manière programmatique. Votre assistant IA devient un citoyen de première classe dans votre workflow terminal.

Construit avec **Tauri v2** (Rust) + **React** + **xterm.js**.

## Fonctionnalités

### Multiplexeur de terminal
- **Espaces de travail et onglets** — organisez vos projets dans des espaces de travail séparés, chacun avec plusieurs onglets
- **Division de panneaux** — division horizontale et verticale avec des séparateurs déplaçables
- **Persistance de session** — les espaces de travail, onglets, disposition des panneaux et répertoires de travail sont sauvegardés et restaurés entre les redémarrages
- **Navigation spatiale des panneaux** — naviguez entre les panneaux avec Alt+Flèches

### Intégration des agents IA
- **Détection d'agent** — détecte automatiquement Claude Code en cours d'exécution dans un panneau (indicateur orange)
- **Reprise de session** — sauvegarde les identifiants de session Claude Code et les reprend au redémarrage avec `--resume`
- **Notifications de permissions** — affiche les demandes de permissions de Claude Code sous forme de notifications de bureau
- **Système de webhooks** — enregistrez des webhooks pour les événements d'agent (demandes de permissions, achèvement de tâches)
- **Contrôle programmatique** — les agents peuvent créer des panneaux, exécuter des commandes et lire la sortie via l'API

### API complète
- **API REST** — plus de 30 points d'accès pour un contrôle total du terminal (port 7685)
- **Outil CLI** — outil en ligne de commande `codeterm` pour le scripting et l'automatisation
- **API Socket** — JSON-RPC v2 via des pipes nommés (Windows) / sockets Unix
- **Streaming en temps réel** — point d'accès SSE pour la sortie terminal en direct
- **Capture de sortie** — pagination par curseur, suppression ANSI, captures d'écran

### Thèmes
- **8 thèmes intégrés** — GitHub Dark, Dracula, Catppuccin Mocha, One Dark, Nord, GitHub Light, Solarized Light, Pip-Boy
- **Mode CRT Pip-Boy** — lignes de balayage, lueur phosphorescente, scintillement d'écran (activé automatiquement)
- **Thématisation complète de l'interface** — les thèmes s'appliquent à l'ensemble de l'interface, pas seulement au terminal
- **Couleurs personnalisées** — remplacez les couleurs individuelles dans `config.toml`
- **Support des thèmes clairs** — la dérivation automatique des couleurs fonctionne pour les thèmes clairs et sombres

### Navigateur intégré
- **Panneaux de navigateur** — ouvrez des pages web côte à côte avec votre terminal
- **Serveur de fichiers local** — affichez des fichiers HTML/visualisations locaux dans les panneaux de navigateur
- **Contrôles de navigation** — précédent, suivant, actualiser, barre d'URL

### Expérience développeur
- **Barres de défilement style macOS** — fines, arrondies, discrètes
- **Glisser-déposer** — déposez des fichiers dans le terminal (les fichiers texte collent le contenu, les images sont enregistrées sur le disque)
- **Complétion par tabulation** — moteur de complétion shell intégré
- **Recherche** — Ctrl+Shift+F pour rechercher dans la sortie du terminal
- **Cloche visuelle** — flash d'écran au lieu d'un son
- **Suivi du répertoire courant** — suit le répertoire de travail via les séquences d'échappement OSC 7

## Installation

### Téléchargement

Des binaires pré-compilés seront disponibles sur la page [Releases](https://github.com/rollacode/codeterm/releases).

### Compilation depuis les sources

**Prérequis :**
- [Node.js](https://nodejs.org/) 18+
- [pnpm](https://pnpm.io/) 10+
- [Rust](https://rustup.rs/) 1.75+

**Windows :**
- Visual Studio Build Tools 2022+ avec la charge de travail « Développement Desktop en C++ »
- WebView2 (préinstallé sur Windows 10 1803+ et Windows 11)

**macOS :**
- Outils en ligne de commande Xcode : `xcode-select --install`

**Linux (Debian/Ubuntu) :**
```bash
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file \
  libxdo-dev libssl-dev libayatana-appindicator3-dev librsvg2-dev
```

```bash
git clone https://github.com/rollacode/codeterm.git
cd codeterm
pnpm install

# Développement
pnpm tauri dev

# Compilation de production
pnpm tauri build

# Outil CLI
cd cli && cargo build --release
```

## Raccourcis clavier

| Raccourci | Action |
|-----------|--------|
| `Ctrl+Shift+T` | Nouvel onglet |
| `Ctrl+Shift+W` | Fermer l'onglet |
| `Ctrl+Shift+D` | Diviser à droite |
| `Ctrl+Shift+E` | Diviser en bas |
| `Ctrl+Shift+Q` | Fermer le panneau |
| `Ctrl+Shift+N` | Nouvel espace de travail |
| `Ctrl+Shift+B` | Ouvrir un panneau navigateur |
| `Ctrl+Shift+F` | Rechercher dans le terminal |
| `Ctrl+Shift+C` | Copier |
| `Ctrl+Shift+V` | Coller |
| `Ctrl+Tab` | Onglet suivant |
| `Ctrl+Shift+Tab` | Onglet précédent |
| `Ctrl+1-9` | Aller à l'onglet N |
| `Ctrl+,` | Paramètres |
| `Ctrl+=` / `Ctrl+-` | Zoom avant/arrière |
| `Ctrl+0` | Réinitialiser le zoom |
| `Alt+Flèches` | Naviguer entre les panneaux |

## API

CodeTerm expose trois API pour l'intégration :

### API REST (port 7685)

```bash
# Lister les panneaux
curl http://localhost:7685/api/panes

# Exécuter une commande et obtenir la sortie
curl -X POST http://localhost:7685/api/panes/{id}/exec \
  -H "Content-Type: application/json" \
  -d '{"command": "git status"}'

# Envoyer du texte au terminal
curl -X POST http://localhost:7685/api/panes/{id}/send \
  -H "Content-Type: application/json" \
  -d '{"text": "npm test", "enter": true}'

# Créer un espace de travail
curl -X POST http://localhost:7685/api/workspaces \
  -H "Content-Type: application/json" \
  -d '{"name": "Mon Projet"}'

# Diviser un panneau
curl -X POST http://localhost:7685/api/panes/{id}/split \
  -H "Content-Type: application/json" \
  -d '{"direction": "right"}'

# Streamer la sortie en temps réel (SSE)
curl http://localhost:7685/api/panes/{id}/output/stream
```

### Outil CLI

```bash
# Gestion des espaces de travail
codeterm workspace list
codeterm workspace create "Project"

# Contrôle du terminal
codeterm pane list
codeterm pane split PANE_ID --direction right
codeterm send-text PANE_ID "npm run build"
codeterm get-output PANE_ID --lines 50

# Notifications
codeterm notify "Build complete" --level info

# Navigateur
codeterm browser open "http://localhost:3000"
```

### Variables d'environnement

CodeTerm définit ces variables dans chaque session PTY :

| Variable | Description |
|----------|-------------|
| `CODETERM` | Toujours `1` — détecte si l'on s'exécute dans CodeTerm |
| `CODETERM_PANE_ID` | UUID du panneau courant |
| `CODETERM_SOCKET_PATH` | Chemin du socket pour l'API JSON-RPC |
| `TERM_PROGRAM` | `CodeTerm` |

Consultez la [Référence API](docs/API.md) pour la documentation complète des points d'accès.

## Configuration

Fichier de configuration : `%APPDATA%/codeterm/config.toml` (Windows) ou `~/.config/codeterm/config.toml`

```toml
[theme]
name = "github-dark"
# Disponibles : github-dark, dracula, catppuccin-mocha, one-dark, nord,
#               github-light, solarized-light, pip-boy

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

Consultez le [Guide de configuration](docs/CONFIG.md) pour toutes les options.

## Architecture

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

## Stack technique

| Couche | Technologie |
|--------|-------------|
| Framework | Tauri v2 |
| Backend | Rust, Axum, tokio |
| Frontend | React 19, TypeScript, Tailwind CSS 4 |
| Terminal | xterm.js 6 (moteur de rendu WebGL) |
| PTY | portable-pty, ConPTY (Windows) |
| Compilation | pnpm, Cargo |

## Plateformes

| Plateforme | Statut |
|------------|--------|
| Windows | Pris en charge |
| macOS | Pris en charge |
| Linux | Pris en charge |

## Licence

MIT

## Contribuer

Les contributions sont les bienvenues. Veuillez d'abord ouvrir une issue pour discuter des changements que vous souhaitez apporter.
