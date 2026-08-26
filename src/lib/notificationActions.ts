/**
 * The one place a notification button may reach a Tauri command, and the one
 * ingress that decides which stored rows are allowed to carry one.
 *
 * Provenance is settled where the action is created, not where it is clicked:
 * the daemon constructs an `invoke` naming a member of the closed `DaemonCommand`
 * set, and a caller-supplied action parses into a type with no invoke member at
 * all. Neither surface that renders these rows re-derives that decision.
 */
import { invoke } from "./bridge";
import type { CallerNotificationAction, DaemonCommand, NotificationAction } from "./types";

type ActionArgs = Record<string, unknown>;

/**
 * The daemon allowlist, as a total map. A command absent from this table is not
 * callable from a notification, and a command added to `DaemonCommand` without
 * a runner here is a compile error rather than a dead button.
 */
const DAEMON_INVOKE_RUNNERS: Record<DaemonCommand, (args: ActionArgs) => Promise<unknown>> = {
  // The arguments are rebuilt field by field, so nothing else rides along.
  respond_provider_update_prompt: (args) =>
    invoke("respond_provider_update_prompt", {
      promptId: args.promptId,
      accept: args.accept === true,
    }),
};

/** The caller-facing kinds. Typed so `invoke` is not expressible here. */
export const CALLER_ACTION_KINDS: ReadonlyArray<CallerNotificationAction["type"]> = [
  "url",
  "dismiss",
  "answer",
];

export const DAEMON_COMMANDS = Object.keys(DAEMON_INVOKE_RUNNERS) as DaemonCommand[];

/** Own keys only: an inherited property name is not a command. */
function daemonRunner(command: unknown): ((args: ActionArgs) => Promise<unknown>) | undefined {
  if (typeof command !== "string") return undefined;
  return Object.prototype.hasOwnProperty.call(DAEMON_INVOKE_RUNNERS, command)
    ? DAEMON_INVOKE_RUNNERS[command as DaemonCommand]
    : undefined;
}

function isDaemonCommand(candidate: unknown): candidate is DaemonCommand {
  return daemonRunner(candidate) !== undefined;
}

function isArgs(candidate: unknown): candidate is ActionArgs {
  return typeof candidate === "object" && candidate !== null && !Array.isArray(candidate);
}

/**
 * Adopt one action off a stored or relayed row. An `invoke` that does not name
 * a member of the daemon set has no runner and no target, so it is dropped;
 * every other kind is rebuilt field by field, which is what keeps a legacy
 * command name from riding in on a field this union no longer has.
 */
function adoptStoredAction(raw: unknown): NotificationAction | null {
  if (typeof raw !== "object" || raw === null) return null;
  const candidate = raw as {
    type?: unknown;
    label?: unknown;
    value?: unknown;
    command?: unknown;
    args?: unknown;
  };
  if (typeof candidate.label !== "string") return null;
  const label = candidate.label;
  switch (candidate.type) {
    case "invoke":
      if (!isDaemonCommand(candidate.command)) return null;
      return {
        type: "invoke",
        label,
        command: candidate.command,
        args: isArgs(candidate.args) ? candidate.args : undefined,
      };
    case "url":
      return typeof candidate.value === "string"
        ? { type: "url", label, value: candidate.value }
        : null;
    case "dismiss":
      return { type: "dismiss", label };
    case "answer":
      return { type: "answer", label };
    default:
      return null;
  }
}

/**
 * The frontend ingress for every stored or relayed action set. A row written
 * before the daemon/caller split loads with its invoke stripped rather than
 * silently accepted: the message is still the user's, the button is not.
 */
export function adoptStoredActions(raw: unknown): NotificationAction[] | undefined {
  if (raw === undefined || raw === null) return undefined;
  if (!Array.isArray(raw)) return undefined;
  const adopted: NotificationAction[] = [];
  for (const candidate of raw) {
    const action = adoptStoredAction(candidate);
    if (action) adopted.push(action);
  }
  return adopted;
}

/**
 * Run one notification action. Shared by the bell panel and the toast so the
 * two surfaces cannot drift apart, and reaching a command is a lookup in the
 * closed daemon table rather than a call on a name read off the row.
 */
export async function runNotificationAction(action: NotificationAction): Promise<void> {
  try {
    switch (action.type) {
      case "url":
        if (action.value) window.open(action.value, "_blank");
        return;
      case "invoke":
        // A table lookup over the closed daemon set, not a check on a target:
        // a caller-supplied action has no field this could have read.
        await daemonRunner(action.command)?.(action.args ?? {});
        return;
      default:
        return;
    }
  } catch (error) {
    console.warn(`Notification action '${action.label}' failed:`, error);
  }
}
