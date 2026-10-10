import { spawn } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, renameSync, writeFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { exactReply, firstTurnPolicy, kind, probeReady } from "./oracle.mjs";
import { digest, logLines, newEvidence } from "./logs.mjs";
import { recordBoundary, nextTurnModel } from "./records.mjs";
import { snapshotAfterEvent, tabSnapshot } from "./snapshots.mjs";
import { assertTarget } from "./target.mjs";
import { restartObserved } from "./restart.mjs";
import { restoreReady, reconcileReadiness } from "./readiness.mjs";

const script = fileURLToPath(import.meta.url);
const root = resolve(dirname(script), "../..");
const [mode, output, roundArgument] = process.argv.slice(2);
const round = Number(roundArgument ?? 0);
const out = resolve(output);
const port = Number(process.env.CODETERM_API_PORT);
const instance = process.env.CODETERM_INSTANCE || "";
const API = `http://127.0.0.1:${port}`;
const deadlineMs = 120_000;
const dataDir = process.env.CODETERM_DEV_DATA_DIR;
assertTarget(instance, port);
if (!dataDir) {
  throw new Error("Run through the selected DEV lib dev_run wrapper");
}
const headers = {
  Authorization: `Bearer ${process.env.CODETERM_API_TOKEN}`,
  "Content-Type": "application/json",
};
const resultPath = join(out, "results.json");
let result = existsSync(resultPath) ? JSON.parse(readFileSync(resultPath, "utf8")) : {
  schema: 1, started_at: new Date().toISOString(), rounds: [], fixtures: [], coverage: [],
};

function save() {
  writeFileSync(`${resultPath}.tmp`, `${JSON.stringify(result, null, 2)}\n`);
  renameSync(`${resultPath}.tmp`, resultPath);
  const cell = (value) => String(value ?? "—").replaceAll("|", "\\|").replaceAll("\n", " ");
  const lines = ["# Restart endurance", "", `Started: ${result.started_at}`, "",
    `Build: ${result.health?.build_sha ?? result.health?.build ?? "unknown"}`, "",
    `Selected instance: ${instance || "default"} on ${port}.`, "",
    "## Coverage", "", ...result.coverage.map((entry) => `- ${cell(entry.provider)}: ${cell(entry.status)} ${cell(entry.reason ?? "")}`), ""];
  if (result.fatal) lines.push(`Harness stopped: ${cell(result.fatal)}`, "");
  for (const entry of result.rounds) {
    lines.push(`## Round ${entry.round}`, "", "| Tab | Cohort | Provider | Boot stage | Identity | Session before → after | Exact answer | ms | Verdict / reason |",
      "| --- | --- | --- | --- | --- | --- | --- | --- | --- |");
    for (const row of entry.rows) lines.push(`| ${[
      row.id ?? "spawn refused", row.cohort, row.provider, row.before_probe?.boot_stage,
      row.before_probe?.agent_type, `${row.previous_session ?? "—"} → ${row.after_probe?.agent_session_id ?? row.before_probe?.agent_session_id ?? "—"}`,
      row.answer ?? "—", row.elapsed_ms, `${row.pass ? "PASS" : row.observed_only ? "OBSERVED" : "FAIL"}: ${row.reason ?? ""}`,
    ].map(cell).join(" | ")} |`);
    if (entry.restart) lines.push("", `Restart: ${entry.restart.pass ? "PASS" : "FAIL"}; listener ${entry.restart.before?.pid ?? "unknown"} → ${entry.restart.after?.pid ?? "unknown"}; process start (epoch microseconds) ${entry.restart.before?.started_us ?? "unknown"} → ${entry.restart.after?.started_us ?? "unknown"}.`);
    lines.push("", `Log evidence: [round-${entry.round}.log](round-${entry.round}.log). Full tab records, lifecycle and transcripts are in results.json.`, "");
    for (const row of entry.rows.filter((row) => !row.pass && !row.observed_only)) {
      lines.push(`### ${cell(row.id ?? row.provider)} failure`, "", `${cell(row.reason)}`, "", "```text");
      if (!row.log_evidence?.length) lines.push("No tab-specific restore/launch log line retained; cause unproven.");
      for (const line of row.log_evidence ?? []) lines.push(line);
      lines.push("```", "");
    }
  }
  if (result.model_switches?.length) {
    lines.push("## Model switches", "", "| Provider | Tab | Requested | Persisted | Same session | Answer | Verdict |", "| --- | --- | --- | --- | --- | --- | --- |");
    for (const row of result.model_switches) lines.push(`| ${[
      row.provider, row.id, row.requested_model, row.persisted?.model,
      row.session_before === row.session_after ? "yes" : "no", row.probe?.answer,
      row.pass ? "PASS" : `FAIL: ${row.reason ?? "Persisted next-turn model did not match"}`,
    ].map(cell).join(" | ")} |`);
  }
  writeFileSync(join(out, "results.md"), `${lines.join("\n")}\n`);
}

