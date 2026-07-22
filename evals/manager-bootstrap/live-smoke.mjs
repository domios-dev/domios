import { mkdirSync, readFileSync } from "node:fs";
import { homedir } from "node:os";
import { resolve } from "node:path";

const API = "http://127.0.0.1:7686";
const DEADLINE_MS = 90_000;
const STARTER = readFileSync(
  resolve("src-tauri/prompts/team_bootstrap_starter.md"),
  "utf8",
).trim();

function token() {
  const config = readFileSync(resolve(homedir(), ".codeterm-dev/config.toml"), "utf8");
  const match = config.match(/^token\s*=\s*"([^"]+)"/m);
  if (!match) throw new Error("dev API token is missing");
  return match[1];
}

const headers = {
  Authorization: `Bearer ${token()}`,
  "Content-Type": "application/json",
};

async function api(path, init = {}) {
  const response = await fetch(`${API}${path}`, {
    ...init,
    headers: { ...headers, ...init.headers },
    signal: AbortSignal.timeout(DEADLINE_MS),
  });
  const text = await response.text();
  if (!response.ok) throw new Error(`${init.method ?? "GET"} ${path}: ${response.status} ${text}`);
  return text ? JSON.parse(text) : null;
}

class EventStream {
  constructor() {
    this.events = [];
    this.waiters = [];
    this.controller = new AbortController();
  }

  async start() {
    const response = await fetch(`${API}/api/events/stream`, {
      headers,
      signal: this.controller.signal,
    });
    if (!response.ok || !response.body) throw new Error(`SSE unavailable: ${response.status}`);
    this.read(response.body).catch((error) => {
      if (error.name !== "AbortError") this.rejectAll(error);
    });
  }

  async read(body) {
    const reader = body.pipeThrough(new TextDecoderStream()).getReader();
    let buffer = "";
    for (;;) {
      const { done, value } = await reader.read();
      if (done) return;
      buffer += value;
      let boundary;
      while ((boundary = buffer.indexOf("\n\n")) >= 0) {
        const frame = buffer.slice(0, boundary).replaceAll("\r", "");
        buffer = buffer.slice(boundary + 2);
        const data = frame
          .split("\n")
          .filter((line) => line.startsWith("data:"))
          .map((line) => line.slice(5).trimStart())
          .join("\n");
        if (data) this.push(JSON.parse(data));
      }
    }
  }

  push(event) {
    const index = this.waiters.findIndex(({ predicate }) => predicate(event));
    if (index < 0) this.events.push(event);
    else this.waiters.splice(index, 1)[0].resolve(event);
  }

  next(predicate) {
    const index = this.events.findIndex(predicate);
    if (index >= 0) return Promise.resolve(this.events.splice(index, 1)[0]);
    const signal = AbortSignal.timeout(DEADLINE_MS);
    return new Promise((resolvePromise, reject) => {
      const waiter = { predicate, resolve: resolvePromise, reject };
      this.waiters.push(waiter);
      signal.addEventListener("abort", () => {
        const waiterIndex = this.waiters.indexOf(waiter);
        if (waiterIndex >= 0) this.waiters.splice(waiterIndex, 1);
        reject(new Error("event deadline exceeded"));
      }, { once: true });
    });
  }

  rejectAll(error) {
    for (const waiter of this.waiters.splice(0)) waiter.reject(error);
  }

  close() {
    this.controller.abort();
  }
}

function messageText(message) {
  const value = message?.text ?? message?.content ?? "";
  if (typeof value === "string") return value.trim();
  if (Array.isArray(value)) return value.map(messageText).join("").trim();
  return messageText(value);
}

function assistantMessages(snapshot) {
  return (snapshot.messages ?? snapshot)
    .filter((message) => message.type === "assistant")
    .map(messageText)
    .filter(Boolean);
}

function assertBootstrap(messages, role, counterpartRole, counterpartName) {
  const texts = messages.map(messageText).filter(Boolean);
  const starterCount = texts.filter((text) => text === STARTER).length;
  if (starterCount !== 1) throw new Error(`${role}: expected one starter turn, found ${starterCount}`);
  const starterIndex = texts.indexOf(STARTER);
  const acknowledgement = texts.slice(starterIndex + 1).join("\n");
  for (const expected of [role, counterpartRole, counterpartName]) {
    if (!acknowledgement.toLowerCase().includes(expected.toLowerCase())) {
      throw new Error(`${role}: acknowledgement is missing ${expected}: ${acknowledgement}`);
    }
  }
}

