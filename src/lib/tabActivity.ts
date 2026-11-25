import type { Tab, TabAgentState, TabStatus } from "./types";
import type { BackgroundProcessEntry } from "./backgroundProcesses";
import { groupBackgroundProcesses } from "./backgroundProcessGroups";
import { runtimeResponseDeadlinePending } from "./agentRuntimeState";
import { providerErrorReason } from "./providerSignals";
import { shellStateLabel } from "./shellCommandState";
import { tabFailure, tabLaunched, tabSessionBound, type TabFailure } from "./tabFailure";
import { liveRuntimePrompt } from "./promptAnswers";

export function runtimeBackgroundProcesses(
  tab: TabActivityInput | null | undefined,
): readonly BackgroundProcessEntry[] {
  if (tab?.remote_channels && tab.remote_channels.control !== "available") return [];
  const runtime = tab?.agent_runtime_state;
  const roster = runtime?.backgroundProcesses;
  if (!roster || runtime?.state !== "loaded") return [];
  if (!tab?.agent_session_id || !tab.session_binding_nonce) return [];
  if (
    roster.rootThreadId !== tab.agent_session_id
    || roster.bindingNonce !== tab.session_binding_nonce
    || roster.epoch !== runtime.epoch
  ) {
    return [];
  }
  return roster.processes;
}

export function backgroundProcessLabel(count: number): string {
  return `${count} background ${count === 1 ? "process" : "processes"}`;
}

export function idleBackgroundLabel(activity: TabActivity): string | null {
  if (activity.kind !== "idle" || !activity.backgroundTasks) return null;
  const { backgroundProcesses, backgroundSubagents, backgroundTasks } = activity;
  const other = backgroundTasks - backgroundProcesses - backgroundSubagents;
  return [
    backgroundProcesses && `${backgroundProcessLabel(backgroundProcesses)} running`,
    backgroundSubagents && `${backgroundSubagents} sub-${backgroundSubagents === 1 ? "agent" : "agents"} running`,
    other && `${other} background ${other === 1 ? "task" : "tasks"} running`,
  ].filter(Boolean).join(" · ");
}

type BackgroundCounts = Pick<TabActivity, "backgroundTasks" | "backgroundProcesses" | "backgroundSubagents">;
const noBackgroundWork: BackgroundCounts = { backgroundTasks: 0, backgroundProcesses: 0, backgroundSubagents: 0 };

function liveBackgroundCounts(
  tab: TabActivityInput | null | undefined,
  sessionBound: boolean,
): BackgroundCounts {
  if (!sessionBound) return noBackgroundWork;
  const roster = tab?.provider_signals?.background;
  const runtimeProcesses = runtimeBackgroundProcesses(tab);
  const groups = groupBackgroundProcesses(runtimeProcesses);
  const known = new Set(runtimeProcesses.map(entry => entry.processId));
  const counts = { ...noBackgroundWork, backgroundTasks: groups.length, backgroundProcesses: groups.length };
  for (const id of Object.keys(roster?.running ?? {})) {
    if (known.has(id)) continue;
    counts.backgroundTasks++;
    if (roster?.kinds?.[id] === "shell") counts.backgroundProcesses++;
    if (roster?.kinds?.[id] === "subagent") counts.backgroundSubagents++;
  }
  return counts;
}

export interface AgentState extends Omit<TabAgentState, "state"> {
  state: TabAgentState["state"] | "quiet";
}

export type TabActivityInput = Omit<Partial<Tab>, "agent_state"> & { agent_state?: AgentState | null };

