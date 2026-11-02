import { execFile } from "node:child_process";
import { appendFileSync, mkdirSync, readFileSync, readdirSync, renameSync, statSync, unlinkSync, watch, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { bindFixtureSession } from "./setup.mjs";
import { readTab } from "./tabs.mjs";

const options = Object.fromEntries(process.argv.slice(2).reduce((pairs, value, index, all) => {
  if (index % 2 === 0) pairs.push([value.slice(2), all[index + 1]]);
  return pairs;
}, []));
const tab = process.env.CODETERM_TAB_ID;
const nonce = process.env.CODETERM_SESSION_BINDING_NONCE;
const token = process.env.CODETERM_API_TOKEN;
const api = process.env.CODETERM_API_URL;
const session = options.session;
if (!tab || !nonce || !token || !api || !process.env.CODETERM_PAD_DIR || !session) throw new Error("Fixture launch identity is incomplete");
const queue = join(process.env.CODETERM_PAD_DIR, "cj-runner", nonce);
const requests = join(queue, "requests"), results = join(queue, "results");
for (const path of [requests, results, options.sessions]) mkdirSync(path, { recursive: true, mode: 0o700 });
const redact = value => String(value ?? "").split(token).join("<redacted>");
const atomicJson = (path, value) => {
  writeFileSync(`${path}.next`, JSON.stringify(value), { mode: 0o600 });
  renameSync(`${path}.next`, path);
};
process.env.CODETERM_SESSION_ID = session;
process.env.CODETERM_INSTANCE = options.instance;
process.env.CODETERM_API_PORT = options.port;
process.env.CORE_JOURNEY_BUILD_CHECKOUT = options.checkout;

const prefixes = [["agent", "set-orchestrator"], ["plan", "create"], ["plan", "task", "add"],
  ["plan", "approve"],
  ["spec", "create"], ["spec", "section", "add"], ["plan", "goal", "set"], ["plan", "update"],
  ["plan", "task", "dispatch"], ["plan", "review", "bounce"], ["plan", "review", "accept"],
  ["plan", "rebind"], ["plan", "delete"]];
function allowed(args) {
  return Array.isArray(args) && args.length <= 100 && args.every(arg => typeof arg === "string" && arg.length <= 32768)
    && prefixes.some(prefix => prefix.every((part, index) => args[index] === part));
}
function execute(args) {
  const env = { ...process.env };
  for (const key of ["BASH_ENV", "SHELLOPTS", "BASHOPTS"]) delete env[key];
  return new Promise(resolve => {
    execFile("bash", ["--noprofile", "--norc", options.bench, "--dc", "--as-tab", tab, ...args],
      { env, timeout: 120_000, maxBuffer: 1024 * 1024, windowsHide: true },
      (error, stdout, stderr) => resolve({ code: error ? (Number.isInteger(error.code) ? error.code : 1) : 0,
        uncertain: Boolean(error && !Number.isInteger(error.code)), stdout: redact(stdout), stderr: redact(stderr) }));
  });
}
let pumping = false;
async function drain() {
  if (pumping) return;
  pumping = true;
  try {
    for (;;) {
      const entries = readdirSync(requests);
      if (entries.filter(name => /\.request(?:\.claimed)?$/.test(name)).length > 32) throw new Error("Fixture launch request bound exceeded");
      const pending = entries.filter(name => /^[a-f0-9-]{36}\.request$/.test(name)).sort();
      if (!pending.length) return;
      if (pending.length > 32) throw new Error("Fixture request bound exceeded");
      for (const name of pending) {
        const path = join(requests, name);
        if (statSync(path).size > 4 * 1024 * 1024) throw new Error("Fixture request exceeds its byte bound");
        const raw = readFileSync(path, "utf8");
        const request = JSON.parse(raw);
        try {
          writeFileSync(`${path}.claimed`, raw, { flag: "wx", mode: 0o600 });
        } catch (error) {
          if (error.code !== "EEXIST") throw error;
          unlinkSync(path);
          continue;
        }
        unlinkSync(path);
        const id = name.slice(0, -8);
        const result = request.tab_id === tab && request.binding_nonce === nonce && request.session_id === session && allowed(request.args)
          ? await execute(request.args) : { code: 1, stdout: "", stderr: "Fixture request identity or command refused" };
        atomicJson(join(results, `${id}.json`), { id, tab_id: tab, binding_nonce: nonce, session_id: session, ...result });
        if (result.uncertain) throw new Error("Fixture CLI completion is uncertain; no further requests admitted");
      }
    }
  } catch (error) {
    process.stderr.write(`fixture control failure: ${redact(error.message)}\n`);
    process.exitCode = 1;
    watcher.close();
  } finally { pumping = false; }
}

appendFileSync(join(options.sessions, `${session}.jsonl`), `${JSON.stringify({ session_id: session, tab_id: tab, binding_nonce: nonce })}\n`, { mode: 0o600 });
const scopedApi = async (path, { method = "GET", body } = {}) => {
  const response = await fetch(`${api}${path}`, { method,
    headers: { Authorization: `Bearer ${token}`, "Content-Type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body), signal: AbortSignal.timeout(30_000) });
  if (!response.ok) throw new Error(`Fixture API ${method} ${path} refused: ${response.status}`);
  return response.json();
};
await bindFixtureSession({ api: scopedApi }, tab, session, options.provider, () => readTab(scopedApi, tab));
const watcher = watch(requests, (_event, name) => { if (name?.toString().endsWith(".request")) void drain(); });
watcher.on("error", error => { process.stderr.write(`fixture watcher failed: ${redact(error.message)}\n`); process.exitCode = 1; watcher.close(); });
mkdirSync(dirname(options.discovery), { recursive: true });
atomicJson(options.discovery, { tab_id: tab, session_id: session, binding_nonce: nonce, queue, pid: process.pid });
process.stdout.write("Core journeys credential fixture ready\n");
void drain();