async function waitForBootstrap(events, paneId, role, counterpartRole, counterpartName) {
  for (;;) {
    const snapshot = await api(`/api/panes/${paneId}/chat`);
    try {
      assertBootstrap(snapshot.messages ?? snapshot, role, counterpartRole, counterpartName);
      return;
    } catch (error) {
      await events.next((event) => event.event_type === `pane-chat-${paneId}`);
      if (error.message.includes("expected one starter turn, found 2")) throw error;
    }
  }
}

async function waitForFreshExactReply(events, paneId, baseline, expected) {
  for (;;) {
    const snapshot = await api(`/api/panes/${paneId}/chat`);
    const fresh = assistantMessages(snapshot).slice(baseline);
    if (fresh.some((text) => text === expected)) return;
    await events.next((event) => event.event_type.includes(paneId));
  }
}

async function waitForSession(events, paneId) {
  for (;;) {
    const panes = await api("/api/panes");
    const pane = panes.find(({ id }) => id === paneId);
    if (pane?.agent_session_id) return;
    await events.next((event) => event.event_type === "mux-state-changed");
  }
}

async function main() {
  const health = await api("/api/health");
  const head = (await import("node:child_process")).execFileSync("git", ["rev-parse", "HEAD"], {
    encoding: "utf8",
  }).trim();
  if (health.build_sha !== head) {
    throw new Error(`dev build ${health.build_sha} does not match HEAD ${head}`);
  }

  const providers = await api("/api/providers");
  for (const provider of ["codex", "claude"]) {
    if (!providers.some(({ id }) => id === provider)) throw new Error(`provider unavailable: ${provider}`);
  }

  const events = new EventStream();
  await events.start();
  let workspaceId;
  let pair;
  const workspaceCwd = resolve("target-check/evals", `manager-bootstrap-${process.pid}`);
  try {
    mkdirSync(workspaceCwd, { recursive: true });
    const workspace = await api("/api/workspaces", {
      method: "POST",
      body: JSON.stringify({
        name: `manager-bootstrap-${process.pid}`,
        cwd: workspaceCwd,
        activate: false,
      }),
    });
    workspaceId = workspace.id;
    pair = await api("/api/orchestration/new", {
      method: "POST",
      body: JSON.stringify({
        orchestrator_agent: "codex",
        planner_agent: "claude",
        workspace_id: workspaceId,
      }),
    });

    await Promise.all([
      waitForSession(events, pair.orchestrator_pane_id),
      waitForSession(events, pair.planner_pane_id),
    ]);
    await Promise.all([
      api(`/api/panes/${pair.orchestrator_pane_id}/chat/start`, { method: "POST", body: "{}" }),
      api(`/api/panes/${pair.planner_pane_id}/chat/start`, { method: "POST", body: "{}" }),
    ]);

    await Promise.all([
      waitForBootstrap(events, pair.orchestrator_pane_id, "Orchestrator", "Planner", pair.planner_pane_id),
      waitForBootstrap(events, pair.planner_pane_id, "Planner", "Orchestrator", pair.orchestrator_pane_id),
    ]);
    const [orchestratorBaseline, plannerBaseline] = await Promise.all([
      api(`/api/panes/${pair.orchestrator_pane_id}/chat`).then((snapshot) => assistantMessages(snapshot).length),
      api(`/api/panes/${pair.planner_pane_id}/chat`).then((snapshot) => assistantMessages(snapshot).length),
    ]);
    const probe = "Reply with only your counterpart tab id.";
    await Promise.all([
      api(`/api/panes/${pair.orchestrator_pane_id}/send`, {
        method: "POST",
        body: JSON.stringify({ text: probe }),
      }),
      api(`/api/panes/${pair.planner_pane_id}/send`, {
        method: "POST",
        body: JSON.stringify({ text: probe }),
      }),
    ]);
    await Promise.all([
      waitForFreshExactReply(events, pair.orchestrator_pane_id, orchestratorBaseline, pair.planner_pane_id),
      waitForFreshExactReply(events, pair.planner_pane_id, plannerBaseline, pair.orchestrator_pane_id),
    ]);
    console.log(`PASS manager bootstrap: Codex ${pair.orchestrator_pane_id} + Claude ${pair.planner_pane_id}`);
    console.log("  one starter turn each; fresh exact counterpart replies observed");
  } finally {
    events.close();
    for (const paneId of [pair?.planner_pane_id, pair?.orchestrator_pane_id].filter(Boolean)) {
      await api(`/api/panes/${paneId}/close`, { method: "POST", body: "{}" }).catch(() => {});
    }
    if (workspaceId) {
      await api(`/api/workspaces/${workspaceId}/close`, { method: "POST", body: "{}" }).catch((error) => {
        console.error(`cleanup warning: ${error.message}`);
      });
    }
  }
}

main().catch((error) => {
  console.error(`FAIL manager bootstrap: ${error.message}`);
  process.exitCode = 1;
});