async function api(path, method = "GET", body, signal = AbortSignal.timeout(30_000)) {
  const response = await fetch(`${API}${path}`, {
    method, headers, body: body === undefined ? undefined : JSON.stringify(body), signal,
  });
  const text = await response.text();
  if (!response.ok) throw Object.assign(new Error(`${method} ${path}: HTTP ${response.status} ${text}`), { status: response.status });
  return text ? JSON.parse(text) : null;
}

async function command(args) {
  return new Promise((accept, reject) => {
    const child = spawn(process.env.ENDURE_BASH ?? "bash", args, {
      cwd: root, stdio: ["ignore", "pipe", "pipe"], signal: AbortSignal.timeout(deadlineMs),
    });
    let stdout = "", stderr = "";
    child.stdout.on("data", (chunk) => { stdout += chunk; });
    child.stderr.on("data", (chunk) => { stderr += chunk; });
    child.on("error", reject);
    child.on("close", (code) => code === 0 ? accept(stdout.trim()) : reject(new Error(`CLI exit ${code}: ${stdout} ${stderr}`)));
  });
}
async function dc(...args) {
  return command([join(root, "evals/restart-endurance/run.sh"), "--dc", ...args]);
}
const unwrap = (value) => value?.data ?? value;
async function dcJson(...args) { return unwrap(JSON.parse(await dc(...args, "--json"))); }

class Events {
  constructor(signal) {
    this.signal = signal;
    this.queue = [];
    this.waiter = null;
    this.failure = null;
    this.controller = new AbortController();
  }
  async open() {
    const response = await fetch(`${API}/api/events/stream`, {
      headers, signal: AbortSignal.any([this.signal, this.controller.signal]),
    });
    if (!response.ok || !response.body) throw new Error(`SSE HTTP ${response.status}`);
    this.reader = response.body.pipeThrough(new TextDecoderStream()).getReader();
    this.consume().catch((error) => {
      this.failure = error;
      this.waiter?.reject(error);
      this.waiter = null;
    });
  }
  async consume() {
    let buffer = "";
    for (;;) {
      const { done, value } = await this.reader.read();
      if (done) throw new Error("SSE closed before observation finished");
      buffer += value.replaceAll("\r", "");
      let boundary;
      while ((boundary = buffer.indexOf("\n\n")) >= 0) {
        const frame = buffer.slice(0, boundary);
        buffer = buffer.slice(boundary + 2);
        const data = frame.split("\n").filter((line) => line.startsWith("data:"))
          .map((line) => line.slice(5).trimStart()).join("\n");
        if (!data) continue;
        const event = JSON.parse(data);
        if (event.event_type === "overflow") throw new Error("SSE overflow: observation incomplete");
        if (this.waiter) {
          const waiter = this.waiter;
          this.waiter = null;
          waiter.resolve(event);
        } else {
          this.queue.push(event);
          if (this.queue.length > 10_000) throw new Error("SSE queue exceeded observation bound");
        }
      }
    }
  }
  next() {
    if (this.failure) return Promise.reject(this.failure);
    if (this.signal.aborted) return Promise.reject(new Error("Observation deadline exceeded"));
    if (this.queue.length) return Promise.resolve(this.queue.shift());
    return new Promise((accept, reject) => {
      const onAbort = () => reject(new Error("Observation deadline exceeded"));
      this.signal.addEventListener("abort", onAbort, { once: true });
      const settle = (fn, value) => {
        this.signal.removeEventListener("abort", onAbort);
        fn(value);
      };
      this.waiter = { resolve: (event) => settle(accept, event), reject: (error) => settle(reject, error) };
    });
  }
  close() { this.controller.abort(); }
}

