import { RELAY_PROTOCOL_VERSION } from "../../../../src/lib/types";
import type { AutomationGraph, BlueprintSnapshot, BlueprintStep } from "../../../../src/components/Automation/types";
import type { CronJob, CronRunStatus, MeshPeersResponse, PairedPeer } from "../../../../src/lib/types";
import { ago, ahead, FILM_NOW, MINUTE } from "./clock";
import { filmRoute } from "./daemon";

export const JOBS_WORKSPACE = { id: "ws-fieldnote", name: "Fieldnote" };

function run(status: CronRunStatus, minutesAgo: number, minutes = 4): CronJob["last_run"] {
  const started = ago(minutesAgo);
  return { run_id: `run-${minutesAgo}`, started_at: started, finished_at: status === "Running" ? null : started + minutes * MINUTE, status, tab_id: null };
}

function cron(id: string, name: string, schedule: string, agent: string, model: string, lastRun: CronJob["last_run"], nextIn: number | null): CronJob {
  return {
    id, name, description: null, workspace_id: JOBS_WORKSPACE.id, team_id: null, schedule, enabled: nextIn !== null, agent, model,
    reasoning_effort: null, task: name, cwd: "C:/Developer/fieldnote", role: null, report_target_tab_id: null, oneshot: false, max_history: 20,
    created_at: ago(30 * 24 * 60), next_run: nextIn === null ? null : ahead(nextIn), jitter_max_ms: null, last_run: lastRun,
  };
}

export const CRONS: CronJob[] = [
  cron("cron-replica-health", "Check test replica health", "*/15 * * * *", "claude", "haiku", run("Running", 2), 13),
  cron("cron-standup", "Morning standup digest", "0 8 * * *", "claude", "opus", run("Success", 120, 6), 22 * 60),
  cron("cron-soak", "Nightly sync soak on gpu-rig", "30 2 * * *", "codex", "gpt-6-sol", run("Success", 450, 41), 16 * 60 + 30),
  cron("cron-telegram", "Telegram inbox triage", "0 */2 * * *", "claude", "sonnet", run("Success", 0, 3), 2 * 60),
  cron("cron-deps", "Weekly dependency audit", "0 17 * * 5", "claude", "sonnet", run("Failed", 6 * 24 * 60 + 17 * 60), null),
];

function step(index: number, title: string, state: BlueprintStep["state"], summary: string | null = null): BlueprintStep {
  return { index, name: title, kind: "command", title, state, outcome: state === "done" ? "ok" : null, started_at_ms: state === "waiting" ? null : ago(9 - index * 2), ended_at_ms: state === "done" ? ago(8 - index * 2) : null, result_summary: summary, reason: state === "failed" ? "exit 1 on windows-latest" : null };
}

function blueprint(id: string, name: string, state: BlueprintSnapshot["state"], trigger: NonNullable<BlueprintSnapshot["trigger"]>, steps: BlueprintStep[], minutesAgo: number): BlueprintSnapshot {
  const current = steps.findIndex((entry) => entry.state === "running");
  return {
    id, name, owner: { kind: "team", tab_id: null, team_id: "film-team" }, state, run_id: state === "running" ? `run-${id}` : null,
    current_step: current < 0 ? null : current, last_event_at_ms: ago(minutesAgo), trigger, steps,
    result: { target: { kind: "team", id: "film-team" }, state: state === "failed" ? "failed" : "waiting", reason: null, body_ref: null },
  };
}

export const BLUEPRINTS: BlueprintSnapshot[] = [
  blueprint("bp-release-gate", "Release gate 2.4", "running", { type: "event", summary: "When a lane merges to release/2.4" }, [
    step(0, "Real-merge lane/offline-sync-resolver", "done", "merged at 8c41e2f"), step(1, "Vitest + sync QA matrix", "running"), step(2, "Report to Helm", "waiting"),
  ], 3),
  blueprint("bp-replica-watch", "Test replica watch", "running", { type: "schedule", summary: "Every 10 minutes" }, [
    step(0, "Ping mac-mini, homelab, laptop", "running"), step(1, "Wake the Orchestrator on loss", "waiting"),
  ], 1),
  blueprint("bp-installer-smoke", "Windows installer smoke", "failed", { type: "event", summary: "When a release tag is pushed" }, [
    step(0, "Download MSI on gpu-rig", "done"), step(1, "Silent install + launch", "failed"),
  ], 38),
  blueprint("bp-beta-publish", "Publish the beta channel", "stopped", { type: "input", summary: "Started by hand" }, [
    step(0, "Build 2.4.0-beta.3", "done"), step(1, "Sign and upload", "waiting"),
  ], 3 * 60),
];

function peer(name: string, online: boolean, lastSeenMinutes: number): PairedPeer {
  return {
    instance_id: `peer-${name.toLowerCase()}`, instance_name: name, addresses: [], api_port: 7685, public_key: "", paired_at: new Date(ago(90 * 24 * 60)).toISOString(),
    last_seen: new Date(ago(lastSeenMinutes)).toISOString(), online, is_primary: false, access_level: "full", version: "0.42.0", relay_protocol_version: RELAY_PROTOCOL_VERSION,
  };
}

export const PEERS: MeshPeersResponse = {
  local_instance_name: "laptop",
  paired: [peer("mac-mini", true, 0), peer("homelab", true, 1), peer("gpu-rig", true, 0), peer("studio", false, 340), peer("pi-hub", true, 2)],
};

const graph = (workspaceId: string): AutomationGraph => ({
  workspace_id: workspaceId, team_id: null, all_teams: true, revision: `film-${FILM_NOW}`, scenarios: [], nodes: [], edges: [],
  blueprints: workspaceId === JOBS_WORKSPACE.id ? BLUEPRINTS : [],
});

filmRoute("GET", "/api/crons", (url) => {
  const workspaceId = url.searchParams.get("workspace_id");
  return CRONS.filter((job) => workspaceId === null || job.workspace_id === workspaceId);
});
filmRoute("GET", "/api/automation/graph/workspace", (url) => graph(url.searchParams.get("workspace_id") ?? ""));
filmRoute("GET", "/api/mesh/peers", () => PEERS);
filmRoute("POST", "/api/automation/blueprints/solutions/list", () => []);
