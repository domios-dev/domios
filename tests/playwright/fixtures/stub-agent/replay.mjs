#!/usr/bin/env node
/**
 * Stub agent replayer — stands in for a real provider CLI inside a CodeTerm
 * tab when the daemon runs with CODETERM_STUB_AGENT_CMD (dev builds only).
 *
 * Invocation (built by src-tauri/src/agent/stub_launch.rs):
 *   node replay.mjs --stub-provider <id> -- <original provider argv...>
 *
 * Fidelity comes from replaying recorded real-session bytes (asciicast v2,
 * CODETERM_STUB_CAST) through the normal PTY pipeline; scripted turns answer
 * stdin directives so multi-turn/tool tests keep their oracles.
 */
import { randomUUID } from "node:crypto";
import { appendFileSync, mkdirSync, readFileSync, realpathSync } from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { DEFAULT_REPLAY_BYTE_CAP, castOutput, defaultCastFor } from "./replay-cast.mjs";
import { styleFor } from "./replay-style.mjs";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const REPO = path.join(HERE, "..", "..", "..", "..");

function parseArgv(argv) {
  const sep = argv.indexOf("--");
  const own = sep === -1 ? argv : argv.slice(0, sep);
  const rest = sep === -1 ? [] : argv.slice(sep + 1);
  // The provider CLI puts the task positionally after its OWN `--`
  // (the shell has already expanded the task-file expression into text).
  const taskSep = rest.indexOf("--");
  const original = taskSep === -1 ? rest : rest.slice(0, taskSep);
  const task = taskSep === -1 ? null : rest.slice(taskSep + 1).join(" ");
  const provider = own[own.indexOf("--stub-provider") + 1] || "claude";
  const flag = (name) => {
    const i = original.findIndex((a) => a === name);
    return i !== -1 ? original[i + 1] : null;
  };
  const model = flag("--model") ?? flag("-m");
  const appendedTaskFile = flag("--append-system-prompt-file");
  const codexResumeSid =
    provider === "codex" && original[0] === "codex" && original[1] === "resume"
      ? original.slice(2).find((arg) => !arg.startsWith("-"))
      : null;
  const resumeSid =
    flag("--resume") ?? flag("-r") ?? flag("--session") ?? codexResumeSid;
  const presetSid = flag("--session-id");
  return {
    provider,
    original,
    model,
    resumeSid,
    presetSid,
    appendedTaskFile,
    task,
  };
}

const {
  provider,
  original,
  model,
  resumeSid,
  presetSid,
  appendedTaskFile,
  task,
} = parseArgv(process.argv.slice(2));
const sessionId = resumeSid || presetSid || randomUUID();
const tabId = process.env.CODETERM_TAB_ID || "";
const apiUrl = (process.env.CODETERM_API_URL || "").replace(/\/$/, "");
const apiToken = process.env.CODETERM_API_TOKEN || "";

const style = styleFor(provider, model);

function out(text) {
  process.stdout.write(text.endsWith("\n") ? text : `${text}\n`);
}

function writeComposer(draft = "") {
  const frame = provider === "codex" ? "OpenAI Codex · " : "";
  process.stdout.write(`\x1b[40;1H${frame}${style.idle}${draft}`);
}

function canonicalCwd() {
  try {
    return realpathSync(process.cwd());
  } catch {
    return process.cwd();
  }
}

function encodeClaudeCwd(cwd) {
  return Array.from(String(cwd || ""))
    .map((c) => (/[\p{L}\p{N}]/u.test(c) ? c : "-"))
    .join("");
}

async function announceSession() {
  if (!apiUrl || !apiToken || !tabId) return;
  try {
    await fetch(`${apiUrl}/api/tabs/${tabId}/hook/session_start`, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        Authorization: `Bearer ${apiToken}`,
      },
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
  const castPath = process.env.CODETERM_STUB_CAST || defaultCastFor(provider);
  if (!castPath) {
    // Silence here is what let a cast-less provider pass for a real boot.
    out(`stub replay: no recorded cast for provider ${provider}`);
    return;
  }
  const resolved = path.isAbsolute(castPath)
    ? castPath
    : path.join(REPO, castPath);
  try {
    const byteCap = Number(process.env.CODETERM_STUB_REPLAY_BYTE_CAP ?? DEFAULT_REPLAY_BYTE_CAP);
    process.stdout.write(castOutput(readFileSync(resolved, "utf8"), byteCap));
    process.stdout.write("\r\n");
  } catch (e) {
    out(`stub replay: cast unavailable (${String(e).slice(0, 80)})`);
  }
}

function readAppendedTask(pathname) {
  if (!pathname) return null;
  try {
    return readFileSync(pathname, "utf8").replace(/^\uFEFF/, "");
  } catch {
    return null;
  }
}

// Claude persists chats as jsonl in ~/.claude/projects/<dash-encoded-cwd>/;
// the tab chat tailer reads that file, so scripted turns must land there
// exactly like the real CLI writes them. Other providers: chat parity TBD.
const sessionFile = (() => {
  if (provider !== "claude") return null;
  const encoded = encodeClaudeCwd(canonicalCwd());
  const dir = path.join(os.homedir(), ".claude", "projects", encoded);
  try {
    mkdirSync(dir, { recursive: true });
    return path.join(dir, `${sessionId}.jsonl`);
  } catch {
    return null;
  }
})();

let turnSeq = 0;
function persistTurn(userText, assistantText) {
  if (!sessionFile) return;
  const ts = new Date().toISOString();
  const userUuid = randomUUID();
  const records = [
    {
      parentUuid: null,
      isSidechain: false,
      type: "user",
      message: { role: "user", content: userText },
      uuid: userUuid,
      timestamp: ts,
      sessionId,
      cwd: process.cwd(),
    },
    {
      parentUuid: userUuid,
      isSidechain: false,
      type: "assistant",
      message: {
        model: model ?? "stub",
        id: `msg_stub_${++turnSeq}`,
        type: "message",
        role: "assistant",
        content: [{ type: "text", text: assistantText }],
      },
      uuid: randomUUID(),
      timestamp: ts,
      sessionId,
      cwd: process.cwd(),
    },
  ];
  try {
    appendFileSync(
      sessionFile,
      records.map((r) => JSON.stringify(r)).join("\n") + "\n",
    );
  } catch {
    /* chat-parity test will surface a missing turn */
  }
}

function submitReport(message) {
  if (!apiUrl || !apiToken || !tabId) return;
  fetch(`${apiUrl}/api/tabs/${tabId}/report`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      Authorization: `Bearer ${apiToken}`,
    },
    body: JSON.stringify({ message }),
    signal: AbortSignal.timeout(10000),
  }).catch(() => {});
}

