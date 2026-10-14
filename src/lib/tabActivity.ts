import type { Tab, TabAgentState, TabStatus } from "./types";
import { shellStateLabel } from "./shellCommandState";

export interface AgentState extends Omit<TabAgentState, "state"> {
  state: TabAgentState["state"] | "quiet";
}

export type TabActivityInput = Omit<Partial<Tab>, "agent_state"> & { agent_state?: AgentState | null };

export interface TabActivity {
  kind: "working" | "waiting-on-user" | "idle" | "starting" | "failed";
  label: string;
  status: TabStatus;
  state: AgentState["state"];
  subtype?: TabAgentState["subtype"];
  phase: "thinking" | "streaming" | "work";
  active: boolean;
  canStop: boolean;
  quiet: boolean;
  launching: boolean;
  shellSucceeded: boolean | null;
}

const questionLabels: Record<NonNullable<TabAgentState["subtype"]>, string> = {
  model_choice: "Choose a model", model_picker: "Choose a model", permission: "Permission needed",
  auth: "Sign in to continue", rate_limit: "Rate limited", update: "Update available",
  error: "Needs attention", unknown: "Waiting for input",
};

const idleOutcomeLabels: Record<string, string> = { interrupted: "Interrupted", failed: "Turn failed" };

const startingStages = new Set<Tab["boot_stage"]>([
  "pty_starting", "pty_spawned", "reader_ready", "shell_ready", "agent_launching",
  "launch_written", "provider_ready", "preparing_role_instructions", "delivering_instructions",
  "launch_deadline_elapsed", "launch_binding_unconfirmed",
]);

export function tabActivity(tab: TabActivityInput | null | undefined, status?: TabStatus | null): TabActivity {
  const live = tab?.agent_state;
  const shell = tab?.shell_command_state;
  const declaresAgent = Boolean(tab?.agent_type) || tab?.backing?.kind === "chat_backend";
  const sessionBound = Boolean(tab?.agent_session_id?.trim())
    || (tab?.backing?.kind === "chat_backend" && Boolean(tab.backing.session_id?.trim()));
  const utility = tab?.tab_type === "Terminal" && !tab.agent_type && !tab.role && !tab.roleless_tracked
    && (!tab.backing || tab.backing.kind === "pty");
  const readyShell = utility && shell && tab?.boot_stage === "shell_ready";
  const launching = tab?.agent_booting === true || (startingStages.has(tab?.boot_stage) && !readyShell)
    || tab?.boot_stage === "awaiting_owner_prompt" || (declaresAgent && !sessionBound && tab?.boot_stage !== "agent_ready");
  const authoritativeStatus = status ?? (live?.state === "working" ? "Working"
    : live?.state === "question" ? "Waiting" : "Idle");
  let kind: TabActivity["kind"];
  if (status === "Dead" || status === "Error" || tab?.boot_stage === "spawn_failed") kind = "failed";
  else if (tab?.pending_prompt || tab?.boot_stage === "awaiting_owner_prompt") kind = "waiting-on-user";
  else if (launching) kind = "starting";
  else if (authoritativeStatus === "Waiting") kind = "waiting-on-user";
  else if (utility && shell) kind = shell.activity === "running" ? "working" : "idle";
  else if (sessionBound && live?.state === "working") kind = "working";
  else kind = "idle";
  const quiet = kind === "idle" && live?.state === "quiet";
  const state = quiet ? "quiet" : kind === "working" ? "working" : kind === "waiting-on-user" ? "question" : "idle";
  const matchingLive = kind !== "starting" && kind !== "failed" && live?.state === state;
  const subtype = state === "question" && matchingLive ? live?.subtype : undefined;
  const phase = kind === "working" && matchingLive && (live?.phase === "thinking" || live?.phase === "streaming") ? live.phase : "work";
  const question = kind === "waiting-on-user" ? tab?.pending_prompt?.question?.trim() : null;
  const label = question || (utility && !launching && kind !== "failed" ? shellStateLabel(shell)
    : (matchingLive ? live?.status_line?.trim() : null)
      || (kind === "failed" ? "Launch failed" : kind === "starting" ? "Starting…"
        : kind === "working" ? matchingLive && live?.phase === "compacting" ? "Compacting" : "Working"
          : kind === "idle" && matchingLive && live?.phase && idleOutcomeLabels[live.phase] ? idleOutcomeLabels[live.phase]
          : kind === "waiting-on-user" ? questionLabels[subtype ?? "unknown"]
          : quiet ? "Session quiet — rebind to continue" : ""));
  return {
    kind, label, state, subtype, phase, quiet, launching,
    status: kind === "failed" ? status === "Dead" ? "Dead" : "Error"
      : kind === "waiting-on-user" ? "Waiting" : kind === "working" ? "Working" : "Idle",
    active: kind === "working",
    canStop: kind === "working",
    shellSucceeded: shell?.activity === "idle" && shell.last_exit_code != null ? shell.last_exit_code === 0 : null,
  };
}