const touches = (event, id) => event.data?.tab_id === id || event.event_type?.includes(id) || event.event_type === "mux-state-changed";
const terminalStages = new Set(["spawn_failed", "launch_deadline_elapsed", "launch_binding_unconfirmed", "awaiting_owner_prompt"]);
async function tabs(signal = AbortSignal.timeout(deadlineMs), events) {
  return tabSnapshot({ events, createEvents: () => new Events(signal),
    summaries: () => api("/api/tabs?local_only=true&all_workspaces=true", "GET", undefined, signal),
    state: () => api("/api/mux/state", "GET", undefined, signal),
  });
}
async function tab(id, signal, events) { return (await tabs(signal, events)).find((record) => record.id === id); }
function failure(record) {
  if (!record) return "Tab absent from the all-tabs projection";
  if (terminalStages.has(record.boot_stage)) return `${record.boot_stage}: ${record.boot_failure_reason ?? JSON.stringify(record.pending_prompt ?? null)}`;
  return null;
}
async function waitReady(events, id, signal, policy, requireSession = false, requireLaunch = false) {
  for (;;) {
    const record = await tab(id, signal, events);
    const reason = failure(record);
    if (reason) throw new Error(reason);
    if (probeReady(record, policy, requireSession) && (!requireLaunch || record.boot_stage === "agent_ready")) return record;
    let event;
    do { event = await events.next(); } while (!touches(event, id));
  }
}
async function observeFresh(fixture) {
  const signal = AbortSignal.timeout(deadlineMs), events = new Events(signal);
  const row = { ...fixture, pass: false };
  try {
    await events.open();
    row.before_probe = await waitReady(events, fixture.id, signal, fixture.first_turn_policy);
    row.transcript = await snapshotAfterEvent(events, () => transcript(fixture.id, signal));
    row.pass = !row.transcript.messages.some((message) => ["user", "assistant"].includes(kind(message)));
    fixture.empty_session_id = row.pass ? row.before_probe.agent_session_id : null;
    fixture.last_session = row.pass ? null : row.before_probe.agent_session_id;
    row.reason = row.pass ? "No user or assistant turn before restart" : "Fresh fixture already has turns";
  } catch (error) {
    row.reason = error.message;
    row.before_probe = await tab(fixture.id).catch(() => null);
    row.lifecycle = await dcJson("tab", "inspect", "--tab", fixture.id, "--lifecycle").catch(() => null);
    row.log_evidence = logLines(join(dataDir, "logs")).filter((line) => line.includes(fixture.id));
  } finally { events.close(); }
  return row;
}
async function transcript(id, signal) {
  return api(`/api/tabs/${id}/chat`, "GET", undefined, signal);
}
async function paddedPrompt(id, marker) {
  const pad = await dc("--as-tab", id, "pad", "path");
  const relativePath = `endure-${marker}.txt`;
  const path = join(pad, relativePath);
  writeFileSync(path, `Reply with exactly ${marker}`, "utf8");
  return { prompt: readFileSync(path, "utf8"), relativePath };
}

