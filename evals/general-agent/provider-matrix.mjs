import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { homedir } from "node:os";
import { resolve } from "node:path";

const API = "http://127.0.0.1:7686";
const DEADLINE_MS = 120_000;
const FULL_PROVIDERS = new Set(["claude", "codex", "opencode"]);
const LAUNCH_ONLY_PROVIDERS = new Set(["cursor"]);
const PROVIDER = process.env.GA_PROVIDER;

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
    if (!response.ok || !response.body) {
      throw new Error(`SSE unavailable: ${response.status}`);
    }
    this.read(response.body).catch((error) => {
      if (error.name !== "AbortError") this.rejectAll(error);
    });
  }

  async read(body) {
    const reader = body.pipeThrough(new TextDecoderStream()).getReader();
    let buffer = "";
    for (;;) {
      const { done, value } = await reader.read();
      if (done) {
        this.rejectAll(new Error("SSE stream ended"));
        return;
      }
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
    if (process.env.GA_MATRIX_DEBUG === "1") {
      console.error(`EVENT ${event.event_type} ${JSON.stringify(event.data ?? {}).slice(0, 180)}`);
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

function tabsOf(snapshot) {
  return Object.values(snapshot.tabs ?? {});
}

async function muxState() {
  return api("/api/mux/state");
}

function generalTab(snapshot) {
  return tabsOf(snapshot).find((tab) => tab.protected === true);
}

function eventTouchesTab(event, tabId) {
  return event.data?.tab_id === tabId ||
    event.event_type.includes(tabId) ||
    JSON.stringify(event.data ?? {}).includes(tabId) ||
    event.event_type === "mux-state-changed";
}

function deliveryState(tab) {
  return tab?.pending_delivery?.state;
}

function messageText(content) {
  if (typeof content === "string") return content;
  if (Array.isArray(content)) return content.map(messageText).join("\n");
  if (content && typeof content === "object") {
    return messageText(content.text ?? content.content ?? "");
  }
  return "";
}

function assistantMessages(snapshot) {
  const messages = snapshot.messages ?? snapshot;
  return messages
    .filter((message) => message.type === "assistant" || message.role === "assistant")
    .map((message) => messageText(message.content))
    .filter((text) => text.trim());
}

async function chatSnapshot(tab) {
  const query = new URLSearchParams({
    provider: tab.agent_type,
    cwd: tab.cwd,
    session_id: tab.agent_session_id,
  });
  return api(`/api/sessions/transcript?${query}`);
}

async function waitForTab(events, predicate, description, tabId) {
  for (;;) {
    const tab = generalTab(await muxState());
    if (tab && predicate(tab)) return tab;
    await events.next((event) => !tabId || eventTouchesTab(event, tabId));
  }
}

async function waitForNewAssistant(events, tab, baselineCount, assertion = () => true) {
  for (;;) {
    const messages = assistantMessages(await chatSnapshot(tab));
    const fresh = messages.slice(baselineCount);
    const match = fresh.find(assertion);
    if (match) return match;
    await events.next((event) => eventTouchesTab(event, tab.id));
  }
}

async function assertCleanFirstRun() {
  const config = await api("/api/config");
  if (config.general_agent?.first_run_complete !== false) {
    throw new Error(
      "GA matrix requires a clean dev profile: general_agent.first_run_complete must be false",
    );
  }
  const snapshot = await muxState();
  const tab = generalTab(snapshot);
  if (!tab || tab.tab_type !== "Choice" || tab.agent_type) {
    throw new Error(
      `GA matrix requires the untouched Get started tab; observed ${JSON.stringify(tab ?? null)}`,
    );
  }
}

async function assertProviderAvailable(provider) {
  const providers = await api("/api/agents/providers");
  const descriptor = providers.find(({ id }) => id === provider);
  if (!descriptor) throw new Error(`provider unavailable: ${provider}`);
  if (descriptor.transport && descriptor.transport !== "pty") {
    throw new Error(`provider ${provider} is not a PTY provider`);
  }
  if (descriptor.installed === false || descriptor.available === false) {
    throw new Error(`provider ${provider} is not installed/available`);
  }
}

async function launch(events, provider) {
  await api("/api/agents/general/complete-first-run", {
    method: "POST",
    body: JSON.stringify({ provider_id: provider }),
  });
  const tab = await waitForTab(
    events,
    (candidate) =>
      candidate.agent_type === provider &&
      candidate.boot_stage === "agent_ready" &&
      Boolean(candidate.agent_session_id) &&
      deliveryState(candidate) === "confirmed",
    `${provider} launch`,
  );
  await api(`/api/tabs/${tab.id}/chat-raw/start`, {
    method: "POST",
    body: JSON.stringify({ subscriptionId: `ga-matrix-${process.pid}` }),
  });
  return tab;
}

async function verifyBootstrapReply(events, tab) {
  const reply = await waitForNewAssistant(events, tab, 0);
  console.log(`PASS ${PROVIDER} bootstrap reply: ${JSON.stringify(reply.slice(0, 120))}`);
}

async function restartAndVerifyResume(events, before) {
  const baseline = assistantMessages(await chatSnapshot(before)).length;
  const result = await api(`/api/agents/general/${before.id}/restart`, {
    method: "POST",
    body: "{}",
  });
  if (result.restarted !== true) {
    throw new Error(`GA restart was not claimed: ${JSON.stringify(result)}`);
  }

  const after = await waitForTab(
    events,
    (candidate) =>
      candidate.id === before.id &&
      candidate.agent_type === PROVIDER &&
      candidate.boot_stage === "agent_ready" &&
      candidate.agent_session_id === before.agent_session_id &&
      Boolean(candidate.pty_id) &&
      candidate.pty_id !== before.pty_id,
    `${PROVIDER} resume`,
    before.id,
  );

  const marker = `GA_RESUME_${PROVIDER.toUpperCase()}_${process.pid}`;
  await api(`/api/tabs/${after.id}/send`, {
    method: "POST",
    body: JSON.stringify({
      text: `Reply with exactly ${marker} and no other text.`,
    }),
  });
  await waitForNewAssistant(
    events,
    after,
    baseline,
    (text) => text.trim() === marker,
  );
  console.log(
    `PASS ${PROVIDER} restart: tab/session preserved, PTY replaced, exact reply received`,
  );
}

async function main() {
  if (!FULL_PROVIDERS.has(PROVIDER) && !LAUNCH_ONLY_PROVIDERS.has(PROVIDER)) {
    throw new Error(
      "set GA_PROVIDER to exactly one of: claude, codex, opencode, cursor",
    );
  }
  const health = await api("/api/health");
  const head = execFileSync("git", ["rev-parse", "HEAD"], { encoding: "utf8" }).trim();
  const build = health.build_sha ?? health.build;
  if (build !== head) {
    throw new Error(`dev build ${build} does not match HEAD ${head}`);
  }
  await assertCleanFirstRun();
  await assertProviderAvailable(PROVIDER);

  const events = new EventStream();
  await events.start();
  try {
    const tab = await launch(events, PROVIDER);
    console.log(
      `PASS ${PROVIDER} launch: tab=${tab.id} session=${tab.agent_session_id} delivery=confirmed`,
    );
    if (LAUNCH_ONLY_PROVIDERS.has(PROVIDER)) {
      console.log(`PASS ${PROVIDER} launch-only matrix (reply intentionally not required)`);
      return;
    }
    await verifyBootstrapReply(events, tab);
    await restartAndVerifyResume(events, tab);
  } finally {
    events.close();
  }
}

main().catch((error) => {
  console.error(`FAIL GA provider matrix (${PROVIDER ?? "unset"}): ${error.message}`);
  process.exitCode = 1;
});
