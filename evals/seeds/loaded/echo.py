"""Auto-split persona seeds (was inline in loaded.py).

Use _add() to append to this module's ITEMS — each persona keeps its own list,
which the loaded package concatenates in __init__.py.
"""

ITEMS: list[dict] = []


def _add(**kw):
    ITEMS.append(kw)


# ─────────────────────────────────────────────────────────────
# ECHO (CLI terminal tool being built) — ~20 items
# ─────────────────────────────────────────────────────────────

_add(kind="fact", title="Echo project stack",
    body="Echo is a Rust CLI with a Tauri v2 GUI wrapper. Backend Rust, frontend React + TypeScript, xterm.js for terminal rendering.",
    area="echo", importance=4, tags=["architecture"])

_add(kind="fact", title="Echo PTY implementation",
    body="Echo uses portable-pty crate for cross-platform PTY. ConPTY on Windows, native on Unix. Each tab has its own PTY handle.",
    area="echo", importance=4, tags=["pty"])

_add(kind="learning", title="parking_lot Mutex not reentrant",
    body="parking_lot::Mutex is not reentrant. Calling .lock() while already holding the lock on the same thread deadlocks immediately.",
    area="echo", importance=5, tags=["rust", "gotcha"])

_add(kind="learning", title="xterm.js renderer choice",
    body="xterm.js default canvas renderer is slow for 80+ columns. Switched to WebGL renderer (addon), 3x faster repaints but breaks on some old GPUs.",
    area="echo", importance=3, tags=["xterm"])

_add(kind="skill", title="Echo build command",
    body="./build.sh dev starts dev server with hot reload. ./build.sh build produces production artifacts. ./build.sh test runs unit tests.",
    area="echo", importance=4, tags=["dev"])

_add(kind="rule", title="Never kill production echo",
    body="When killing a dev process, identify by port (dev=7686, prod=7685). Only kill dev PID. Accidentally killing prod loses user session data.",
    area="echo", importance=5, tags=["safety", "rule"])

_add(kind="fact", title="Echo session storage",
    body="Echo persists sessions (tabs, cwd, command history) to ~/.echo/sessions/last.json. Reloaded on next start unless --new-session flag.",
    area="echo", importance=3, tags=["persistence"])

_add(kind="learning", title="Tauri v2 plugin architecture change",
    body="Tauri v2 moved from invoke/invoke_handler to the new plugin system. Old v1 commands need refactoring as plugin commands or exposed via the core namespace.",
    area="echo", importance=3, tags=["tauri"])

_add(kind="preference", title="Echo commit scope",
    body="Commit messages use scopes: feat(core), fix(ui), refactor(pty), etc. Breaking changes prefixed with !. Follows conventional commits.",
    area="echo", importance=2, tags=["git"])

_add(kind="skill", title="Echo release process",
    body="Bump version in Cargo.toml and package.json, tag v<version>, push tag. CI builds artifacts for macOS, Windows, Linux and uploads to GH release.",
    area="echo", importance=3, tags=["release"])

_add(kind="note", title="Switched from serde_json to simd-json",
    body="Profiling showed JSON parsing was 8% of startup time for large session files. Switched to simd-json, now 2%. No behavior changes.",
    area="echo", importance=2, tags=["perf"])

_add(kind="learning", title="React 18 StrictMode double effect",
    body="React 18 StrictMode runs effects twice in dev. Caused duplicate WebSocket connections. Guard with a useRef<boolean> or move to initializer.",
    area="echo", importance=4, tags=["react"])

_add(kind="rule", title="No unwrap in library code",
    body="Library crates must not contain .unwrap() or .expect() unless it's a documented invariant. Return Result<T, Error> and propagate.",
    area="echo", importance=4, tags=["rust", "rule"])

_add(kind="fact", title="Echo keybinding config",
    body="Keybindings configurable via ~/.echo/config.toml [keybinds] section. Defaults: Ctrl+T new tab, Ctrl+W close tab, Ctrl+Shift+P command palette.",
    area="echo", importance=3, tags=["config"])

_add(kind="conversation", title="Discussion about plugin API",
    body="Debated whether to expose a Wasm plugin runtime or just native dylib plugins. Decided: start with native, revisit Wasm after core is stable.",
    area="echo", importance=2, tags=["planning"])

# ─────────────────────────────────────────────────────────────
# EXTRA echo (CLI tool)
# ─────────────────────────────────────────────────────────────

_add(kind="fact", title="Echo agent providers",
    body="Echo supports multiple agent providers: Claude, Codex, Gemini. Each implemented as a trait in src/plugin/. Detection via OSC title and session files.",
    area="echo", importance=4, tags=["architecture"])

_add(kind="learning", title="OSC 7 cwd tracking",
    body="OSC 7 escape sequence reports current working directory from shell to terminal. Bash and zsh emit it natively. For PowerShell, inject via prompt function.",
    area="echo", importance=3, tags=["pty"])

_add(kind="note", title="Rust parallel compile investigation",
    body="Echo Rust build takes 4 minutes cold. Enabled sparse registry, cargo chef for CI layer caching, and jobs=8. Cold build now 90s.",
    area="echo", importance=3, tags=["rust", "perf"])

_add(kind="learning", title="Tokio runtime choice",
    body="Tokio multi-thread runtime shines for IO-bound workloads. For echo's single-user event loop, current-thread runtime is actually faster (no work stealing overhead).",
    area="echo", importance=3, tags=["tokio"])

_add(kind="skill", title="Echo unit test pattern",
    body="Tests inline in #[cfg(test)] mod tests { } at bottom of source file. Use tempfile::tempdir() for filesystem tests. Avoid real network — mock or bypass.",
    area="echo", importance=3, tags=["testing", "rust"])

_add(kind="rule", title="Lock ordering in echo",
    body="Acquire locks in order: mux → output_buf → screen_state → agent_tracker → pty. Never hold higher-numbered while acquiring lower. Deadlock prevention.",
    area="echo", importance=5, tags=["concurrency", "rule"])
