import { spawn } from "node:child_process";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export const root = resolve(dirname(fileURLToPath(import.meta.url)), "../../..");
const runner = join(root, "evals/core-journeys/run.sh");
const PROD_PORT = 7685;
const SHARED_DEV_PORT = 7686;

export function assertIsolated({ instance, port }) {
  if (!String(instance ?? "").trim()) throw new Error("Refused: CODETERM_INSTANCE is unset; core journeys run only on a named isolated instance");
  if (port === PROD_PORT) throw new Error("Refused: port 7685 is production");
  if (port === SHARED_DEV_PORT) throw new Error("Refused: port 7686 is the shared DEV instance; use a named instance with its own port");
}

export function target(env = process.env) {
  const port = Number(env.CODETERM_API_PORT);
  if (!env.CODETERM_DEV_DATA_DIR || !Number.isInteger(port)) {
    throw new Error("Run through evals/core-journeys/run.sh, which composes the DEV connection with dev_run");
  }
  assertIsolated({ instance: env.CODETERM_INSTANCE, port });
  return {
    port, instance: env.CODETERM_INSTANCE || "", dataDir: env.CODETERM_DEV_DATA_DIR,
    url: `http://127.0.0.1:${port}`, token: env.CODETERM_API_TOKEN,
  };
}

export class ApiError extends Error {
  constructor(message, status, body) {
    super(message);
    this.status = status;
    this.body = body;
  }
}

export function client(dev = target()) {
  const headers = { Authorization: `Bearer ${dev.token}`, "Content-Type": "application/json" };
  async function api(path, { method = "GET", body, signal = AbortSignal.timeout(30_000), extra = {} } = {}) {
    const response = await fetch(`${dev.url}${path}`, {
      method, headers: { ...headers, ...extra }, signal,
      body: body === undefined ? undefined : JSON.stringify(body),
    });
    const text = await response.text();
    let parsed = null;
    try { parsed = text ? JSON.parse(text) : null; } catch { parsed = text; }
    if (!response.ok) throw new ApiError(`${method} ${path}: HTTP ${response.status} ${text.slice(0, 600)}`, response.status, parsed);
    return parsed;
  }
  return { dev, headers, api };
}

// Every CLI call re-enters run.sh --dc so the lib's dev-locked `dc` owns target selection.
export function dc(args, { asTab, stdin, timeoutMs = 120_000, env = {} } = {}) {
  const argv = [runner, "--dc", ...(asTab ? ["--as-tab", asTab] : []), ...args];
  return new Promise((accept, reject) => {
    const child = spawn("bash", argv, {
      cwd: root, stdio: [stdin === undefined ? "ignore" : "pipe", "pipe", "pipe"],
      signal: AbortSignal.timeout(timeoutMs), env: { ...process.env, ...env },
    });
    let stdout = "", stderr = "";
    child.stdout.on("data", (chunk) => { stdout += chunk; });
    child.stderr.on("data", (chunk) => { stderr += chunk; });
    child.on("error", reject);
    child.on("close", (code) => accept({ code, stdout: stdout.trim(), stderr: stderr.trim() }));
    if (stdin !== undefined) child.stdin.end(stdin);
  });
}

export const unwrap = (value) => (value && typeof value === "object" && "data" in value && value.data !== undefined ? value.data : value);

export async function dcJson(args, options) {
  const result = await dc([...args, "--json"], options);
  let parsed;
  try { parsed = JSON.parse(result.stdout); } catch { parsed = undefined; }
  if (result.code !== 0) {
    throw Object.assign(new Error(`dc ${args.slice(0, 3).join(" ")} exit ${result.code}: ${(result.stdout || result.stderr).slice(0, 800)}`),
      { result, parsed });
  }
  if (parsed === undefined) throw new Error(`dc ${args.slice(0, 3).join(" ")} returned non-JSON: ${result.stdout.slice(0, 400)}`);
  return unwrap(parsed);
}

export class Events {
  constructor(dev, signal) {
    this.dev = dev;
    this.signal = signal;
    this.queue = [];
    this.waiter = null;
    this.failure = null;
    this.controller = new AbortController();
  }
  async open() {
    const response = await fetch(`${this.dev.url}/api/events/stream`, {
      headers: { Authorization: `Bearer ${this.dev.token}` },
      signal: AbortSignal.any([this.signal, this.controller.signal]),
    });
    if (!response.ok || !response.body) throw new Error(`SSE HTTP ${response.status}`);
    this.reader = response.body.pipeThrough(new TextDecoderStream()).getReader();
    this.consume().catch((error) => this.fail(error));
    return this;
  }
  fail(error) {
    this.failure ??= error;
    this.waiter?.reject(this.failure);
    this.waiter = null;
  }
  push(event) {
    if (this.waiter) {
      const waiter = this.waiter;
      this.waiter = null;
      waiter.resolve(event);
      return;
    }
    this.queue.push(event);
    if (this.queue.length > 20_000) throw new Error("SSE queue exceeded observation bound");
  }
  async consume() {
    let buffer = "";
    for (;;) {
      const { done, value } = await this.reader.read();
      if (done) throw new Error("SSE closed before the observation finished");
      buffer += value.replaceAll("\r", "");
      let boundary;
      while ((boundary = buffer.indexOf("\n\n")) >= 0) {
        const frame = buffer.slice(0, boundary);
        buffer = buffer.slice(boundary + 2);
        const data = frame.split("\n").filter((line) => line.startsWith("data:"))
          .map((line) => line.slice(5).trimStart()).join("\n");
        if (data) this.push(JSON.parse(data));
      }
    }
  }
  drain() { return this.queue.splice(0); }
  next() {
    if (this.failure) return Promise.reject(this.failure);
    if (this.queue.length) return Promise.resolve(this.queue.shift());
    if (this.signal.aborted) return Promise.reject(new Error("deadline"));
    return new Promise((accept, reject) => {
      const onAbort = () => { this.waiter = null; reject(new Error("deadline")); };
      this.signal.addEventListener("abort", onAbort, { once: true });
      const settle = (fn, value) => { this.signal.removeEventListener("abort", onAbort); fn(value); };
      this.waiter = { resolve: (event) => settle(accept, event), reject: (error) => settle(reject, error) };
    });
  }
  close() { this.controller.abort(); }
}
