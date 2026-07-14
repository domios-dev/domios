import fs from "node:fs";
import os from "node:os";
import path from "node:path";

const port = Number(process.env.CODETERM_API_PORT ?? 7686);
if (port === 7685) throw new Error("Refusing to run against prod port 7685");

const api = `http://127.0.0.1:${port}`;
const dataDir = process.env.CODETERM_DATA_DIR ?? path.join(os.homedir(), ".codeterm-dev");
const config = fs.readFileSync(path.join(dataDir, "config.toml"), "utf8");
const token = config.match(/^\s*token\s*=\s*"([^"]+)"/m)?.[1];
if (!token) throw new Error(`No API token in ${dataDir}/config.toml`);

const headers = { Authorization: `Bearer ${token}` };
const jsonHeaders = { ...headers, "Content-Type": "application/json" };
const requested = new Set(process.argv.slice(2));

async function request(url, init = {}) {
  const response = await fetch(`${api}${url}`, { ...init, headers: { ...headers, ...init.headers } });
  if (!response.ok) throw new Error(`${init.method ?? "GET"} ${url}: ${response.status} ${await response.text()}`);
  const text = await response.text();
  return text ? JSON.parse(text) : null;
}

async function eventStream() {
  const controller = new AbortController();
  const events = [];
  const waiters = new Set();
  let failure;

  const response = await fetch(`${api}/api/events/stream`, { headers, signal: controller.signal });
  if (!response.ok || !response.body) throw new Error(`events stream: ${response.status}`);

  void (async () => {
    const reader = response.body.pipeThrough(new TextDecoderStream()).getReader();
    let pending = "";
    while (true) {
      const { value, done } = await reader.read();
      if (done) break;
      pending += value;
      const frames = pending.split("\n\n");
      pending = frames.pop() ?? "";
      for (const frame of frames) {
        const data = frame
          .split(/\r?\n/)
          .filter((line) => line.startsWith("data:"))
          .map((line) => line.slice(5).trimStart())
          .join("\n");
        if (!data) continue;
        const event = JSON.parse(data);
        events.push(event);
        for (const waiter of [...waiters]) waiter(event);
      }
    }
    throw new Error("events stream ended");
  })().catch((error) => {
    if (!controller.signal.aborted) failure = error;
    for (const waiter of [...waiters]) waiter(null);
  });

  async function waitFor(predicate, timeoutMs) {
    const existingIndex = events.findIndex(predicate);
    if (existingIndex >= 0) return events.splice(existingIndex, 1)[0];
    if (failure) throw failure;
    const deadline = AbortSignal.timeout(timeoutMs);
    return new Promise((resolve, reject) => {
      const onEvent = (event) => {
        if (failure) return finish(() => reject(failure));
        if (event && predicate(event)) {
          const eventIndex = events.indexOf(event);
          if (eventIndex >= 0) events.splice(eventIndex, 1);
          finish(() => resolve(event));
        }
      };
      const onAbort = () => finish(() => reject(new Error(`event deadline ${timeoutMs}ms exceeded`)));
      const finish = (complete) => {
        waiters.delete(onEvent);
        deadline.removeEventListener("abort", onAbort);
        complete();
      };
      waiters.add(onEvent);
      deadline.addEventListener("abort", onAbort, { once: true });
    });
  }

  return { waitFor, close: () => controller.abort() };
}

function bootEvent(event, paneId) {
  return event?.event_type === "pane-boot-stage" && event.data?.pane_id === paneId;
}

const health = await request("/api/health");
const providers = await request("/api/agents/providers");
const ptyProviders = providers.filter((provider) => provider.transport === "pty" && (!requested.size || requested.has(provider.id)));
const nonPtyProviders = providers.filter((provider) => provider.transport !== "pty");
const workspaces = await request("/api/workspaces");
const workspace = workspaces.find((candidate) => candidate.is_active && !candidate.protected)
  ?? workspaces.find((candidate) => !candidate.protected);
if (!workspace) throw new Error("No non-protected dev workspace available");

console.log(`dev ${health.build_sha} workspace=${workspace.id}`);
console.log(`registered: ${providers.map((provider) => `${provider.id}:${provider.transport}`).join(", ")}`);

const results = [];
for (const provider of ptyProviders) {
  const stream = await eventStream();
  const started = performance.now();
  let spawned;
  try {
    spawned = await request("/api/agents/spawn", {
      method: "POST",
      headers: jsonHeaders,
      body: JSON.stringify({
        agent: provider.id,
        workspace_id: workspace.id,
        cwd: workspace.cwd,
        title: `spawn-latency-${provider.id}`,
      }),
    });
    const acceptedMs = performance.now() - started;
    const stages = [];
    while (true) {
      const event = await stream.waitFor((candidate) => bootEvent(candidate, spawned.pane_id), 30_000);
      const stage = event.data.stage;
      stages.push({ stage, ms: performance.now() - started, reason: event.data.reason ?? null });
      if (stage === "spawn_failed") throw new Error(event.data.reason ?? "spawn failed");
      if (stage === "agent_ready") break;
    }
    const output = await request(`/api/panes/${spawned.pane_id}/output?lines=80`);
    results.push({ provider: provider.id, acceptedMs, readyMs: performance.now() - started, stages, outputBytes: output?.output?.length ?? 0 });
  } catch (error) {
    results.push({ provider: provider.id, error: String(error instanceof Error ? error.message : error) });
  } finally {
    stream.close();
    if (spawned?.tab_id) {
      await request(`/api/workspaces/${workspace.id}/tabs/${spawned.tab_id}/close`, { method: "POST" }).catch(() => {});
    }
  }
}

console.log(JSON.stringify({ results, registeredNonPty: nonPtyProviders.map((provider) => provider.id) }, null, 2));
if (results.some((result) => result.error)) process.exitCode = 1;
