#!/usr/bin/env node

import { readFileSync } from "node:fs";

const [mode, ...values] = process.argv.slice(2);
const api = `http://127.0.0.1:${process.env.CODETERM_API_PORT}`;
const headers = { Authorization: `Bearer ${process.env.CODETERM_API_TOKEN}` };
const controller = new AbortController();
const deadline = AbortSignal.timeout(45_000);
deadline.addEventListener("abort", () => controller.abort(deadline.reason), { once: true });
const observedEventTypes = new Set();
let lastMatchingTab = null;
let lastLifecycleTitles = [];
const matchingTabStates = [];
let busyMuxSnapshots = 0;

async function json(path) {
  const response = await fetch(`${api}${path}`, { headers, signal: controller.signal });
  if (!response.ok) throw new Error(`${path}: ${response.status}`);
  return response.json();
}

// A 503 is the bounded-snapshot back-pressure answer (docs/API-EVENTS.md): no
// observation from this event; the next matching stream event re-evaluates.
async function muxSnapshot() {
  const response = await fetch(`${api}/api/mux/state`, { headers, signal: controller.signal });
  if (response.status === 503) {
    busyMuxSnapshots += 1;
    return null;
  }
  if (!response.ok) throw new Error(`/api/mux/state: ${response.status}`);
  return response.json();
}

async function observedState() {
  const mux = await muxSnapshot();
  if (!mux) return null;
  const tabEnvelope = await json("/api/tabs?all_workspaces=true");
  const tabs = Array.isArray(tabEnvelope) ? tabEnvelope : (tabEnvelope.tabs || []);
  lastLifecycleTitles = tabs
    .map((candidate) => candidate.title)
    .filter((title) => typeof title === "string" && title.startsWith("LR-"));
  if (mode === "live" || mode === "absent") {
    const [title] = values;
    const tab = tabs.find((candidate) => candidate.title === title);
    lastMatchingTab = tab
      ? {
          id: tab.id,
          agent_liveness: tab.agent_liveness,
          agent_type: tab.agent_type ?? null,
          boot_stage: tab.boot_stage ?? null,
          role_profile: tab.role_profile ?? null,
          team_id: tab.team_id ?? null,
        }
      : null;
    if (lastMatchingTab) matchingTabStates.push(lastMatchingTab);
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
  if (mode === "absent-shell") {
    const [title] = values;
    const tab = tabs.find((candidate) => candidate.title === title);
    lastMatchingTab = tab
      ? { id: tab.id, agent_liveness: tab.agent_liveness, agent_type: tab.agent_type ?? null, boot_stage: tab.boot_stage ?? null }
      : null;
    if (lastMatchingTab) matchingTabStates.push(lastMatchingTab);
    if (!tab || tab.agent_liveness !== "absent" || tab.agent_type != null || tab.boot_stage !== "shell_ready") {
      return null;
    }
    return { tab_id: tab.id, agent_liveness: tab.agent_liveness };
  }
  if (mode === "in-tab-ready") {
    const [tabId, sessionId, journalPath] = values;
    const tab = tabs.find((candidate) => candidate.id === tabId);
    lastMatchingTab = tab
      ? {
          id: tab.id,
          agent_liveness: tab.agent_liveness,
          boot_stage: tab.boot_stage ?? null,
          agent_session_id: tab.agent_session_id ?? null,
        }
      : null;
    if (lastMatchingTab) matchingTabStates.push(lastMatchingTab);
    if (
      !tab
      || tab.agent_liveness !== "live"
      || tab.boot_stage !== "agent_ready"
      || tab.agent_session_id !== sessionId
    ) return null;
    const transitions = readFileSync(journalPath, "utf8")
      .split(/\r?\n/)
      .filter(Boolean)
      .map((line) => JSON.parse(line))
      .filter((record) => record.record === "transition" && record.entry?.key?.tab_id === tabId)
      .map((record) => record.entry);
    const operation = (entry) => `${entry.key.epoch}/${entry.key.operation_id}`;
    const boundOperations = new Set(transitions
      .filter((entry) => entry.transition.transition === "session_bound"
        && entry.transition.session_id === sessionId)
      .map(operation));
    const sealed = transitions.find((entry) => entry.transition.transition === "launch_ready"
      && boundOperations.has(operation(entry)));
    if (!sealed) return null;
    return { tab_id: tabId, epoch: sealed.key.epoch, operation_id: sealed.key.operation_id };
  }
  if (mode === "rebound") {
    const [tabId, previousSessionId] = values;
    const tab = tabs.find((candidate) => candidate.id === tabId);
    lastMatchingTab = tab
      ? {
          id: tab.id,
          agent_liveness: tab.agent_liveness,
          boot_stage: tab.boot_stage ?? null,
          agent_session_id: tab.agent_session_id ?? null,
        }
      : null;
    if (lastMatchingTab) matchingTabStates.push(lastMatchingTab);
    if (
      !tab
      || tab.agent_liveness !== "live"
      || tab.boot_stage !== "agent_ready"
      || !tab.agent_session_id
      || tab.agent_session_id === previousSessionId
    ) return null;
    return { tab_id: tabId, agent_session_id: tab.agent_session_id };
  }
  if (mode === "received") {
    const [tabId, marker] = values;
    const screen = JSON.stringify(await json(`/api/tabs/${tabId}/text`));
    if (!screen.includes(`stub-received ${marker}`)) return null;
    return { tab_id: tabId, marker };
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
  if (mode === "team-owner") {
    const [teamId] = values;
    const matchingTeams = (mux.teams || []).filter((team) => team.id === teamId);
    const liveOwners = tabs.filter((candidate) => (
      candidate.team_id === teamId
      && candidate.role_profile === "orchestrator"
      && candidate.agent_liveness === "live"
    ));
    if (matchingTeams.length !== 1 || liveOwners.length !== 1) return null;
    return { team_id: teamId, owner_tab_id: liveOwners[0].id };
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
        observedEventTypes.add(event.event_type);
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
  const evidence = JSON.stringify({
    values,
    event_types: [...observedEventTypes],
    lifecycle_titles: lastLifecycleTitles,
    last_matching_tab: lastMatchingTab,
    matching_tab_states: matchingTabStates,
    busy_mux_snapshots: busyMuxSnapshots,
  });
  process.stderr.write(`FAIL wait-state ${mode}: ${error.message}; evidence=${evidence}\n`);
  process.exitCode = 1;
});
