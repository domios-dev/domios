// Transcriber plugin — CodeTerm's speech-to-text backend (capability: transcriber).
//
// CodeTerm records the voice note and inserts the returned text; this plugin owns
// the engine end to end. For the `local` engine it installs whisper.cpp's
// `whisper-server`, downloads a GGML model, and runs the daemon itself (via
// host.exec — blocking for one-shot installs, `detach` for the long-lived server).
// For `mesh` it just talks to a peer's daemon over the same wire contract.

function settings() {
  try {
    return JSON.parse(host.settingsJson()) || {};
  } catch (e) {
    return {};
  }
}

function isMesh() {
  return (settings().engine || "local") === "mesh";
}

function daemonUrl() {
  var s = settings();
  var url =
    (s.daemonUrl && s.daemonUrl.length && s.daemonUrl) ||
    host.envGet("CODETERM_TRANSCRIBE_URL") ||
    "http://127.0.0.1:7891";
  return url.replace(/\/+$/, "");
}

function port() {
  var m = daemonUrl().match(/:(\d+)(?:\/|$)/);
  return m ? m[1] : "7891";
}

function language() {
  var s = settings();
  if (s.language && s.language.length) return s.language;
  var lang = host.envGet("CODETERM_TRANSCRIBE_LANG");
  return lang && lang.length ? lang : "";
}

function modelId() {
  var s = settings();
  return s.model && s.model.length ? s.model : "base.en";
}

function modelDir() {
  var home = host.homeDir() || ".";
  return home + "/.codeterm/transcriber";
}

function modelPath() {
  return modelDir() + "/ggml-" + modelId() + ".bin";
}

function exec(opts) {
  var raw = host.exec(JSON.stringify(opts));
  try {
    return JSON.parse(raw);
  } catch (e) {
    return { error: String(e) };
  }
}

// ── Lifecycle (local engine) ─────────────────────────────────────────

function ensureEngine() {
  var check = exec({ bin: "whisper-server", args: ["--help"] });
  // present if it ran at all (help may exit 0 or 1, but no spawn error)
  if (!check.error) return { ok: true };
  var plat = host.platform();
  if (plat === "macos") {
    var r = exec({ bin: "brew", args: ["install", "whisper-cpp"] });
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

function ensureModel() {
  var mp = modelPath();
  if (host.fileExists(mp)) return { ok: true, path: mp };
  host.makeDirs(modelDir());
  var url =
    "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-" + modelId() + ".bin";
  var r = exec({ bin: "curl", args: ["-fsSL", "-o", mp, url] });
  if (r.error || r.code !== 0) {
    return { error: "model download failed: " + (r.stderr || r.error || "curl exit " + r.code) };
  }
  return { ok: true, path: mp };
}

function install() {
  if (isMesh()) return { ok: true };
  var e = ensureEngine();
  if (e.error) return e;
  var m = ensureModel();
  if (m.error) return m;
  return { ok: true };
}

function start() {
  if (isMesh()) return { ok: true };
  if (transcribeStatus().state === "ready") return { ok: true };
  var mp = modelPath();
  if (!host.fileExists(mp)) return { error: "model not installed — run Install first" };
  var r = exec({
    bin: "whisper-server",
    args: ["-m", mp, "--host", "127.0.0.1", "--port", port()],
    detach: true,
    logFile: modelDir() + "/server.log",
  });
  if (r.error) return { error: "could not start whisper-server: " + r.error };
  return { ok: true, pid: r.pid };
}

function stop() {
  if (isMesh()) return { ok: true };
  exec({ bin: "pkill", args: ["-f", "whisper-server"] });
  return { ok: true };
}

function ensureRunning() {
  if (!isMesh() && transcribeStatus().state !== "ready") start();
}

// ── Transcription wire (called by JsTranscriber) ─────────────────────

// path: absolute audio file path. lang: BCP-47 hint ("" = auto / plugin default).
function transcribe(path, lang) {
  ensureRunning();
  var body = { path: path };
  var l = lang && lang.length ? lang : language();
  if (l) body.language = l;

  var raw = host.fetch(JSON.stringify({
    url: daemonUrl() + "/transcribe",
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
    timeoutMs: 60000,
  }));

  var res;
  try {
    res = JSON.parse(raw);
  } catch (e) {
    return { error: "fetch returned non-JSON: " + e };
  }
  if (res.error) return { error: res.error };
  if (res.status && res.status >= 400) return { error: "transcribe HTTP " + res.status };

  try {
    var data = JSON.parse(res.body || "{}");
    return { text: typeof data.text === "string" ? data.text : "" };
  } catch (e) {
    return { error: "could not parse transcribe response: " + e };
  }
}

function transcribeStatus() {
  var raw = host.fetch(JSON.stringify({
    url: daemonUrl() + "/healthz",
    method: "GET",
    timeoutMs: 5000,
  }));
  try {
    var res = JSON.parse(raw);
    if (res.error) return { state: "unavailable", reason: res.error };
    if (res.status && res.status >= 400) return { state: "unavailable", reason: "HTTP " + res.status };
    return { state: "ready" };
  } catch (e) {
    return { state: "unavailable", reason: String(e) };
  }
}

// ── Glance (peek + lifecycle actions) ────────────────────────────────

function renderGlance() {
  var up = transcribeStatus().state === "ready";
  var installed = host.fileExists(modelPath());
  var nodes = [];

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

function glanceAction(action) {
  if (action === "install") install();
  else if (action === "start") start();
  else if (action === "stop") stop();
  else if (action === "restart") { stop(); start(); }
  return renderGlance();
}

module.exports.default = {
  transcribe: transcribe,
  transcribeStatus: transcribeStatus,
  renderGlance: renderGlance,
  glanceAction: glanceAction,
};
