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
export function updateIndicator(rows: readonly UpdateRow[], options: { busy: boolean; checking: boolean; failed: boolean; appStaged: boolean }) {
  const offers = rows.some(row => !row.channelError && row.state === "available");
  const failed = options.failed || rows.some(row => row.state === "failed");
  const updating = options.busy || rows.some(row => row.state === "active");
  const visible = offers || failed || updating || options.appStaged;
  const active = updating || options.checking || options.appStaged;
  const restart = rows.some(row => !row.channelError && row.group === "App" && row.state !== "completed");
  const activeRow = rows.find(row => row.state === "active");
  const state = failed ? "failed" : active ? "active" : offers ? "available" : "idle";
  const description = failed ? "Update failed. Open updates for details."
    : options.appStaged ? "Restart required."
      : options.checking ? "Checking for updates."
        : active ? activeRow ? `Updating ${activeRow.name}.` : "Updating components."
          : offers ? `Updates available. ${restart ? "App update restarts Domios." : rows.some(row => row.group === "Interface" && row.state === "available") ? "Interface update needs no restart." : ""}`.trim()
            : "Open updates.";
  return { state, description, active, offers, visible };
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

export interface UpdateFailure { message: string; details: string; channelUnavailable: boolean; recovery?: string }

export interface BlockingUpdateTab { tab_id: string; title: string; workspace: string; command: string }
const BLOCKING_TABS_MARKER = "[update-blocking-tabs]";

export function blockingUpdateTabs(raw: string | null | undefined): BlockingUpdateTab[] {
  if (!raw?.includes(BLOCKING_TABS_MARKER)) return [];
  try {
    const values: unknown = JSON.parse(raw.slice(raw.indexOf(BLOCKING_TABS_MARKER) + BLOCKING_TABS_MARKER.length));
    if (!Array.isArray(values)) return [];
    const tabs = values.filter((value): value is BlockingUpdateTab => {
      if (typeof value !== "object" || value === null) return false;
      const record = value as Record<string, unknown>;
      return ["tab_id", "title", "workspace", "command"].every(key => typeof record[key] === "string");
    });
    return [...new Map(tabs.map(tab => [tab.tab_id, tab])).values()];
  } catch { return []; }
}

const RELAUNCH_RECOVERY = "Relaunch the Domios desktop app and retry the update.";

/** Describes a failed update the owner started; a failed check never reaches here. */
export function classifyUpdateFailure(detail: unknown): UpdateFailure {
  const raw = detail instanceof Error ? detail.toString() : String(detail);
  const blockers = blockingUpdateTabs(raw);
  if (blockers.length) return { message: raw.slice(0, raw.indexOf(BLOCKING_TABS_MARKER)).replace(/^Error: /, "").trim(), details: raw, channelUnavailable: false };
  if (/Update failed: expected version|installer launch|client cleanup did not complete/.test(raw)) {
    return { message: raw.replace(/^Error: /, ""), details: raw, channelUnavailable: false };
  }
  try {
    const refusal: unknown = JSON.parse(raw.replace(/^Error: /, ""));
    if (typeof refusal === "object" && refusal !== null && "kind" in refusal && "message" in refusal && typeof refusal.message === "string") {
      if (refusal.kind === "native_update_admission_refused") {
        return { message: refusal.message, details: raw, channelUnavailable: false };
      }
      if (refusal.kind === "install_handoff_failed") {
        const recovery = "recovery" in refusal && typeof refusal.recovery === "string" ? refusal.recovery : undefined;
        return { message: refusal.message, details: raw, channelUnavailable: false, recovery };
      }
    }
  } catch {}
  if (/server attachment is unavailable|native host(?: attachment)? is unavailable|native updater attachment is unavailable|attached server process identity changed|NotAttached/i.test(raw)) {
    return {
      message: "The native host attachment to the Domios server is unavailable; the installer handoff was refused.",
      details: raw,
      channelUnavailable: false,
      recovery: RELAUNCH_RECOVERY,
    };
  }
  if (/native_capability_unavailable/i.test(raw)) {
    return {
      message: "Automatic installation requires a native application host attached to this server.",
      details: raw,
      channelUnavailable: false,
      recovery: RELAUNCH_RECOVERY,
    };
  }
  const channel = raw.indexOf(CHANNEL_UNAVAILABLE_PREFIX);
  if (channel >= 0) return { message: raw.slice(channel), details: raw, channelUnavailable: true };
  const httpStatus = raw.match(/\bHTTP (?:status )?(\d{3})\b/i)?.[1];
  if (httpStatus) return { message: `Update request failed (HTTP ${httpStatus}).`, details: raw, channelUnavailable: false };
  return {
    message: "Couldn't update. Try again later.",
    details: raw,
    channelUnavailable: false,
  };
}
