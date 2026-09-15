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
const busySnapshots = {};
const receiptEvents = [];
// Output-side events for the watched tab follow the stub's receipt into the screen buffer.
const TAB_OUTPUT_EVENTS = new Set(["agent.task_complete", "tab-status-changed", "chat-stream-preview"]);

// A journal line still being appended does not parse yet; the next event re-reads it.
function journalRecords(path) {
  let body;
  try {
    body = readFileSync(path, "utf8");
  } catch {
    return [];
  }
  return body.split(/\r?\n/).filter(Boolean).flatMap((line) => {
    try {
      return [JSON.parse(line)];
    } catch {
      return [];
    }
  });
}
const journalSeq = (record) => (record.record === "checkpoint" ? record.checkpoint.committed_through : record.entry.seq);
const journalTab = (record) => (record.record === "checkpoint" ? record.checkpoint.key.tab_id : record.entry.key.tab_id);
// Captured before READY, so only records the observed action appended can prove it.
const journalBaseline = mode === "resumed"
  ? journalRecords(values[2]).reduce((max, record) => Math.max(max, journalSeq(record)), 0)
  : 0;

function shouldEvaluate(event) {
  if (["mux-state-changed", "plan-changed", "overflow"].includes(event.event_type)) return true;
  if (mode !== "received" || !TAB_OUTPUT_EVENTS.has(event.event_type)) return false;
  if (event.data?.tab_id !== values[0]) return false;
  receiptEvents.push(event.event_type);
  return true;
}

// Every snapshot the waiter reads goes through here. A 503 is the snapshot
// back-pressure answer: no observation from this event, the next frame re-evaluates.
async function json(path) {
  const response = await fetch(`${api}${path}`, { headers, signal: controller.signal });
  if (response.status === 503) {
    busySnapshots[path] = (busySnapshots[path] || 0) + 1;
    return null;
  }
  if (!response.ok) throw new Error(`${path}: ${response.status}`);
  return response.json();
}

async function observedState() {
  const mux = await json("/api/mux/state");
  if (!mux) return null;
  const tabEnvelope = await json("/api/tabs");
  if (!tabEnvelope) return null;
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
  if (mode === "resumed") {
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
    const appended = journalRecords(journalPath)
      .filter((record) => journalSeq(record) > journalBaseline && journalTab(record) === tabId);
    const checkpoint = appended.find((record) => record.record === "checkpoint"
      && record.checkpoint.authority?.session_id === sessionId);
    if (checkpoint) {
      return { tab_id: tabId, epoch: checkpoint.checkpoint.key.epoch, operation_id: checkpoint.checkpoint.key.operation_id };
    }
    const transitions = appended.filter((record) => record.record === "transition").map((record) => record.entry);
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
  if (mode === "received") {
    const [tabId, marker] = values;
    const text = await json(`/api/tabs/${tabId}/text`);
    if (!text || !JSON.stringify(text).includes(`stub-received ${marker}`)) return null;
    return { tab_id: tabId, marker };
  }
  if (mode === "recovered") {
    const [teamId, planId] = values;
    const matchingTeams = (mux.teams || []).filter((team) => team.id === teamId);
    const envelope = await json(`/api/plans/${planId}`);
    if (!envelope) return null;
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
        if (shouldEvaluate(event)) {
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
    busy_snapshots: busySnapshots,
    receipt_events: receiptEvents,
  });
  process.stderr.write(`FAIL wait-state ${mode}: ${error.message}; evidence=${evidence}\n`);
  process.exitCode = 1;
});