async function probe(fixture, n, observeOnly = false) {
  const row = { id: fixture.id, cohort: fixture.cohort, provider: fixture.provider,
    previous_session: fixture.last_session ?? null, pass: false, observed_only: observeOnly,
    started_at: new Date().toISOString() };
  if (!fixture.id) { row.reason = fixture.spawn_error; return row; }
  const start = performance.now();
  const signal = AbortSignal.timeout(deadlineMs);
  const events = new Events(signal);
  let subscribed = false;
  try {
    await events.open();
    row.initial = await tab(fixture.id, signal, events);
    if (observeOnly) {
      row.before_probe = row.initial;
      row.reason = failure(row.initial) ?? "Existing tab inspected; no message authorized outside Endure fixtures";
      return row;
    }
    row.phase = "waiting_for_input_readiness";
    const ready = await waitReady(events, fixture.id, signal, fixture.first_turn_policy, Boolean(fixture.has_answer));
    row.before_probe = ready;
    if (ready.agent_type !== fixture.provider) throw new Error(`Provider identity changed: ${fixture.provider} → ${ready.agent_type}`);
    if (fixture.has_answer && fixture.last_session !== ready.agent_session_id) throw new Error(`Session changed before probe: ${fixture.last_session} → ${ready.agent_session_id}`);
    const before = await snapshotAfterEvent(events, () => transcript(fixture.id, signal));
    const baseline = before.messages.length;
    row.transcript_baseline = baseline;
    if (ready.agent_session_id) {
      try { row.record_boundary = recordBoundary(fixture.provider, ready.cwd, ready.agent_session_id); }
      catch (error) { row.record_boundary_error = error.message; }
    }
    const marker = `ENDURE_${fixture.provider}_${n}`;
    row.expected = marker;
    const payload = await paddedPrompt(fixture.id, marker);
    row.prompt = payload.prompt;
    row.prompt_file = payload.relativePath;
    row.send_started_at = new Date().toISOString();
    row.phase = "sending";
    row.send_response = await dcJson("--as-tab", fixture.id, "send", "--tab", fixture.id,
      "--require-agent", "--from-pad", payload.relativePath);
    row.phase = "waiting_for_session_binding";
    const bound = await waitReady(events, fixture.id, signal, fixture.first_turn_policy, true);
    row.bound_session = bound.agent_session_id;
    if (bound.agent_type !== fixture.provider || (ready.agent_session_id && ready.agent_session_id !== bound.agent_session_id)) {
      throw new Error("Provider or session changed while establishing probe binding");
    }
    if (!ready.agent_session_id) {
      try {
        row.record_boundary = recordBoundary(fixture.provider, bound.cwd, bound.agent_session_id);
        row.record_boundary.bytes = 0;
        row.record_boundary.message_ids = [];
      } catch (error) { row.record_boundary_error = error.message; }
    }
    await api(`/api/tabs/${fixture.id}/chat-raw/start`, "POST", { subscriptionId: `endure-${fixture.id}-${n}` }, signal);
    subscribed = true;
    row.phase = "waiting_for_exact_reply";
    for (;;) {
      const snapshot = await snapshotAfterEvent(events, () => transcript(fixture.id, signal));
      row.transcript = snapshot;
      const answer = exactReply(snapshot.messages, baseline, row.prompt, marker, fixture.provider === "grok");
      const current = await tab(fixture.id, signal, events);
      let completedRecord;
      if (answer && row.record_boundary) {
        try { completedRecord = nextTurnModel(row.record_boundary, marker); }
        catch (error) { row.completed_record_error = error.message; }
      }
      if (answer && (current?.status === "idle" || completedRecord?.turn_completed)) {
        row.answer = answer;
        row.completion_source = completedRecord?.turn_completed ? "own_session_completed_turn" : "idle_status";
        row.completed_record = completedRecord;
        row.reply_ms = Math.round(performance.now() - start);
        row.after_probe = current;
        if (row.after_probe?.agent_session_id !== bound.agent_session_id) throw new Error("Session changed during probe; reply ownership uncertain");
        if (fixture.has_answer && fixture.last_session !== ready.agent_session_id) {
          row.reason = `Session changed across restart: ${fixture.last_session} → ${ready.agent_session_id}`;
        } else { row.pass = true; }
        fixture.has_answer = true;
        fixture.last_session = row.after_probe.agent_session_id;
        row.phase = "answered";
        break;
      }
      if (failure(current)) throw new Error(failure(current));
      let event;
      do { event = await events.next(); } while (!touches(event, fixture.id));
    }
  } catch (error) {
    row.reason = error.message;
  } finally {
    row.elapsed_ms = Math.round(performance.now() - start);
    row.finished_at = new Date().toISOString();
    events.close();
    row.after_probe ??= await tab(fixture.id).catch(() => null);
    row.before_probe ??= row.initial ?? row.after_probe;
    if (!observeOnly || failure(row.initial)) {
      row.lifecycle = await dcJson("tab", "inspect", "--tab", fixture.id, "--lifecycle").catch((error) => ({ error: error.message }));
    }
    if (subscribed) await api(`/api/tabs/${fixture.id}/chat-raw/stop`, "POST", { subscriptionId: `endure-${fixture.id}-${n}` }).catch((error) => { row.subscription_stop_error = error.message; });
    console.log(`${observeOnly ? "OBSERVED" : row.pass ? "PASS" : "FAIL"} round=${n} tab=${fixture.id} provider=${fixture.provider} ${row.reason ?? row.answer ?? ""}`);
  }
  return row;
}

async function checkHealth() {
  const health = await api("/api/health");
  if (Number(health.instance?.api_port) !== port || (health.instance?.name || "") !== instance) throw new Error("Health does not identify selected DEV instance");
  const head = await command(["-c", "git rev-parse HEAD"]);
  const build = health.build_sha ?? health.build;
  if (!build) throw new Error("DEV health reports no build SHA");
  if (!head.startsWith(build)) {
    const backend = await command(["-c", `git merge-base --is-ancestor ${build} HEAD && git diff --name-only ${build} HEAD -- src-tauri cli plugins packages/plugin-sdk || echo NOT_ANCESTOR`]);
    if (backend.trim()) throw new Error(`DEV build ${build} differs from HEAD ${head} in backend or plugin files; build once before the eval`);
  }
  return health;
}

