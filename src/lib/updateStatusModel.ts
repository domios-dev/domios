import type { UpdateAvailabilitySnapshot } from "./updateAvailability";
import { CORE_PLUGIN_CHANNEL, updateSourceLabel, updateSourceMessage, type UpdateRow } from "./updatePanelModel";

/** "Restart required" is reserved for a newer version staged and waiting for its restart. */
export function updateRestartDescription(row: Pick<UpdateRow, "group" | "state" | "targetVersion" | "channelError">, appStaged: boolean): string | null {
  if (row.targetVersion === null || row.channelError || row.state === "completed" || row.state === "failed") return null;
  if (row.group === "Interface") return "No restart required";
  if (row.group !== "App") return null;
  return appStaged ? "Restart required" : "Update restarts Domios";
}

/** A failed check is not an updater state: the indicator only knows offers and update runs. */
export function updateIndicator(rows: readonly UpdateRow[], options: { busy: boolean; checking: boolean; failed: boolean; completed?: boolean }) {
  const offers = rows.some(row => !row.channelError && row.state === "available");
  const failed = options.failed || rows.some(row => row.state === "failed");
  const active = options.busy || options.checking || rows.some(row => row.state === "active");
  const completed = options.completed || rows.some(row => row.state === "completed");
  const restart = rows.some(row => !row.channelError && row.group === "App" && row.state !== "completed");
  const activeRow = rows.find(row => row.state === "active");
  const state = failed ? "failed" : active ? "active" : offers ? "available" : completed ? "completed" : "idle";
  const description = failed ? "Update failed. Open updates for details."
    : options.checking ? "Checking for updates."
      : active ? activeRow ? `Updating ${activeRow.name}.` : "Updating components."
        : offers ? `Updates available. ${restart ? "App update restarts Domios." : rows.some(row => row.group === "Interface" && row.state === "available") ? "Interface update needs no restart." : ""}`.trim()
          : completed ? "Updates completed." : "Open updates.";
  return { state, description, active, offers };
}

const CHANNEL_UNAVAILABLE_PREFIX = "Plugin channel unavailable: ";

const PATH_LABEL: Readonly<Record<string, string>> = { direct: "direct download", github_api: "GitHub API fallback" };

export interface CheckDetail { source: string; line: string }

/** The quiet record of the last check: which path served each source, or what each failure was. */
export function updateCheckDetails(snapshot: UpdateAvailabilitySnapshot): CheckDetail[] {
  const errors = classifyUpdateStatus(snapshot).errors;
  const details = Object.entries(snapshot.served_by ?? {})
    .filter(([source]) => !errors[source])
    .map(([source, path]) => ({ source, line: `${updateSourceLabel(source)}: served by ${PATH_LABEL[path] ?? path}` }));
  return [...details, ...Object.entries(errors).map(([source, message]) => ({
    source, line: `${updateSourceLabel(source)}: ${updateSourceMessage(message)}`,
  }))];
}

export const COULD_NOT_CHECK = "Couldn't check right now.";

export function classifyUpdateStatus(snapshot: UpdateAvailabilitySnapshot) {
  const information = new Map<string, string>();
  for (const item of snapshot.items) {
    if (item.kind === "plugin" && item.reason === "local_ahead" && item.state !== "core_silent") {
      information.set(item.id, `${item.id}: installed version ${item.installed ?? "unknown"} is ahead of channel version ${item.latest ?? "unknown"}`);
    }
  }
  const errors: Record<string, string> = {};
  for (const [source, message] of Object.entries(snapshot.errors)) {
    const failures = message.split("; ").filter(part => !/^system plugin '[^']+' is not installable: local_ahead$/.test(part));
    if (failures.length) errors[source] = failures.join("; ");
  }
  return { errors, information: [...information.values()] };
}

/** Sources a finished check could not reach; the core channel syncs silently and only shows in the details. */
export function unavailableSourceKeys(errors: Readonly<Record<string, string>>): string[] {
  return Object.keys(errors).filter(source => source !== CORE_PLUGIN_CHANNEL);
}

export interface UpdateFailure { message: string; details: string; channelUnavailable: boolean }

/** Describes a failed update the owner started; a failed check never reaches here. */
export function classifyUpdateFailure(detail: unknown): UpdateFailure {
  const raw = detail instanceof Error ? detail.toString() : String(detail);
  try {
    const refusal: unknown = JSON.parse(raw.replace(/^Error: /, ""));
    if (typeof refusal === "object" && refusal !== null && "kind" in refusal
      && refusal.kind === "native_update_admission_refused" && "message" in refusal && typeof refusal.message === "string") {
      return { message: refusal.message, details: raw, channelUnavailable: false };
    }
  } catch {}
  const channel = raw.indexOf(CHANNEL_UNAVAILABLE_PREFIX);
  if (channel >= 0) return { message: raw.slice(channel), details: raw, channelUnavailable: true };
  return {
    message: "Couldn't update. Try again later.",
    details: raw,
    channelUnavailable: false,
  };
}
