import type { UpdateAvailabilitySnapshot } from "./updateAvailability";
import type { UpdateItem, UpdateRow } from "./updatePanelModel";

export function updateRestartDescription(group: UpdateItem["group"]): string | null {
  return group === "App" ? "Restart required" : group === "Interface" ? "No restart required" : null;
}

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
        : offers ? `Updates available. ${restart ? "App update requires a restart." : rows.some(row => row.group === "Interface" && row.state === "available") ? "Interface update needs no restart." : ""}`.trim()
          : completed ? "Updates completed." : "Open updates.";
  return { state, description, active, offers };
}

export const CORE_PLUGIN_CHANNEL = "codeterm-core";
const CHANNEL_UNAVAILABLE_PREFIX = "Plugin channel unavailable: ";
const DISCOVERY_SOURCES = new Set(["app", "frontend"]);

export interface ChannelDiagnostic { channel: string; message: string }

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

/** Channel failures no update row carries, such as the silently synced core channel. */
export function channelDiagnostics(errors: Readonly<Record<string, string>>, shownChannels: ReadonlySet<string>): ChannelDiagnostic[] {
  return Object.entries(errors)
    .filter(([channel]) => !DISCOVERY_SOURCES.has(channel) && !shownChannels.has(channel))
    .map(([channel, message]) => ({ channel, message }));
}

export type UpdateFailureOrigin = "background" | "check" | "update";
export interface UpdateFailure { message: string; details: string; channelUnavailable: boolean }

export function classifyUpdateFailure(origin: UpdateFailureOrigin, detail: unknown): UpdateFailure | null {
  if (origin === "background") return null;
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
    message: origin === "check" ? "Couldn't check for updates. Try again later." : "Couldn't update. Try again later.",
    details: raw,
    channelUnavailable: false,
  };
}
