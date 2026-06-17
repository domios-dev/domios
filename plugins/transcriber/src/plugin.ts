// Transcriber plugin — CodeTerm's speech-to-text backend (capability: transcriber).
// Authored in TypeScript against @codeterm/plugin-sdk and compiled to a
// QuickJS-compatible plugin.js by scripts/build-plugin.mjs.
//
// CodeTerm records the voice note and inserts the returned text; this plugin owns
// the engine end to end. For the `local` engine it installs whisper.cpp's
// `whisper-server`, downloads a GGML model, and runs the daemon itself (via
// host.exec — blocking for one-shot installs, `detach` for the long-lived server).
// For `mesh` it just talks to a peer's daemon over the same wire contract.
import type {
  ExecOpts,
  ExecResult,
  FetchResult,
  GlanceView,
  PluginModule,
  TranscribeResult,
  ViewNode,
} from "@codeterm/plugin-sdk";

interface TranscriberSettings {
  engine?: string;
  daemonUrl?: string;
  language?: string;
  model?: string;
}

type LifecycleResult = { ok: true; pid?: number; path?: string } | { error: string };

interface StatusResult {
  state: "ready" | "unavailable";
  reason?: string;
}

function settings(): TranscriberSettings {
  try {
    return (JSON.parse(host.settingsJson()) as TranscriberSettings) || {};
  } catch (e) {
    return {};
  }
}

function isMesh(): boolean {
  return (settings().engine || "local") === "mesh";
}

function daemonUrl(): string {
  const s = settings();
  const url =
    (s.daemonUrl && s.daemonUrl.length && s.daemonUrl) ||
    host.envGet("CODETERM_TRANSCRIBE_URL") ||
    "http://127.0.0.1:7891";
  return url.replace(/\/+$/, "");
}

function port(): string {
  const m = daemonUrl().match(/:(\d+)(?:\/|$)/);
  return m ? m[1] : "7891";
}

function language(): string {
  const s = settings();
  if (s.language && s.language.length) return s.language;
  const lang = host.envGet("CODETERM_TRANSCRIBE_LANG");
  return lang && lang.length ? lang : "";
}

function modelId(): string {
  const s = settings();
  return s.model && s.model.length ? s.model : "base.en";
}

function modelDir(): string {
  const home = host.homeDir() || ".";
  return home + "/.codeterm/transcriber";
}

function modelPath(): string {
  return modelDir() + "/ggml-" + modelId() + ".bin";
}

function exec(opts: ExecOpts): ExecResult {
  const raw = host.exec(JSON.stringify(opts));
  try {
    return JSON.parse(raw) as ExecResult;
  } catch (e) {
    return { error: String(e) };
  }
}

// ── Lifecycle (local engine) ─────────────────────────────────────────

function ensureEngine(): LifecycleResult {
  const check = exec({ bin: "whisper-server", args: ["--help"] });
  // present if it ran at all (help may exit 0 or 1, but no spawn error)
  if (!check.error) return { ok: true };
  const plat = host.platform();
  if (plat === "macos") {
    const r = exec({ bin: "brew", args: ["install", "whisper-cpp"] });
    if (r.error) return { error: "brew install failed: " + r.error };
    if (r.code !== 0) return { error: "brew install whisper-cpp exited " + r.code + ": " + (r.stderr || "") };
    return { ok: true };
  }
  return {
    error:
      "Auto-install runs on macOS (brew install whisper-cpp). On " +
      plat +
      ", install whisper.cpp's whisper-server yourself, then set the daemon URL.",
  };
}

function ensureModel(): LifecycleResult {
  const mp = modelPath();
  if (host.fileExists(mp)) return { ok: true, path: mp };
  host.makeDirs(modelDir());
  const url =
    "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-" + modelId() + ".bin";
  const r = exec({ bin: "curl", args: ["-fsSL", "-o", mp, url] });
  if (r.error || r.code !== 0) {
    return { error: "model download failed: " + (r.stderr || r.error || "curl exit " + r.code) };
  }
  return { ok: true, path: mp };
}

function install(): LifecycleResult {
  if (isMesh()) return { ok: true };
  const e = ensureEngine();
  if ("error" in e) return e;
  const m = ensureModel();
  if ("error" in m) return m;
  return { ok: true };
}