async function setup() {
  result.health = await checkHealth();
  const modelOverrides = { ...(instance ? { opencode: "deepseek/deepseek-flash" } : {}),
    ...JSON.parse(process.env.ENDURE_MODEL_OVERRIDES ?? "{}") };
  result.model_overrides = modelOverrides;
  const providers = await dcJson("agent", "providers");
  result.provider_descriptors = providers;
  const inventory = await dcJson("plugin", "list");
  const policies = new Map();
  for (const plugin of inventory.plugins ?? []) {
    const directory = plugin.install_path || plugin.path ||
      (plugin.state === "bundled" && plugin.source?.kind === "system_channel"
        ? join(root, "plugins", plugin.source.path) : null);
    if (!directory) continue;
    const path = join(directory, "plugin.json");
    if (!existsSync(path)) continue;
    const manifest = JSON.parse(readFileSync(path, "utf8"));
    if (manifest.id !== plugin.id) throw new Error(`Plugin inventory/manifest mismatch: ${plugin.id}`);
    policies.set(plugin.id, { ...firstTurnPolicy(manifest), manifest_path: path });
  }
  const workspaces = await dcJson("workspace", "list");
  const workspaceName = process.env.ENDURE_WORKSPACE_NAME || "Endure";
  if (workspaces.some((workspace) => workspace.name === workspaceName)) throw new Error(`${workspaceName} already exists; preserve it and review the previous fixture manifest before explicitly closing it`);
  const records = await tabs();
  const watched = new Set(workspaces.filter((workspace) => ["General", "Rcqa"].includes(workspace.name)).map((workspace) => workspace.id));
  result.existing = records.filter((record) => watched.has(record.workspace_id)).map((record) => ({
    id: record.id, provider: record.agent_type, cohort: "existing", baseline: record,
  }));
  const config = await dcJson("config", "get");
  const general = records.find((record) => record.protected && record.role_profile === "general_agent") ?? records.find((record) => record.protected && record.agent_type);
  if (!instance && (!general || !config.general_agent?.first_run_complete)) throw new Error("General must have completed Get started; eval does not change global setup");
  const workspace = await dcJson("workspace", "create", workspaceName, "--provision");
  result.workspace = workspace;
  save();
  for (const cohort of ["fresh", "used"]) {
    for (const provider of (instance ? ["codex", "claude", "opencode", "grok"] : ["codex", "claude", "opencode", "grok", "aider"])) {
      if (provider === "grok" && process.env.ENDURE_DEFER_GROK === "1") {
        result.coverage.push({ provider, cohort, status: "deferred owner login" });
        save();
        continue;
      }
      const descriptor = providers.find((candidate) => candidate.id === provider);
      if (!descriptor || descriptor.availability === "unavailable" || descriptor.cli_status?.installed === false) {
        result.coverage.push({ provider, cohort, status: ["aider", "grok"].includes(provider) ? "optional unavailable" : "required unavailable", reason: descriptor ? JSON.stringify(descriptor.cli_status ?? descriptor.availability) : "not registered" });
        save();
        continue;
      }
      result.coverage.push({ provider, cohort, status: "launch requested" });
      const fixture = { provider, cohort, requested_model: modelOverrides[provider], first_turn_policy: policies.get(provider) ?? firstTurnPolicy(null) };
      result.fixtures.push(fixture);
      save();
      try {
        const spawned = await dcJson("agent", "spawn", provider, "--workspace", workspace.id,
          "--cwd", workspace.cwd, "--title", `${provider}${cohort === "fresh" ? "F" : "U"}`,
          "--description", `Restart endurance ${cohort} fixture`, "--non-user-facing",
          ...(fixture.requested_model ? ["--model", fixture.requested_model] : []));
        fixture.id = spawned.tab_id;
        if (!fixture.id) throw new Error(`Spawn did not return tab_id: ${JSON.stringify(spawned)}`);
      } catch (error) { fixture.spawn_error = error.message; }
      save();
    }
    for (let sibling = 1; sibling <= (instance ? 0 : 2); sibling++) {
      const provider = config.general_agent.agent_provider;
      const fixture = { provider, cohort: `general-${cohort}`, sibling, first_turn_policy: policies.get(provider) ?? firstTurnPolicy(null) };
      result.fixtures.push(fixture);
      save();
      try {
        const spawned = await api("/api/agents/general/sessions", "POST", {
          workspaceId: general.workspace_id, providerId: fixture.provider, cwd: general.cwd,
          role: "general_agent", intent: "fresh_general",
        });
        fixture.id = spawned.tab_id;
        if (!fixture.id) throw new Error("General spawn did not return tab_id");
      } catch (error) { fixture.spawn_error = error.message; }
      save();
    }
  }
  const entry = { round: 0, rows: [] };
  result.rounds.push(entry);
  for (const fixture of result.fixtures) {
    if (fixture.cohort.endsWith("used")) entry.rows.push(await probe(fixture, 0));
    else if (fixture.id) {
      const signal = AbortSignal.timeout(deadlineMs);
      const events = new Events(signal);
      let record, reason, snapshot;
      try {
        await events.open();
        record = await waitReady(events, fixture.id, signal, fixture.first_turn_policy);
        snapshot = await snapshotAfterEvent(events, () => transcript(fixture.id, signal));
        const hasTurns = snapshot.messages.some((message) => ["user", "assistant"].includes(kind(message)));
        if (hasTurns) reason = "Fresh fixture has conversation turns before any eval probe";
        fixture.empty_session_id = hasTurns ? null : record.agent_session_id;
        fixture.last_session = hasTurns ? record.agent_session_id : null;
      } catch (error) { reason = error.message; }
      finally { events.close(); }
      entry.rows.push({ ...fixture, before_probe: record, transcript: snapshot, pass: !reason, reason: reason ?? "No user or assistant turn before restart" });
    } else entry.rows.push({ ...fixture, pass: false, reason: fixture.spawn_error });
    save();
  }
  result.setup_complete = true;
  save();
}

