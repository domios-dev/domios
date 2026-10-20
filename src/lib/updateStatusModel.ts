import type { UpdateAvailabilitySnapshot } from "./updateAvailability";

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
  const channel = raw.indexOf(CHANNEL_UNAVAILABLE_PREFIX);
  if (channel >= 0) return { message: raw.slice(channel), details: raw, channelUnavailable: true };
  return {
    message: origin === "check" ? "Couldn't check for updates. Try again later." : "Couldn't update. Try again later.",
    details: raw,
    channelUnavailable: false,
  };
}
