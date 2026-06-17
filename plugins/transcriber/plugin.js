"use strict";
var __defProp = Object.defineProperty;
var __getOwnPropDesc = Object.getOwnPropertyDescriptor;
var __getOwnPropNames = Object.getOwnPropertyNames;
var __hasOwnProp = Object.prototype.hasOwnProperty;
var __export = (target, all) => {
  for (var name in all)
    __defProp(target, name, { get: all[name], enumerable: true });
};
var __copyProps = (to, from, except, desc) => {
  if (from && typeof from === "object" || typeof from === "function") {
    for (let key of __getOwnPropNames(from))
      if (!__hasOwnProp.call(to, key) && key !== except)
        __defProp(to, key, { get: () => from[key], enumerable: !(desc = __getOwnPropDesc(from, key)) || desc.enumerable });
  }
  return to;
};
var __toCommonJS = (mod) => __copyProps(__defProp({}, "__esModule", { value: true }), mod);

// plugins/transcriber/src/plugin.ts
var plugin_exports = {};
__export(plugin_exports, {
  default: () => plugin_default
});
module.exports = __toCommonJS(plugin_exports);
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
  const s = settings();
  const url = s.daemonUrl && s.daemonUrl.length && s.daemonUrl || host.envGet("CODETERM_TRANSCRIBE_URL") || "http://127.0.0.1:7891";
  return url.replace(/\/+$/, "");
}
function port() {
  const m = daemonUrl().match(/:(\d+)(?:\/|$)/);
  return m ? m[1] : "7891";
}
function language() {
  const s = settings();
  if (s.language && s.language.length) return s.language;
  const lang = host.envGet("CODETERM_TRANSCRIBE_LANG");
  return lang && lang.length ? lang : "";
}
function modelId() {
  const s = settings();
  return s.model && s.model.length ? s.model : "base.en";
}
function modelDir() {
  const home = host.homeDir() || ".";
  return home + "/.codeterm/transcriber";
}
function modelPath() {
  return modelDir() + "/ggml-" + modelId() + ".bin";
}
function exec(opts) {
  const raw = host.exec(JSON.stringify(opts));
  try {
    return JSON.parse(raw);
  } catch (e) {
    return { error: String(e) };
  }
}
function ensureEngine() {
  const check = exec({ bin: "whisper-server", args: ["--help"] });
  if (!check.error) return { ok: true };
  const plat = host.platform();
  if (plat === "macos") {
    const r = exec({ bin: "brew", args: ["install", "whisper-cpp"] });
    if (r.error) return { error: "brew install failed: " + r.error };
    if (r.code !== 0) return { error: "brew install whisper-cpp exited " + r.code + ": " + (r.stderr || "") };
    return { ok: true };
  }
  return {
    error: "Auto-install runs on macOS (brew install whisper-cpp). On " + plat + ", install whisper.cpp's whisper-server yourself, then set the daemon URL."
  };
}
function ensureModel() {
  const mp = modelPath();
  if (host.fileExists(mp)) return { ok: true, path: mp };
  host.makeDirs(modelDir());
  const url = "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-" + modelId() + ".bin";
  const r = exec({ bin: "curl", args: ["-fsSL", "-o", mp, url] });
  if (r.error || r.code !== 0) {
    return { error: "model download failed: " + (r.stderr || r.error || "curl exit " + r.code) };
  }
  return { ok: true, path: mp };
}
function install() {
  if (isMesh()) return { ok: true };
  const e = ensureEngine();
  if ("error" in e) return e;
  const m = ensureModel();
  if ("error" in m) return m;
  return { ok: true };
}
function start() {
  if (isMesh()) return { ok: true };
  if (transcribeStatus().state === "ready") return { ok: true };
  const mp = modelPath();
  if (!host.fileExists(mp)) return { error: "model not installed \u2014 run Install first" };
  const r = exec({
    bin: "whisper-server",
    args: ["-m", mp, "--host", "127.0.0.1", "--port", port()],
    detach: true,
    logFile: modelDir() + "/server.log"
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
function transcribe(path, lang) {
  ensureRunning();
  const body = { path };
  const l = lang && lang.length ? lang : language();
  if (l) body.language = l;
  const raw = host.fetch(JSON.stringify({
    url: daemonUrl() + "/transcribe",
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(body),
    timeoutMs: 6e4
  }));
  let res;
  try {
    res = JSON.parse(raw);
  } catch (e) {
    return { error: "fetch returned non-JSON: " + e };
  }
  if (res.error) return { error: res.error };
  if (res.status && res.status >= 400) return { error: "transcribe HTTP " + res.status };
  try {
    const data = JSON.parse(res.body || "{}");
    return { text: typeof data.text === "string" ? data.text : "" };
  } catch (e) {
    return { error: "could not parse transcribe response: " + e };
  }
}
function transcribeStatus() {
  const raw = host.fetch(JSON.stringify({
    url: daemonUrl() + "/healthz",
    method: "GET",
    timeoutMs: 5e3
  }));
  try {
    const res = JSON.parse(raw);
    if (res.error) return { state: "unavailable", reason: res.error };
    if (res.status && res.status >= 400) return { state: "unavailable", reason: "HTTP " + res.status };
    return { state: "ready" };
  } catch (e) {
    return { state: "unavailable", reason: String(e) };
  }
}
function renderGlance() {
  const up = transcribeStatus().state === "ready";
  const installed = host.fileExists(modelPath());
  const nodes = [];
  if (isMesh()) {
    nodes.push({ kind: "badge", label: up ? "Peer up" : "Peer down", tone: up ? "ok" : "danger" });
    nodes.push({ kind: "keyVal", key: "Engine", value: "Mesh peer" });
    nodes.push({ kind: "keyVal", key: "Daemon", value: daemonUrl() });
    nodes.push({ kind: "divider" });
    nodes.push({ kind: "button", label: "Settings", action: "settings" });
    return { title: "Transcriber", nodes };
  }
  nodes.push({
    kind: "badge",
    label: up ? "Engine up" : installed ? "Stopped" : "Not installed",
    tone: up ? "ok" : installed ? "warn" : "danger"
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
        { kind: "button", label: "Stop", action: "stop", style: { tone: "danger" } }
      ]
    });
  }
  nodes.push({ kind: "button", label: "Settings", action: "settings" });
  return { title: "Transcriber", nodes };
}
function glanceAction(action) {
  if (action === "install") install();
  else if (action === "start") start();
  else if (action === "stop") stop();
  else if (action === "restart") {
    stop();
    start();
  }
  return renderGlance();
}
var plugin = {
  transcribe,
  transcribeStatus,
  renderGlance,
  glanceAction
};
var plugin_default = plugin;
