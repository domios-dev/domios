import { FILM_NOW } from "./clock";
import type { BlueprintDocument } from "../../../../src/lib/blueprintApi";
import type { PipelineView } from "../../../../src/components/Automation/pipelineModel";
import { buildBlueprintGraph, type BlueprintGraphModel } from "../../../../src/components/Automation/blueprint/graphModel";

export const SERVICE_HEALTH_YAML = `schema_version: 2
kind: pipeline
name: Service health
trigger: {type: schedule, every: 5m}
steps:
  - id: health
    uses: http
    with: {url: https://api.example.com/health}
    routes: {up: "@stop", down: diagnose, failed: diagnose}
  - id: diagnose
    uses: agent
    does: Find why the health check failed and summarise the fix
    with: {provider: claude, profile: reader}
    routes: {done: "@next", failed: "@next"}
result: {to: notify}
`;

const DOCUMENT: BlueprintDocument = {
  schema_version: 2, kind: "pipeline", name: "Service health",
  trigger: { type: "schedule", every: "5m" },
  steps: [
    { id: "health", uses: "http", with: { url: "https://api.example.com/health" }, routes: { up: "@stop", down: "diagnose", failed: "diagnose" } },
    { id: "diagnose", uses: "agent", does: "Find why the health check failed and summarise the fix", with: { provider: "claude", profile: "reader" }, routes: { done: "@next", failed: "@next" } },
  ],
  result: { to: "notify" },
};

export const BLUEPRINT_RUN = {
  checkStarts: 3.2, checkDown: 4.05, diagnoseStarts: 5.4,
  diagnoseDone: 6.35, resultDelivering: 7.15, resultDelivered: 7.4,
} as const;

const SIGNALS = [
  { from: "$trigger", to: "health", start: 2.65, end: BLUEPRINT_RUN.checkStarts },
  { from: "health", to: "diagnose", start: 4.2, end: BLUEPRINT_RUN.diagnoseStarts },
  { from: "diagnose", to: "$result", start: BLUEPRINT_RUN.diagnoseDone, end: BLUEPRINT_RUN.resultDelivering },
];

export function signalAt(t: number) {
  const signal = SIGNALS.find((entry) => t >= entry.start && t <= entry.end);
  return signal ? { ...signal, progress: (t - signal.start) / (signal.end - signal.start) } : null;
}

export function serviceHealthAt(t: number): BlueprintGraphModel {
  const run = BLUEPRINT_RUN;
  const step = (id: string, start: number, end: number, outcome: string, summary: string) => ({
    step: id, state: t < start ? "pending" : t < end ? "running" : "done",
    outcome: t < end ? null : outcome, summary: t < start ? null : t < end ? "Reading evidence" : summary, at: FILM_NOW,
  });
  const pipeline: PipelineView = {
    id: "bp-service-health", name: "Service health", owner: { kind: "team", tab_id: null, team_id: "film-team" },
    state: t < run.resultDelivered ? "running" : "idle", nodes: [], edges: [], call_graph: DOCUMENT,
    runId: "run-service-health-1",
    retained: { id: "run-service-health-1", at: FILM_NOW, state: "running", incomplete: false, steps: [
      { step: "$trigger", state: "done", outcome: null, summary: "Every 5 minutes", at: FILM_NOW },
      step("health", run.checkStarts, run.checkDown, "down", "503 Service Unavailable"),
      step("diagnose", run.diagnoseStarts, run.diagnoseDone, "done", "Connection pool exhausted; restart api-db"),
      { step: "$result", state: t < run.resultDelivering ? "pending" : t < run.resultDelivered ? "delivering" : "delivered", outcome: null, summary: t < run.resultDelivered ? null : "Diagnosis delivered to notify", at: FILM_NOW },
    ] },
  };
  const build = buildBlueprintGraph(pipeline, new Set());
  let next = build.next();
  while (!next.done) next = build.next();
  return next.value;
}
