import type { BootStage, Tab } from "./types";

export interface WakeRequestView {
  requested: boolean;
  pending: boolean;
  error: string | null;
}

export interface RestoreStageView {
  phase: "sleeping" | "restoring" | "failed" | "attention" | "ready";
  label: string;
  detail: string | null;
  steps: Array<{ id: string; label: string; state: "pending" | "current" | "complete" | "failed" }>;
  current: number;
}

const sessionStages = new Set<BootStage>(["shell_ready", "agent_launching", "launch_written"]);
const connectingStages = new Set<BootStage>([
  "provider_ready", "preparing_role_instructions", "delivering_instructions",
  "launch_binding_unconfirmed", "launch_deadline_elapsed", "awaiting_owner_prompt",
]);
const backendStages = new Set<BootStage>(["pty_starting", "pty_spawned", "reader_ready"]);

export function restoreStageView(tab: Partial<Tab>, wake?: Partial<WakeRequestView>): RestoreStageView {
  const chat = tab.backing?.kind === "chat_backend";
  const agent = Boolean(tab.agent_type) || chat;
  const definitions = agent ? [
    { id: "backend", label: chat ? "Starting runtime" : "Starting terminal" },
    { id: "session", label: tab.agent_session_id || (tab.backing?.kind === "chat_backend" && tab.backing.session_id) ? "Resuming session" : "Opening session" },
    { id: "connection", label: "Connecting" },
    { id: "ready", label: "Ready" },
  ] : [{ id: "backend", label: "Starting terminal" }, { id: "ready", label: "Ready" }];
  const stage = tab.boot_stage;
  const runtime = tab.agent_runtime_state;
  const activeStage = Boolean(stage && (backendStages.has(stage) || sessionStages.has(stage) || connectingStages.has(stage)));
  const ready = !tab.frozen && stage !== "spawn_failed" && runtime?.state !== "degraded"
    && (stage === "agent_ready" || (!agent && (stage === "shell_ready" || stage === "pty_reattached"))
      || (!activeStage && !tab.agent_booting && runtime?.state === "loaded"));
  let phase: RestoreStageView["phase"] = "restoring";
  let detail: string | null = null;
  let current = 0;
  if (ready) {
    phase = "ready";
    current = definitions.length - 1;
  } else if (wake?.error) {
    phase = "failed";
    detail = wake.error;
  } else if (tab.frozen) {
    phase = wake?.requested ? "restoring" : "sleeping";
    current = -1;
    detail = wake?.requested ? "Wake requested" : null;
  } else if (wake?.pending && stage === "spawn_failed") {
    current = -1;
    detail = "Wake requested";
  } else if (stage === "spawn_failed" || (!activeStage && runtime?.state === "degraded")) {
    phase = "failed";
    detail = tab.boot_failure_reason || runtime?.reason || "The session could not be restored.";
  } else if (stage === "awaiting_owner_prompt") {
    phase = "attention";
    current = agent ? 2 : 0;
    detail = tab.pending_prompt?.question || "The session needs your input.";
  } else if (stage && connectingStages.has(stage)) {
    current = agent ? 2 : 0;
    detail = stage === "launch_binding_unconfirmed" || stage === "launch_deadline_elapsed"
      ? tab.boot_failure_reason || "Waiting for the session to connect" : null;
  } else if (stage && sessionStages.has(stage)) {
    current = agent && !(chat && stage === "agent_launching") ? 1 : 0;
  } else if (!activeStage && tab.session_bound) {
    current = agent ? 2 : 0;
  }
  return {
    phase,
    label: phase === "sleeping" ? "Sleeping" : phase === "failed" ? "Could not restore" : phase === "ready" ? "Ready"
      : phase === "attention" ? "Needs your input" : "Restoring…",
    detail,
    current,
    steps: definitions.map((step, index) => ({ ...step,
      state: phase === "ready" || index < current ? "complete"
        : index === current ? phase === "failed" ? "failed" : "current" : "pending",
    })),
  };
}
