import type { Tab, TabAgentState, TabStatus } from "./types";
import { providerErrorReason } from "./providerSignals";
import { shellStateLabel } from "./shellCommandState";

export interface AgentState extends Omit<TabAgentState, "state"> {
  state: TabAgentState["state"] | "quiet";
}

export type TabActivityInput = Omit<Partial<Tab>, "agent_state"> & { agent_state?: AgentState | null };

export interface TabActivity {
  kind: "working" | "waiting-on-user" | "idle" | "starting" | "failed";
  label: string;
  /** The label is the generic work placeholder because the provider sent no status line. */
  genericWork?: boolean;
  /** How long the finished turn took, as the provider reported it on screen. */
  lastTurnMs?: number;
  /** Live background tasks of the own session; they never make a finished turn active. */
  backgroundTasks: number;
  status: TabStatus;
  state: AgentState["state"];
  subtype?: TabAgentState["subtype"];
  phase: "thinking" | "streaming" | "compacting" | "work";
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

const QUOTED_PROGRAM = /^\s*"((?:[^"\\]|\\.)*)"/;

/** A provider command line reads as its program name, not an escaped absolute path. */
export function readableActivityLine(line: string): string {
  const quoted = QUOTED_PROGRAM.exec(line);
  const program = quoted ? quoted[1] : line.trim().split(/\s+/, 1)[0] ?? "";
  if (!/[\\/]/.test(program)) return line.trim();
  const name = program.replace(/\\\\/g, "\\").split(/[\\/]/).pop()?.replace(/\.(exe|cmd|bat)$/i, "") ?? program;
  const rest = quoted ? line.slice(quoted[0].length) : line.trim().slice(program.length);
  return `${name}${rest}`.trim();
}

/** A pending approval whose question is a raw command line shows as the question kind instead. */
function questionText(question: string | null | undefined): string | null {
  const text = question?.trim();
  if (!text) return null;
  return readableActivityLine(text) === text ? text : null;
}

const startingStages = new Set<Tab["boot_stage"]>([
  "pty_starting", "pty_spawned", "reader_ready", "shell_ready", "agent_launching",
  "launch_written", "provider_ready", "preparing_role_instructions", "delivering_instructions",
  "launch_deadline_elapsed", "launch_binding_unconfirmed",
]);

function lazyFirstTurnComposerIsLive(tab: TabActivityInput | null | undefined): boolean {
  const launch = tab?.provisional_launch;
  return tab?.boot_stage === "launch_binding_unconfirmed"
    && launch?.awaiting_binding_ack === true
    && launch.first_turn_input_ready === true
    && Boolean(tab.pty_id)
    && launch.pty_id === tab.pty_id
    && Boolean(tab.session_binding_nonce)
    && launch.binding_nonce === tab.session_binding_nonce;
}

function boundSessionRetiresShellStartup(tab: TabActivityInput | null | undefined): boolean {
  return tab?.agent_booting === false
    && Boolean(tab.agent_session_id?.trim())
    && Boolean(tab.pty_id)
    && Boolean(tab.session_binding_nonce?.trim())
    && tab.reported_session_binding_nonce === tab.session_binding_nonce
    && (tab.boot_stage === "pty_starting" || tab.boot_stage === "pty_spawned"
      || tab.boot_stage === "reader_ready" || tab.boot_stage === "shell_ready");
}

export function tabActivity(tab: TabActivityInput | null | undefined, status?: TabStatus | null): TabActivity {
  if (tab?.frozen) return {
    kind: "idle", label: "Sleeping", status: "Idle", state: "idle", backgroundTasks: 0,
    active: false, canStop: false, quiet: false, launching: false, phase: "work", shellSucceeded: null,
  };
  const live = tab?.agent_state;
  const shell = tab?.shell_command_state;
  const declaresAgent = Boolean(tab?.agent_type) || tab?.backing?.kind === "chat_backend";
  const sessionBound = tab?.session_bound === true || Boolean(tab?.agent_session_id?.trim())
    || (tab?.backing?.kind === "chat_backend" && Boolean(tab.backing.session_id?.trim()));
  const utility = tab?.tab_type === "Terminal" && !tab.agent_type && !tab.role && !tab.roleless_tracked
    && (!tab.backing || tab.backing.kind === "pty");
  const readyShell = utility && shell && tab?.boot_stage === "shell_ready";
  const lazyComposerReady = lazyFirstTurnComposerIsLive(tab);
  const launching = ((tab?.boot_stage !== "agent_ready" && tab?.agent_booting === true)
    || (startingStages.has(tab?.boot_stage) && !readyShell)
    || tab?.boot_stage === "awaiting_owner_prompt"
    || (declaresAgent && !sessionBound && tab?.boot_stage !== "agent_ready"))
    && !lazyComposerReady && !boundSessionRetiresShellStartup(tab);
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
  const backgroundTasks = sessionBound ? Object.keys(tab?.provider_signals?.background?.running ?? {}).length : 0;
  const quiet = kind === "idle" && live?.state === "quiet";
  const state = quiet ? "quiet" : kind === "working" ? "working" : kind === "waiting-on-user" ? "question" : "idle";
  const matchingLive = kind !== "starting" && kind !== "failed" && live?.state === state;
  const subtype = state === "question" && matchingLive ? live?.subtype : undefined;
  const phase = kind === "working" && matchingLive && (live?.phase === "thinking" || live?.phase === "streaming" || live?.phase === "compacting") ? live.phase : "work";
  const question = kind === "waiting-on-user" ? questionText(tab?.pending_prompt?.question) : null;
  const providerReason = kind !== "working" && kind !== "starting" && kind !== "failed"
    ? providerErrorReason(tab?.provider_signals?.api_error) : null;
  const statusLine = matchingLive ? live?.status_line?.trim() : null;
  const providerLine = statusLine ? readableActivityLine(statusLine) : null;
  const genericWork = kind === "working" && !utility && !providerLine && phase !== "compacting";
  const lastTurnMs = kind === "idle" && matchingLive && typeof live?.elapsed_ms === "number" ? live.elapsed_ms : undefined;
  const label = question || providerReason || (utility && !launching && kind !== "failed" ? shellStateLabel(shell)
    : phase === "compacting" ? "Compacting context…" : providerLine
      || (kind === "failed" ? status === "Dead" ? "Ended" : "Launch failed" : kind === "starting" ? "Starting…"
        : kind === "working" ? "Working"
          : kind === "idle" && matchingLive && live?.phase && idleOutcomeLabels[live.phase] ? idleOutcomeLabels[live.phase]
          : kind === "waiting-on-user" ? questionLabels[subtype ?? "unknown"]
          : quiet ? "Session quiet — rebind to continue" : ""));
  return {
    kind, label, genericWork, lastTurnMs, backgroundTasks, state, subtype, phase, quiet, launching,
    status: kind === "failed" ? status === "Dead" ? "Dead" : "Error"
      : kind === "waiting-on-user" ? "Waiting" : kind === "working" ? "Working" : "Idle",
    active: kind === "working",
    canStop: kind === "working",
    shellSucceeded: shell?.activity === "idle" && shell.last_exit_code != null ? shell.last_exit_code === 0 : null,
  };
}