function handleLine(line) {
  const echo = line.match(/STUB_ECHO\s+(\S+)/);
  const tool = line.match(/STUB_TOOL\s+(\S+)/);
  const report = line.match(/STUB_REPORT\s+(\S+)/);
  if (line.trim() === "/exit") {
    out("");
    process.exit(0);
  } else if (report) {
    submitReport(report[1]);
    out(style.turn(`reported ${report[1]}`));
    persistTurn(line.trim(), `reported ${report[1]}`);
  } else if (echo) {
    out(style.turn(`ack ${echo[1]}`));
    persistTurn(line.trim(), `ack ${echo[1]}`);
  } else if (tool) {
    out(style.tool(tool[1]));
    out(style.turn(`tool round-trip done ${tool[1]}`));
    persistTurn(line.trim(), `tool round-trip done ${tool[1]}`);
  } else if (line.trim()) {
    out(style.turn(`received: ${line.trim().slice(0, 120)}`));
    persistTurn(line.trim(), `received: ${line.trim().slice(0, 120)}`);
  }
  writeComposer();
}

out(style.banner);
if (provider === "opencode") out("tab agents · ctrl+p commands");
out(`session: ${sessionId}${resumeSid ? " (resumed)" : ""}`);
if (original.length) out(`[2margv: ${original.join(" ")}[0m`);
replayCast();
await announceSession();
const initialTask =
  process.env.CODETERM_STUB_TASK || readAppendedTask(appendedTaskFile) || task;
if (initialTask) handleLine(initialTask.replace(/\r?\n/g, " "));
writeComposer();

let buf = "";
process.stdin.setEncoding("utf8");
process.stdin.setRawMode?.(true);
const rawInput = Boolean(process.stdin.isTTY);
let codexDraft = "";
let codexInput = "";
let rawDraft = "";
let rawInputBuffer = "";

function handleCodexInput(chunk) {
  codexInput = `${codexInput}${chunk}`
    .replaceAll("\u001b[?1;2c", "")
    .replaceAll("\u001b[0n", "")
    .replaceAll("\u001b[1;1R", "");
  while (codexInput) {
    if (codexInput.startsWith("\u001b[200~")) {
      const close = codexInput.indexOf("\u001b[201~", 6);
      if (close === -1) return;
      codexDraft += codexInput.slice(6, close).replace(/^ /, "");
      codexInput = codexInput.slice(close + 6);
      continue;
    }
    if ("\u001b[200~".startsWith(codexInput)) return;
    const terminator = codexInput.search(/[\r\n]/);
    if (terminator === -1) {
      codexDraft += codexInput.replace(/^ /, "");
      codexInput = "";
      return;
    }
    codexDraft += codexInput.slice(0, terminator).replace(/^ /, "");
    codexInput = codexInput.slice(terminator + 1);
    handleLine(codexDraft);
    codexDraft = "";
  }
}

function handleRawInput(chunk) {
  rawInputBuffer += chunk;
  let terminator = rawInputBuffer.search(/[\r\n]/);
  while (terminator !== -1) {
    rawDraft += rawInputBuffer.slice(0, terminator);
    rawInputBuffer = rawInputBuffer.slice(terminator + 1);
    handleLine(rawDraft);
    rawDraft = "";
    terminator = rawInputBuffer.search(/[\r\n]/);
  }
  rawDraft += rawInputBuffer;
  rawInputBuffer = "";
}

process.stdin.on("data", (chunk) => {
  if (provider === "codex") {
    handleCodexInput(chunk);
    return;
  }
  if (rawInput) {
    handleRawInput(chunk);
    return;
  }
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
