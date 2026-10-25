import { spawn, spawnSync } from "node:child_process";

const INHERITED_SELECTORS = [
  "CODETERM_API_URL", "CODETERM_API_TOKEN", "CODETERM_API_PORT", "CODETERM_SOCKET_PATH", "CODETERM_INSTANCE",
  "CODETERM_DATA_DIR", "CODETERM_TAB_ID", "CODETERM_PARENT_TAB_ID", "CODETERM_SESSION_BINDING_NONCE",
];
const OUTPUT_CAP = 8 * 1024 * 1024;

export function targetEnv(target, base = process.env) {
  const env = { ...base };
  for (const key of INHERITED_SELECTORS) delete env[key];
  if (target.transport.kind === "local") {
    env.CODETERM_API_PORT = String(target.api_port);
    if (target.instance) env.CODETERM_INSTANCE = target.instance;
  }
  return env;
}

export function cliArgv(target, args) {
  const prefix = target.transport.kind === "prefix" ? target.transport.argv : [];
  return [...prefix, target.cli, ...args];
}

export function extractJson(stdout) {
  const text = String(stdout ?? "");
  for (let start = text.search(/[[{]/); start !== -1; start = nextStart(text, start)) {
    const end = matchingEnd(text, start);
    if (end === -1) continue;
    try {
      return JSON.parse(text.slice(start, end + 1));
    } catch { /* hint lines may contain braces; keep scanning */ }
  }
  return undefined;
}

function nextStart(text, from) {
  const rest = text.slice(from + 1).search(/[[{]/);
  return rest === -1 ? -1 : from + 1 + rest;
}

function matchingEnd(text, start) {
  const stack = [];
  let inString = false;
  for (let i = start; i < text.length; i += 1) {
    const c = text[i];
    if (inString) {
      if (c === "\\") i += 1;
      else if (c === "\"") inString = false;
      continue;
    }
    if (c === "\"") inString = true;
    else if (c === "{" || c === "[") stack.push(c === "{" ? "}" : "]");
    else if (c === "}" || c === "]") {
      if (stack.pop() !== c) return -1;
      if (stack.length === 0) return i;
    }
  }
  return -1;
}

function killTree(child) {
  if (child.exitCode !== null || child.signalCode !== null) return;
  if (process.platform === "win32") {
    spawnSync("taskkill", ["/PID", String(child.pid), "/T", "/F"], { stdio: "ignore", windowsHide: true });
  } else {
    try { process.kill(-child.pid, "SIGKILL"); } catch { child.kill("SIGKILL"); }
  }
}

// The deadline bounds failure only; completion is the child's own exit event.
export function runBounded(argv, { env = process.env, input, deadlineMs, spawnImpl = spawn, now = Date.now } = {}) {
  if (!Number.isInteger(deadlineMs) || deadlineMs <= 0) throw new Error("runBounded: deadlineMs required");
  const started = now();
  return new Promise((resolve) => {
    let child;
    try {
      child = spawnImpl(argv[0], argv.slice(1), { env, stdio: ["pipe", "pipe", "pipe"], detached: process.platform !== "win32", windowsHide: true });
    } catch (error) {
      resolve({ argv, exit: null, stdout: "", stderr: String(error.message), json: undefined, timedOut: false, spawnError: true, ms: now() - started });
      return;
    }
    let stdout = "";
    let stderr = "";
    let timedOut = false;
    let settled = false;
    const finish = (exit, spawnError = false) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      resolve({ argv, exit, stdout, stderr, json: extractJson(stdout), timedOut, spawnError, ms: now() - started });
    };
    const timer = setTimeout(() => {
      timedOut = true;
      killTree(child);
    }, deadlineMs);
    child.stdout.on("data", (d) => { if (stdout.length < OUTPUT_CAP) stdout += d; });
    child.stderr.on("data", (d) => { if (stderr.length < OUTPUT_CAP) stderr += d; });
    child.on("error", (error) => { stderr += String(error.message); finish(null, true); });
    child.on("close", (code) => finish(code));
    if (input !== undefined) child.stdin.end(input);
    else child.stdin.end();
  });
}

export function cliCommandExists(cliManifest, args) {
  let node = cliManifest;
  for (const token of args) {
    if (token.startsWith("-") || token.startsWith("{")) break;
    const next = (node?.subcommands ?? []).find((s) => s.name === token || (s.aliases ?? []).includes(token));
    if (!next) return (node?.args ?? []).some((a) => a.positional) && node !== cliManifest;
    node = next;
  }
  return node !== cliManifest;
}
