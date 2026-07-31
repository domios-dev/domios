import { execFileSync } from "node:child_process";
import { mkdirSync, readFileSync, rmSync } from "node:fs";
import { homedir } from "node:os";
import { join, resolve } from "node:path";

const API = "http://127.0.0.1:7686";
const DEADLINE_MS = 120_000;
const ALL_PROVIDERS = ["claude", "codex", "opencode", "cursor"];
const PROVIDERS = process.env.MATRIX_PROVIDERS?.split(",").filter(Boolean) ?? ALL_PROVIDERS;
const ALL_PAIRS = [
  ["codex", "claude"],
  ["opencode", "cursor"],
];
const REQUESTED_PAIRS = process.env.MATRIX_PAIRS?.split(",").filter(Boolean);
const PAIRS = process.env.MATRIX_SKIP_TEAMS === "1"
  ? []
  : REQUESTED_PAIRS?.map((pair) => pair.split("+")) ?? ALL_PAIRS;
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
  const body = await response.text();
  if (!response.ok) {
    throw new Error(`${init.method ?? "GET"} ${path}: ${response.status} ${body}`);
  }
  return body ? JSON.parse(body) : null;
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
      buffer += value.replaceAll("\r", "");
      let boundary;
      while ((boundary = buffer.indexOf("\n\n")) >= 0) {
        const frame = buffer.slice(0, boundary);
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
    if (process.env.MATRIX_DEBUG === "1") {
      console.error(`EVENT ${event.event_type} ${JSON.stringify(event.data ?? {}).slice(0, 160)}`);
    }
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

function messagesOf(snapshot) {
  return snapshot.messages ?? snapshot;
}

function assistantText(snapshot) {
  return messagesOf(snapshot)
    .filter((message) => message.type === "assistant")
    .map((message) => message.content ?? "")
    .join("\n");
}

function tabChanged(event, tabId) {
  return event.event_type.includes(tabId) ||
    event.data?.tab_id === tabId ||
    JSON.stringify(event.data ?? {}).includes(tabId) ||
    event.event_type === "mux-state-changed";
}

async function waitForSession(events, tabId) {
  for (;;) {
    const tab = (await api("/api/tabs")).find(({ id }) => id === tabId);
    if (tab?.agent_session_id) return;
    await events.next((event) => event.event_type === "mux-state-changed");
  }
}

async function waitForAssistant(events, tabId, assertion) {
  for (;;) {
    const snapshot = await api(`/api/tabs/${tabId}/chat`);
    const text = assistantText(snapshot);
    if (assertion(text)) return text;
    await events.next((event) => tabChanged(event, tabId));
  }
}

async function closeTab(tabId) {
  await api(`/api/tabs/${tabId}/close`, { method: "POST", body: "{}" }).catch(() => {});
}

async function createWorkspace(label) {
  const cwd = resolve("target-check", "evals", `${label}-${process.pid}`);
  mkdirSync(cwd, { recursive: true });
  const workspace = await api("/api/workspaces", {
    method: "POST",
    body: JSON.stringify({ name: `${label}-${process.pid}`, cwd, activate: false }),
  });
  return { id: workspace.id, cwd };
}

async function closeWorkspace(workspaceId) {
  await api(`/api/workspaces/${workspaceId}/close`, { method: "POST", body: "{}" }).catch(() => {});
}

function removeWorkspace(cwd) {
  try {
    rmSync(cwd, { recursive: true, force: true });
  } catch {}
}

async function verifySingle(events, provider) {
  console.log(`RUN single ${provider}`);
  const workspace = await createWorkspace(`provider-single-${provider}`);
  let tabId;
  try {
    const expected = "UTC";
    const spawned = await api("/api/agents/spawn", {
      method: "POST",
      body: JSON.stringify({
        agent: provider,
        workspace_id: workspace.id,
        cwd: workspace.cwd,
        title: `provider-matrix-${provider}`,
        skip_permissions: true,
        activate: false,
        task: "What is the standard three-letter abbreviation for Coordinated Universal Time? Reply with only that abbreviation.",
      }),
    });
    tabId = spawned.tab_id;
    await waitForSession(events, tabId);
    await api(`/api/tabs/${tabId}/chat-raw/start`, { method: "POST", body: "{}" });
    await waitForAssistant(events, tabId, (text) => text.trim() === expected);
    console.log(`PASS single ${provider}: exact assistant-only oracle returned`);
  } finally {
    if (tabId) await closeTab(tabId);
    await closeWorkspace(workspace.id);
    removeWorkspace(workspace.cwd);
  }
}

function assertBootstrap(snapshot, role, counterpartRole, counterpartId) {
  const messages = messagesOf(snapshot);
  const starters = messages.filter(
    (message) => message.type === "user" && (message.content ?? "").includes(STARTER),
  );
  if (starters.length > 1) throw new Error(`${role}: expected one starter turn, found ${starters.length}`);
  if (starters.length === 0) return false;
  const assistant = assistantText(snapshot).toLowerCase();
  return [role, counterpartRole, counterpartId].every((value) =>
    assistant.includes(value.toLowerCase()),
  );
}

async function waitForBootstrap(events, tabId, role, counterpartRole, counterpartId) {
  let snapshot;
  try {
    for (;;) {
      snapshot = await api(`/api/tabs/${tabId}/chat`);
      if (assertBootstrap(snapshot, role, counterpartRole, counterpartId)) return;
      await events.next((event) => tabChanged(event, tabId));
    }
  } catch (error) {
    snapshot ??= await api(`/api/tabs/${tabId}/chat`).catch(() => ({ messages: [] }));
    const messages = messagesOf(snapshot);
    const starters = messages.filter(
      (message) => message.type === "user" && (message.content ?? "").includes(STARTER),
    );
    const assistant = assistantText(snapshot).replaceAll(/\s+/g, " ").slice(0, 240);
    throw new Error(
      `${role} tab ${tabId}: ${error.message}; starter_turns=${starters.length}; assistant=${JSON.stringify(assistant)}`,
    );
  }
}

async function verifyTeam(events, orchestratorProvider, plannerProvider) {
  console.log(`RUN team ${orchestratorProvider}+${plannerProvider}`);
  const workspace = await createWorkspace(`provider-team-${orchestratorProvider}-${plannerProvider}`);
  let pair;
  try {
    pair = await api("/api/orchestration/new", {
      method: "POST",
      body: JSON.stringify({
        orchestrator_agent: orchestratorProvider,
        planner_agent: plannerProvider,
        workspace_id: workspace.id,
      }),
    });
    await Promise.all([
      waitForSession(events, pair.orchestrator_tab_id),
      waitForSession(events, pair.planner_tab_id),
    ]);
    await Promise.all([
      api(`/api/tabs/${pair.orchestrator_tab_id}/chat-raw/start`, { method: "POST", body: "{}" }),
      api(`/api/tabs/${pair.planner_tab_id}/chat-raw/start`, { method: "POST", body: "{}" }),
    ]);
    await Promise.all([
      waitForBootstrap(
        events,
        pair.orchestrator_tab_id,
        "Orchestrator",
        "Planner",
        pair.planner_tab_id,
      ),
      waitForBootstrap(
        events,
        pair.planner_tab_id,
        "Planner",
        "Orchestrator",
        pair.orchestrator_tab_id,
      ),
    ]);
    console.log(`PASS team ${orchestratorProvider}+${plannerProvider}: one starter and both roles acknowledged`);
  } finally {
    if (pair?.planner_tab_id) await closeTab(pair.planner_tab_id);
    if (pair?.orchestrator_tab_id) await closeTab(pair.orchestrator_tab_id);
    await closeWorkspace(workspace.id);
    removeWorkspace(workspace.cwd);
  }
}

async function main() {
  const health = await api("/api/health");
  const head = execFileSync("git", ["rev-parse", "HEAD"], { encoding: "utf8" }).trim();
  if (health.build_sha !== head) {
    throw new Error(`dev build ${health.build_sha} does not match HEAD ${head}`);
  }
  const available = new Set((await api("/api/providers")).map(({ id }) => id));
  for (const provider of ALL_PROVIDERS) {
    if (!available.has(provider)) throw new Error(`provider unavailable: ${provider}`);
  }

  const events = new EventStream();
  await events.start();
  try {
    for (const provider of PROVIDERS) await verifySingle(events, provider);
    for (const [orchestrator, planner] of PAIRS) {
      await verifyTeam(events, orchestrator, planner);
    }
  } finally {
    events.close();
  }
  console.log(`PASS provider matrix: ${PROVIDERS.length} singles and ${PAIRS.length * 2} team roles`);
}

main().catch((error) => {
  console.error(`FAIL provider matrix: ${error.message}`);
  process.exitCode = 1;
});