export interface TabActivity {
  kind: "working" | "waiting-on-user" | "idle" | "starting" | "failed";
  label: string;
  /** Present exactly when `kind` is "failed": what failed and how to recover. */
  failure?: TabFailure;
  /** The label is the generic work placeholder because the provider sent no status line. */
  genericWork?: boolean;
  /** How long the finished turn took, as the provider reported it on screen. */
  lastTurnMs?: number;
  /** Live background tasks of the own session; they never make a finished turn active. */
  backgroundTasks: number;
  backgroundProcesses: number;
  backgroundSubagents: number;
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

const SHORT_QUESTION_CHARS = 80;

/** A status line summarises a pending prompt; only a short one-line question is quoted verbatim. */
function promptStatus(tab: TabActivityInput | null | undefined, subtype: TabAgentState["subtype"]): string | null {
  const prompt = tab?.pending_prompt;
  const text = prompt?.question?.trim();
  if (!prompt || !text) return null;
  const permission = subtype === "permission"
    || (tab?.backing?.kind !== "chat_backend"
      && (tab?.pending_prompt_kind === "permission" || tab?.pending_prompt_kind === "automatic_permission"));
  if (permission) return "Waiting for your permission";
  if (readableActivityLine(text) !== text) return null;
  return !text.includes("\n") && text.length <= SHORT_QUESTION_CHARS ? text : "Waiting for your answer";
}

const startingStages = new Set<Tab["boot_stage"]>([
  "pty_starting", "pty_spawned", "reader_ready", "shell_ready", "agent_launching",
  "launch_written", "provider_ready", "preparing_role_instructions", "delivering_instructions",
  "launch_deadline_elapsed", "launch_binding_unconfirmed",
]);

export function lazyFirstTurnComposerIsLive(tab: TabActivityInput | null | undefined): boolean {
  const launch = tab?.provisional_launch;
  return tab?.boot_stage === "provider_ready"
    && !tab.frozen && !tab.agent_booting
    && launch?.awaiting_binding_ack === true
    && launch.first_turn_input_ready === true
    && Boolean(tab.pty_id)
    && launch.pty_id === tab.pty_id
    && Boolean(tab.session_binding_nonce)
    && launch.binding_nonce === tab.session_binding_nonce;
}

export function hasExitedChatRecipient(tab: TabActivityInput | null | undefined): boolean {
  return Boolean(tab?.exited_chat_recipient) && !tab?.agent_type;
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
    kind: "idle", label: "Sleeping", status: "Idle", state: "idle", ...noBackgroundWork,
    active: false, canStop: false, quiet: false, launching: false, phase: "work", shellSucceeded: null,
  };
  const chatBacked = tab?.backing?.kind === "chat_backend";
  const runtimePermission = chatBacked && tab.pending_prompt_kind === "permission" && liveRuntimePrompt(tab) !== null;
  const orphanedPermission = chatBacked && tab.agent_state?.subtype === "permission" && !runtimePermission;
  const live = orphanedPermission ? undefined : tab?.agent_state;
  const shell = tab?.shell_command_state;
  const declaresAgent = Boolean(tab?.agent_type) || tab?.backing?.kind === "chat_backend";
  const sessionBound = tabSessionBound(tab);
  const utility = tab?.tab_type === "Terminal" && !tab.agent_type && !tab.role && !tab.roleless_tracked
    && (!tab.backing || tab.backing.kind === "pty");
  const readyShell = utility && shell && tab?.boot_stage === "shell_ready";
  const lazyComposerReady = lazyFirstTurnComposerIsLive(tab);
  const curatorWorking = tab?.role_profile === "curator" && status === "Working"
    && sessionBound && Boolean(tab.session_binding_nonce)
    && tab.reported_session_binding_nonce === tab.session_binding_nonce;
  const launching = ((tab?.boot_stage !== "agent_ready" && tab?.agent_booting === true)
    || (startingStages.has(tab?.boot_stage) && !readyShell)
    || tab?.boot_stage === "awaiting_owner_prompt"
    || (declaresAgent && !sessionBound && tab?.boot_stage !== "agent_ready"))
    && !lazyComposerReady && !boundSessionRetiresShellStartup(tab) && !curatorWorking;
  const runtimeFailed = tab?.backing?.kind === "chat_backend"
    && (tab.agent_runtime_state?.state === "degraded"
      || (tab.agent_runtime_state?.state === "unloaded" && Boolean(tab.agent_runtime_state.reason)))
    && !runtimeResponseDeadlinePending(tab.agent_runtime_state, tab.boot_failure_reason);
  const authoritativeStatus = orphanedPermission && status === "Waiting"
    ? tab?.agent_runtime_state?.active_turn_started_at_ms != null ? "Working" : "Idle"
    : status ?? "Idle";
  let kind: TabActivity["kind"];
  if (runtimeFailed || status === "Dead" || status === "Error" || tab?.boot_stage === "spawn_failed") kind = "failed";
  else if (tab?.pending_prompt || tab?.boot_stage === "awaiting_owner_prompt") kind = "waiting-on-user";
  else if (launching) kind = "starting";
  else if (authoritativeStatus === "Waiting") kind = "waiting-on-user";
  else if (utility && shell) kind = shell.activity === "running" ? "working" : "idle";
  else if (sessionBound && authoritativeStatus === "Working") kind = "working";
  else kind = "idle";
  const failure = kind === "failed" ? tabFailure({ status, bootStage: tab?.boot_stage, runtime: tab?.agent_runtime_state,
    launched: tabLaunched(tab), chatBacked: tab?.backing?.kind === "chat_backend" }) : undefined;
  const background = liveBackgroundCounts(tab, sessionBound);
  const quiet = kind === "idle" && live?.state === "quiet";
  const state = quiet ? "quiet" : kind === "working" ? "working" : kind === "waiting-on-user" ? "question" : "idle";
  const matchingLive = kind !== "starting" && kind !== "failed" && live?.state === state;
  const subtype = state === "question"
    ? runtimePermission ? "permission" : matchingLive ? live?.subtype : undefined
    : undefined;
  const phase = kind === "working" && matchingLive && (live?.phase === "thinking" || live?.phase === "streaming" || live?.phase === "compacting") ? live.phase : "work";
  const question = kind === "waiting-on-user" ? promptStatus(tab, subtype) : null;
  const providerReason = kind !== "working" && kind !== "starting" && kind !== "failed"
    ? providerErrorReason(tab?.provider_signals?.api_error) : null;
  const statusLine = matchingLive ? live?.status_line?.trim() : null;
  const providerLine = statusLine ? readableActivityLine(statusLine) : null;
  const genericWork = kind === "working" && !utility && !providerLine && phase !== "compacting";
  const lastTurnMs = kind === "idle" && matchingLive && typeof live?.elapsed_ms === "number" ? live.elapsed_ms : undefined;
  const label = question || providerReason || (utility && !launching && kind !== "failed" ? shellStateLabel(shell)
    : phase === "compacting" ? "Compacting context…" : providerLine
      || (failure ? failure.label : kind === "starting" ? "Starting…"
        : kind === "working" ? "Working"
          : kind === "idle" && matchingLive && live?.phase && idleOutcomeLabels[live.phase] ? idleOutcomeLabels[live.phase]
          : kind === "waiting-on-user" ? questionLabels[subtype ?? "unknown"]
          : quiet ? "Session quiet — rebind to continue" : ""));
  return {
    kind, label, failure, genericWork, lastTurnMs, ...background, state, subtype, phase, quiet, launching,
    status: kind === "failed" ? status === "Dead" ? "Dead" : "Error"
      : kind === "waiting-on-user" ? "Waiting" : kind === "working" ? "Working" : "Idle",
    active: kind === "working",
    canStop: kind === "working",
    shellSucceeded: shell?.activity === "idle" && shell.last_exit_code != null ? shell.last_exit_code === 0 : null,
  };
}
