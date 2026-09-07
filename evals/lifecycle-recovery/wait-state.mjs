#!/usr/bin/env node

const [mode, ...values] = process.argv.slice(2);
const api = `http://127.0.0.1:${process.env.CODETERM_API_PORT}`;
const headers = { Authorization: `Bearer ${process.env.CODETERM_API_TOKEN}` };
const controller = new AbortController();
const deadline = AbortSignal.timeout(45_000);
deadline.addEventListener("abort", () => controller.abort(deadline.reason), { once: true });

async function json(path) {
  const response = await fetch(`${api}${path}`, { headers, signal: controller.signal });
  if (!response.ok) throw new Error(`${path}: ${response.status}`);
  return response.json();
}

async function observedState() {
  const mux = await json("/api/mux/state");
  const tabs = Object.values(mux.tabs || {});
  if (mode === "live" || mode === "absent") {
    const [title] = values;
    const tab = tabs.find((candidate) => candidate.tab_title === title);
    if (!tab || tab.agent_liveness !== mode) return null;
    if (
      mode === "absent"
      && (tab.agent_type != null
        || tab.boot_stage !== "shell_ready"
        || tab.role_profile !== "orchestrator"
        || !tab.team_id)
    ) return null;
    return {
      tab_id: tab.id,
      agent_liveness: tab.agent_liveness,
      team_id: tab.team_id ?? null,
    };
  }
  if (mode === "recovered") {
    const [teamId, planId] = values;
    const matchingTeams = (mux.teams || []).filter((team) => team.id === teamId);
    const envelope = await json(`/api/plans/${planId}`);
    const plan = envelope.plan || envelope;
    if (plan.id !== planId || plan.team_id !== teamId || plan.status !== "active") return null;
    const liveOwners = tabs.filter((candidate) => (
      candidate.team_id === teamId
      && candidate.role_profile === "orchestrator"
      && candidate.agent_liveness === "live"
    ));
    const owner = liveOwners.find((candidate) => candidate.id === plan.owner?.tab_id);
    if (matchingTeams.length !== 1 || liveOwners.length !== 1 || !owner) return null;
    return { team_id: teamId, plan_id: planId, owner_tab_id: owner.id };
  }
  throw new Error(`unknown wait mode: ${mode}`);
}

async function run() {
  const response = await fetch(`${api}/api/events/stream`, {
    headers,
    signal: controller.signal,
  });
  if (!response.ok || !response.body) throw new Error(`event stream: ${response.status}`);
  process.stdout.write("READY\n");
  const reader = response.body.pipeThrough(new TextDecoderStream()).getReader();
  let buffer = "";
  for (;;) {
    const { done, value } = await reader.read();
    if (done) throw new Error("event stream ended before state observation");
    buffer += value;
    let boundary = buffer.indexOf("\n\n");
    while (boundary >= 0) {
      const frame = buffer.slice(0, boundary).replaceAll("\r", "");
      buffer = buffer.slice(boundary + 2);
      const payload = frame
        .split("\n")
        .filter((line) => line.startsWith("data:"))
        .map((line) => line.slice(5).trimStart())
        .join("\n");
      if (payload) {
        const event = JSON.parse(payload);
        if (event.event_type === "mux-state-changed" || event.event_type === "plan-changed") {
          const state = await observedState();
          if (state) {
            process.stdout.write(`${JSON.stringify(state)}\n`);
            controller.abort();
            return;
          }
        }
      }
      boundary = buffer.indexOf("\n\n");
    }
  }
}

run().catch((error) => {
  process.stderr.write(`FAIL wait-state ${mode}: ${error.message}\n`);
  process.exitCode = 1;
});