function captureLogs(n) {
  const lines = newEvidence(logLines(join(dataDir, "logs")), result.log_baseline ?? []);
  writeFileSync(join(out, `round-${n}.log`), `${lines.join("\n")}\n`);
  return lines;
}

async function runRound(n) {
  const health = await checkHealth();
  const restart = JSON.parse(readFileSync(join(out, `restart-${n}.json`), "utf8"));
  if (!restart.pass || !restartObserved(restart.before, restart.after)) throw new Error("restart_not_observed");
  const entry = { round: n, health, restart, rows: [], started_at: result.round_started_at,
    health_observed_at: new Date().toISOString() };
  result.rounds.push(entry);
  save();
  let workspace;
  const readiness = [];
  for (const fixture of result.fixtures.filter((fixture) => fixture.id)) {
    const signal = AbortSignal.timeout(deadlineMs);
    const events = new Events(signal);
    try {
      await events.open();
      const record = await tab(fixture.id, signal, events);
      if (record && workspace !== record.workspace_id) {
        await dc("workspace", "focus", record.workspace_id);
        workspace = record.workspace_id;
      }
      await dc("tab", "focus", "--tab", fixture.id);
      readiness.push(await waitReady(events, fixture.id, signal, fixture.first_turn_policy, Boolean(fixture.has_answer), true));
    } catch (error) {
      readiness.push({ id: fixture.id, readiness_error: error.message });
    } finally { events.close(); }
  }
  entry.initial_readiness = readiness.map((record) => ({ ...record }));
  const signal = AbortSignal.timeout(deadlineMs);
  const events = new Events(signal);
  try {
    await events.open();
    const settled = await tabs(signal, events);
    entry.restore_readiness = reconcileReadiness(readiness, settled, result.fixtures);
    entry.materialized_pty_count = settled.filter((record) => record.pty_id).length;
    entry.restore_ready_at = new Date().toISOString();
    const unresolved = result.fixtures.filter((fixture) => fixture.id).filter((fixture) => {
      const observed = entry.restore_readiness.find((record) => record.id === fixture.id);
      const current = settled.find((record) => record.id === fixture.id);
      return observed?.readiness_error || failure(current) || !restoreReady(current, fixture);
    });
    if (unresolved.length) throw new Error(`Restore barrier unresolved: ${unresolved.map((fixture) => fixture.id).join(", ")}`);
    entry.all_ready_at = entry.restore_ready_at;
    entry.stop_to_health_ms = Date.parse(entry.health_observed_at) - Date.parse(entry.started_at);
    entry.stop_to_all_ready_ms = Date.parse(entry.all_ready_at) - Date.parse(entry.started_at);
    save();
  } catch (error) {
    entry.barrier_error = error.message;
    const lines = captureLogs(n);
    for (const fixture of result.fixtures) {
      const record = fixture.id ? await tab(fixture.id).catch(() => null) : null;
      const observed = entry.restore_readiness?.find((record) => record.id === fixture.id);
      entry.rows.push({ ...fixture, pass: false, previous_session: fixture.last_session,
        phase: "restore_readiness_failed", before_probe: record,
        reason: observed?.readiness_error || failure(record) || `Not probed: restore barrier failed: ${error.message}`,
        lifecycle: fixture.id ? await dcJson("tab", "inspect", "--tab", fixture.id, "--lifecycle").catch(() => null) : null,
        log_evidence: fixture.id ? lines.filter((line) => line.includes(fixture.id)) : [] });
    }
    save();
    throw error;
  } finally { events.close(); }
  const probeFixture = async (fixture) => {
    entry.rows.push(await probe(fixture, n));
    save();
  };
  if (instance) await Promise.all(result.fixtures.map(probeFixture));
  else for (const fixture of result.fixtures) await probeFixture(fixture);
  for (const row of entry.rows.filter((row) => instance && !row.pass && row.id)) {
    const began = performance.now();
    row.failure_health = await checkHealth().catch((error) => ({ error: error.message }));
    row.failure_health_ms = Math.round(performance.now() - began);
    row.serial_retry = row.provider === "grok" && row.send_started_at
      ? { pass: false, observed_only: true, reason: "One prompt per Grok fixture per round is authorized; submitted probe is retained without a second prompt" }
      : await probe(result.fixtures.find((fixture) => fixture.id === row.id), `${n}_retry`);
    save();
  }
  for (const fixture of result.existing) {
    entry.rows.push(await probe(fixture, n, true));
    save();
  }
  const lines = captureLogs(n);
  for (const row of entry.rows) row.log_evidence = row.id ? lines.filter((line) => line.includes(row.id)) : [];
  const owners = new Map();
  for (const row of entry.rows.filter((row) => !row.observed_only)) {
    const session = row.after_probe?.agent_session_id;
    if (!session) continue;
    if (owners.has(session)) {
      const other = owners.get(session);
      row.pass = other.pass = false;
      row.reason = other.reason = `Session ${session} is bound to both ${other.id} and ${row.id}`;
    } else owners.set(session, row);
  }
  entry.rows.sort((a, b) => `${a.cohort}-${a.provider}-${a.id}`.localeCompare(`${b.cohort}-${b.provider}-${b.id}`));
  entry.finished_at = new Date().toISOString();
  save();
}

