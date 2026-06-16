// Transcriber plugin — CodeTerm's speech-to-text backend (capability: transcriber).
//
// CodeTerm records the voice note and inserts the returned text; this plugin only
// turns audio into text. The engine is external (no model ships here): it POSTs the
// audio path to a transcribe daemon over `host.fetch`. Engine location:
//   - local  (default): a daemon on this machine (CODETERM_TRANSCRIBE_URL or :7891)
//   - mesh   (future):  delegate to a peer's daemon — same wire contract
//
// Settings UI (engine/model/watchdog) is rendered by CodeTerm from the schema and
// surfaced to the plugin via env for now; the full settings bridge lands with the
// Extensions UX.

function settings() {
  try {
    return JSON.parse(host.settingsJson()) || {};
  } catch (e) {
    return {};
  }
}

function daemonUrl() {
  var s = settings();
  var url =
    (s.daemonUrl && s.daemonUrl.length && s.daemonUrl) ||
    host.envGet("CODETERM_TRANSCRIBE_URL") ||
    "http://127.0.0.1:7891";
  return url.replace(/\/+$/, "");
}

function language() {
  var s = settings();
  if (s.language && s.language.length) return s.language;
  var lang = host.envGet("CODETERM_TRANSCRIBE_LANG");
  return lang && lang.length ? lang : "";
}

// path: absolute audio file path. lang: BCP-47 hint ("" = auto / plugin default).
function transcribe(path, lang) {
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
  // Liveness check via the daemon's /healthz; unreachable → unavailable.
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

module.exports.default = {
  transcribe: transcribe,
  transcribeStatus: transcribeStatus,
};