function start(): LifecycleResult {
  if (isMesh()) return { ok: true };
  if (transcribeStatus().state === "ready") return { ok: true };
  const mp = modelPath();
  if (!host.fileExists(mp)) return { error: "model not installed — run Install first" };
  const r = exec({
    bin: "whisper-server",
    args: ["-m", mp, "--host", "127.0.0.1", "--port", port()],
    detach: true,
    logFile: modelDir() + "/server.log",
  });
  if (r.error) return { error: "could not start whisper-server: " + r.error };
  return { ok: true, pid: r.pid };
}

function stop(): LifecycleResult {
  if (isMesh()) return { ok: true };
  exec({ bin: "pkill", args: ["-f", "whisper-server"] });
  return { ok: true };
}

function ensureRunning(): void {
  if (!isMesh() && transcribeStatus().state !== "ready") start();
}

// ── Transcription wire (called by JsTranscriber) ─────────────────────

// path: absolute audio file path. lang: BCP-47 hint ("" = auto / plugin default).
function transcribe(path: string, lang?: string): TranscribeResult {
  ensureRunning();
  const body: { path: string; language?: string } = { path: path };
  const l = lang && lang.length ? lang : language();
  if (l) body.language = l;

  const raw = host.fetch(JSON.stringify({
    url: daemonUrl() + "/transcribe",
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
    timeoutMs: 60000,
  }));

  let res: FetchResult;
  try {
    res = JSON.parse(raw) as FetchResult;
  } catch (e) {
    return { error: "fetch returned non-JSON: " + e };
  }
  if (res.error) return { error: res.error };
  if (res.status && res.status >= 400) return { error: "transcribe HTTP " + res.status };

  try {
    const data = JSON.parse(res.body || "{}") as { text?: unknown };
    return { text: typeof data.text === "string" ? data.text : "" };
  } catch (e) {
    return { error: "could not parse transcribe response: " + e };
  }
}

function transcribeStatus(): StatusResult {
  const raw = host.fetch(JSON.stringify({
    url: daemonUrl() + "/healthz",
    method: "GET",
    timeoutMs: 5000,
  }));
  try {
    const res = JSON.parse(raw) as FetchResult;
    if (res.error) return { state: "unavailable", reason: res.error };
    if (res.status && res.status >= 400) return { state: "unavailable", reason: "HTTP " + res.status };
    return { state: "ready" };
  } catch (e) {
    return { state: "unavailable", reason: String(e) };
  }
}

// ── Glance (peek + lifecycle actions) ────────────────────────────────

function renderGlance(): GlanceView {
  const up = transcribeStatus().state === "ready";
  const installed = host.fileExists(modelPath());
  const nodes: ViewNode[] = [];

  if (isMesh()) {
    nodes.push({ kind: "badge", label: up ? "Peer up" : "Peer down", tone: up ? "ok" : "danger" });
    nodes.push({ kind: "keyVal", key: "Engine", value: "Mesh peer" });
    nodes.push({ kind: "keyVal", key: "Daemon", value: daemonUrl() });
    nodes.push({ kind: "divider" });
    nodes.push({ kind: "button", label: "Settings", action: "settings" });
    return { title: "Transcriber", nodes: nodes };
  }

  nodes.push({
    kind: "badge",
    label: up ? "Engine up" : installed ? "Stopped" : "Not installed",
    tone: up ? "ok" : installed ? "warn" : "danger",
  });
  nodes.push({ kind: "keyVal", key: "Model", value: modelId() });
  nodes.push({ kind: "keyVal", key: "Daemon", value: daemonUrl() });
  nodes.push({ kind: "divider" });
  if (!installed) {
    nodes.push({ kind: "button", label: "Install engine + model", action: "install" });
  } else if (!up) {
    nodes.push({ kind: "button", label: "Start engine", action: "start" });
  } else {
    nodes.push({
      kind: "row",
      style: { spacing: "sm" },
      children: [
        { kind: "button", label: "Restart", action: "restart" },
        { kind: "button", label: "Stop", action: "stop", style: { tone: "danger" } },
      ],
    });
  }
  nodes.push({ kind: "button", label: "Settings", action: "settings" });
  return { title: "Transcriber", nodes: nodes };
}

function glanceAction(action: string): GlanceView {
  if (action === "install") install();
  else if (action === "start") start();
  else if (action === "stop") stop();
  else if (action === "restart") { stop(); start(); }
  return renderGlance();
}

const plugin: PluginModule = {
  transcribe,
  transcribeStatus,
  renderGlance,
  glanceAction,
};

export default plugin;