async function main() {
  mkdirSync(out, { recursive: true });
  if (mode === "setup") await setup();
  else if (mode === "repair-setup") {
    if (!result.setup_complete || result.rounds.some((entry) => entry.round > 0)) throw new Error("Repair setup only before restart 1");
    const [provider, adoptFreshId] = roundArgument.split(":");
    if (!["claude", "codex", "opencode", "grok"].includes(provider)) throw new Error("Unknown setup provider");
    const inventory = await dcJson("plugin", "list");
    const plugin = inventory.plugins.find((plugin) => plugin.id === provider);
    if (!plugin) throw new Error(`Provider plugin absent: ${provider}`);
    const directory = plugin.install_path || plugin.path ||
      (plugin.state === "bundled" && plugin.source?.kind === "system_channel" ? join(root, "plugins", plugin.source.path) : null);
    const manifestPath = join(directory, "plugin.json");
    const policy = { ...firstTurnPolicy(JSON.parse(readFileSync(manifestPath, "utf8"))), manifest_path: manifestPath };
    const entry = result.rounds.find((entry) => entry.round === 0);
    for (const cohort of ["fresh", "used"]) {
      let fixture = result.fixtures.find((candidate) => candidate.provider === provider && candidate.cohort === cohort);
      if (!fixture) {
        fixture = { provider, cohort, requested_model: result.model_overrides?.[provider] };
        result.fixtures.push(fixture);
      }
      fixture.first_turn_policy = policy;
      if (!fixture.id && cohort === "fresh" && adoptFreshId) {
        const record = await tab(adoptFreshId);
        if (!record || record.agent_type !== provider || record.workspace_id !== result.workspace.id || record.cwd !== result.workspace.cwd)
          throw new Error("Explicit adoption does not match the owned fixture workspace/provider/cwd");
        fixture.id = adoptFreshId;
      }
      save();
      if (!fixture.id) {
        try {
          const spawned = await dcJson("agent", "spawn", provider, "--workspace", result.workspace.id,
            "--cwd", result.workspace.cwd, "--title", `${provider}${cohort === "fresh" ? "F" : "U"}`,
            "--description", `Restart endurance ${cohort} fixture`, "--non-user-facing",
            ...(fixture.requested_model ? ["--model", fixture.requested_model] : []));
          fixture.id = spawned.tab_id;
          if (!fixture.id) throw new Error("Spawn did not return tab_id");
          delete fixture.spawn_error;
        } catch (error) { fixture.spawn_error = error.message; }
        save();
      }
      const row = !fixture.id ? {...fixture, pass: false, reason: fixture.spawn_error}
        : cohort === "fresh" ? await observeFresh(fixture)
        : fixture.has_answer ? { ...fixture, pass: true, reason: "Existing completed seed retained; no extra turn sent" }
        : await probe(fixture, 0);
      const original = entry.rows.find((candidate) => candidate.provider === provider && candidate.cohort === cohort);
      if (original) (original.setup_recoveries ??= []).push({ ...row, observed_at: new Date().toISOString() });
      else entry.rows.push(row);
      result.coverage = result.coverage.filter((candidate) => candidate.provider !== provider || candidate.cohort !== cohort);
      result.coverage.push({ provider, cohort, status: row.pass ? "setup ready" : "setup failed", reason: row.pass ? undefined : row.reason });
      save();
    }
  }
  else if (mode === "switch") {
    const [provider, ...modelParts] = roundArgument.split(":");
    const model = modelParts.join(":");
    const fixture = result.fixtures.find((fixture) => fixture.provider === provider && fixture.cohort === "used");
    if (!fixture?.has_answer) throw new Error(`No answered used fixture for ${provider}`);
    const before = await tab(fixture.id);
    const row = { provider, id: fixture.id, requested_model: model, session_before: before.agent_session_id, started_at: new Date().toISOString() };
    result.model_switches ??= [];
    result.model_switches.push(row);
    save();
    try {
      const boundary = recordBoundary(provider, before.cwd, before.agent_session_id);
      row.response = await dcJson("agent", "model", "--tab", fixture.id, model);
      row.probe = await probe(fixture, "switch");
      row.persisted = nextTurnModel(boundary, row.probe.expected);
      row.session_after = row.probe.after_probe?.agent_session_id;
      row.pass = row.probe.pass && row.session_before === row.session_after && Boolean(row.persisted)
        && (row.persisted.model === model || (provider === "claude" && row.persisted.model.toLowerCase().includes(model.toLowerCase())));
    } catch (error) { row.pass = false; row.reason = error.message; }
    row.finished_at = new Date().toISOString();
    save();
    console.log(JSON.stringify(row));
  }
  else if (mode === "health") { console.log(JSON.stringify(await checkHealth())); }
  else if (mode === "before") {
    await checkHealth();
    result.round_started_at = new Date().toISOString();
    result.log_baseline = logLines(join(dataDir, "logs")).map(digest);
    result.pre_restart_tabs = await tabs();
    save();
  } else if (mode === "round") await runRound(round);
  else if (mode === "abort") {
    const evidencePath = join(out, `restart-${round}.json`);
    const restart = existsSync(evidencePath) ? JSON.parse(readFileSync(evidencePath, "utf8")) : null;
    result.fatal = `restart_not_observed: DEV restart ${round} failed; see restart-${round}.log`;
    const lines = captureLogs(round);
    result.rounds.push({ round, restart, started_at: result.round_started_at, rows: result.fixtures.map((fixture) => ({
      ...fixture, pass: false, phase: "restart_not_observed", reason: result.fatal,
      log_evidence: lines.filter((line) => line.includes(fixture.id)),
    })) });
    save();
    writeFileSync(join(out, "complete"), "1\n");
  } else if (mode === "finish") {
    const failed = result.coverage.some((entry) => entry.status === "required unavailable") ||
      result.rounds.some((entry) => entry.rows.some((row) => !row.pass && !row.observed_only)) ||
      result.rounds.filter((entry) => entry.round > 0).length !== 3 ||
      result.rounds.filter((entry) => entry.round > 0).some((entry) => !entry.restart?.pass || !restartObserved(entry.restart.before, entry.restart.after));
    result.finished_at = new Date().toISOString();
    result.pass = !failed;
    save();
    writeFileSync(join(out, "complete"), failed ? "1\n" : "0\n");
    console.log(`Results: ${join(out, "results.md")}`);
  } else throw new Error(`Unknown mode ${mode}`);
}
main().catch((error) => {
  result.fatal = error.message;
  save();
  console.error(error.message);
  process.exitCode = 2;
});
