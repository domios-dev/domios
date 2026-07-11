#!/usr/bin/env node
/**
 * Stub agent replayer — stands in for a real provider CLI inside a CodeTerm
 * pane when the daemon runs with CODETERM_STUB_AGENT_CMD (dev builds only).
 *
 * Invocation (built by src-tauri/src/agent/stub_launch.rs):
 *   node replay.mjs --stub-provider <id> -- <original provider argv...>
 *
 * Fidelity comes from replaying recorded real-session bytes (asciicast v2,
 * CODETERM_STUB_CAST) through the normal PTY pipeline; scripted turns answer
 * stdin directives so multi-turn/tool tests keep their oracles.
 */
import { randomUUID } from "node:crypto";
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO = path.join(HERE, "..", "..", "..", "..");

// Recorded real-session boot screens: detection fingerprints come from these
// bytes, so status/agent classification runs the provider's true code path.
const DEFAULT_CASTS = {
  claude:
    "testbench/replays/claude/geometry_smoke_v1-20260621-geometry-smoke-v2-claude-geometry_smoke_v1-100x30/raw.cast",
  codex:
    "testbench/replays/codex/codex_prompt_idle_regression_v1-20260621-codex-prompt-idle-v1/raw.cast",
  opencode:
    "testbench/replays/opencode/geometry_smoke_v1-20260621-geometry-smoke-v2-opencode-geometry_smoke_v1-100x30/raw.cast",
};

function parseArgv(argv) {
  const sep = argv.indexOf("--");
  const own = sep === -1 ? argv : argv.slice(0, sep);
  const original = sep === -1 ? [] : argv.slice(sep + 1);
  const provider = own[own.indexOf("--stub-provider") + 1] || "claude";
  const modelIdx = original.findIndex((a) => a === "--model" || a === "-m");
  const model = modelIdx !== -1 ? original[modelIdx + 1] : null;
  const resumeIdx = original.findIndex((a) => a === "--resume" || a === "-r");
  const resumeSid = resumeIdx !== -1 ? original[resumeIdx + 1] : null;
  return { provider, original, model, resumeSid };
}

const { provider, original, model, resumeSid } = parseArgv(process.argv.slice(2));
const sessionId = resumeSid || randomUUID();
const paneId = process.env.CODETERM_PANE_ID || "";
const apiUrl = (process.env.CODETERM_API_URL || "").replace(/\/$/, "");
const apiToken = process.env.CODETERM_API_TOKEN || "";

const STYLE = {
  claude: {
    banner: `[38;2;217;119;87m✳[0m Claude Code (stub replay)  model: ${model ?? "default"}`,
    turn: (text) => `[1m●[0m ${text}`,
    tool: (marker) =>
      `[2m⏺ Bash(echo ${marker})[0m\n[2m  ⎿  ${marker}[0m`,
    idle: "[2m❯[0m ",
  },
  codex: {
    banner: `[36m>_ Codex (stub replay)[0m  model: ${model ?? "default"}`,
    turn: (text) => `[1mcodex[0m  ${text}`,
    tool: (marker) => `[2mexec[0m echo ${marker}\n${marker}`,
    idle: "[2m▌[0m ",
  },
  opencode: {
    banner: `[35m█ opencode (stub replay)[0m  model: ${model ?? "default"}`,
    turn: (text) => `[1m@[0m ${text}`,
    tool: (marker) => `[2m$ echo ${marker}[0m\n${marker}`,
    idle: "[2m>[0m ",
  },
};
const style = STYLE[provider] ?? STYLE.claude;

function out(text) {
  process.stdout.write(text.endsWith("\n") ? text : `${text}\n`);
}

async function announceSession() {
  if (!apiUrl || !apiToken || !paneId) return;
  try {
    await fetch(`${apiUrl}/api/panes/${paneId}/hook/session_start`, {
      method: "POST",
      headers: { "Content-Type": "application/json", Authorization: `Bearer ${apiToken}` },
      body: JSON.stringify({
        session_id: sessionId,
        source: "startup",
        provider,
        binding_nonce: process.env.CODETERM_SESSION_BINDING_NONCE || undefined,
      }),
      signal: AbortSignal.timeout(5000),
    });
  } catch {
    /* detection tests will surface a missing sid */
  }
}

function replayCast() {
  const castPath = process.env.CODETERM_STUB_CAST || DEFAULT_CASTS[provider];
  if (!castPath) return;
  const resolved = path.isAbsolute(castPath) ? castPath : path.join(REPO, castPath);
  try {
    const lines = readFileSync(resolved, "utf8").split("\n").slice(1);
    for (const line of lines) {
      if (!line.trim()) continue;
      const [, kind, data] = JSON.parse(line);
      if (kind === "o") process.stdout.write(data);
    }
    process.stdout.write("\r\n");
  } catch (e) {
    out(`stub replay: cast unavailable (${String(e).slice(0, 80)})`);
  }
}

function handleLine(line) {
  const echo = line.match(/STUB_ECHO\s+(\S+)/);
  const tool = line.match(/STUB_TOOL\s+(\S+)/);
  if (line.trim() === "/exit") {
    out("");
    process.exit(0);
  } else if (echo) {
    out(style.turn(`ack ${echo[1]}`));
  } else if (tool) {
    out(style.tool(tool[1]));
    out(style.turn(`tool round-trip done ${tool[1]}`));
  } else if (line.trim()) {
    out(style.turn(`received: ${line.trim().slice(0, 120)}`));
  }
  process.stdout.write(style.idle);
}

out(style.banner);
out(`session: ${sessionId}${resumeSid ? " (resumed)" : ""}`);
if (original.length) out(`[2margv: ${original.join(" ")}[0m`);
replayCast();
await announceSession();
const initialTask = process.env.CODETERM_STUB_TASK;
if (initialTask) handleLine(initialTask);
process.stdout.write(style.idle);

let buf = "";
process.stdin.setEncoding("utf8");
process.stdin.on("data", (chunk) => {
  buf += chunk.replace(/\r/g, "\n");
  let nl;
  while ((nl = buf.indexOf("\n")) !== -1) {
    const line = buf.slice(0, nl);
    buf = buf.slice(nl + 1);
    handleLine(line);
  }
});
process.stdin.on("end", () => process.exit(0));
process.on("SIGINT", () => process.exit(0));
process.on("SIGTERM", () => process.exit(0));
