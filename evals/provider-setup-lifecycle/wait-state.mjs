#!/usr/bin/env node

const [apiUrl, apiToken, mode, expected, providerId] = process.argv.slice(2);
if (!apiUrl || !apiToken || !mode || !expected) {
  throw new Error("dev API URL, dev API token, mode, and expected value are required");
}
const api = new URL(apiUrl).toString().replace(/\/$/, "");
const headers = { Authorization: `Bearer ${apiToken}` };
const controller = new AbortController();
const deadline = AbortSignal.timeout(45_000);
deadline.addEventListener("abort", () => controller.abort(deadline.reason), { once: true });
let selectedTab = null;
let selectedPty = null;
const observed = new Set();

async function json(route) {
  const response = await fetch(`${api}${route}`, { headers, signal: controller.signal });
  if (!response.ok) throw new Error(`${route}: ${response.status}`);
  return response.json();
}

async function observe(event) {
  const data = event.data || {};
  if (mode === "provider" && event.event_type === "contributions-changed") {
    const envelope = await json("/api/agents/providers");
    const providers = Array.isArray(envelope) ? envelope : envelope.providers || [];
    if (providers.some((provider) => provider.id === expected)) return { provider: expected };
  }
  if (mode === "provider-absent" && event.event_type === "contributions-changed") {
    const envelope = await json("/api/agents/providers");
    const providers = Array.isArray(envelope) ? envelope : envelope.providers || [];
    if (!providers.some((provider) => provider.id === expected)) return { provider: expected, absent: true };
  }
  if (event.event_type === "tab-title-pinned" && data.title === expected) selectedTab = data.tab_id;
  if (mode === "agent-ready" && selectedTab && event.event_type === "tab-boot-stage" && data.tab_id === selectedTab && data.stage === "agent_ready") {
    const envelope = await json("/api/tabs?all_workspaces=true");
    const tabs = Array.isArray(envelope) ? envelope : envelope.tabs || [];
    const tab = tabs.find((candidate) => candidate.id === selectedTab);
    if (tab?.agent_type === providerId && tab.boot_stage === "agent_ready") {
      return { tab_id: selectedTab, agent_type: tab.agent_type, boot_stage: tab.boot_stage };
    }
  }
  if (mode === "install-live" && selectedTab && event.event_type === "mux-state-changed") {
    const envelope = await json("/api/tabs?all_workspaces=true");
    const tabs = Array.isArray(envelope) ? envelope : envelope.tabs || [];
    const tab = tabs.find((candidate) => candidate.id === selectedTab);
    if (tab?.pty_id) return { tab_id: selectedTab, pty_id: tab.pty_id };
  }
  if (mode === "install-rollback" && selectedTab && event.event_type === "tab-closed" && data.tab_id === selectedTab) {
    const envelope = await json("/api/tabs?all_workspaces=true");
    const tabs = Array.isArray(envelope) ? envelope : envelope.tabs || [];
    if (!tabs.some((candidate) => candidate.id === selectedTab) && selectedPty) {
      return { tab_id: selectedTab, pty_id: selectedPty, absent: true };
    }
  }
  if (mode === "install-rollback" && event.event_type === "provider-install-fixture-dispatch-refused" && data.tab_id === selectedTab) selectedPty = data.pty_id;
  return null;
}

async function run() {
  const response = await fetch(`${api}/api/events/stream`, { headers, signal: controller.signal });
  if (!response.ok || !response.body) throw new Error(`event stream: ${response.status}`);
  process.stdout.write("READY\n");
  const reader = response.body.pipeThrough(new TextDecoderStream()).getReader();
  let buffer = "";
  for (;;) {
    const chunk = await reader.read();
    if (chunk.done) throw new Error("event stream ended before observation");
    buffer += chunk.value;
    let boundary = buffer.indexOf("\n\n");
    while (boundary >= 0) {
      const frame = buffer.slice(0, boundary).replaceAll("\r", "");
      buffer = buffer.slice(boundary + 2);
      const payload = frame.split("\n").filter((line) => line.startsWith("data:")).map((line) => line.slice(5).trimStart()).join("\n");
      if (payload) {
        const event = JSON.parse(payload);
        observed.add(event.event_type);
        const result = await observe(event);
        if (result) {
          process.stdout.write(`${JSON.stringify(result)}\n`);
          controller.abort();
          return;
        }
      }
      boundary = buffer.indexOf("\n\n");
    }
  }
}

run().catch((error) => {
  process.stderr.write(`FAIL ${mode}: ${error.message}; selected_tab=${selectedTab || ""}; selected_pty=${selectedPty || ""}; events=${[...observed].join(",")}\n`);
  process.exitCode = 1;
});
