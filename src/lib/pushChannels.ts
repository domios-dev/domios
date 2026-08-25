import type { AppConfig, PushChannelConfig } from "./types";

/** Mirrors the daemon's own fallback ceiling for a channel that never named one. */
export const DEFAULT_PER_SCOPE_LIMIT = 12;

/** Patch one channel's audience entry, keyed by its service id. Every channel
 *  editor writes through here, so no editor restates the shape of the map. */
export function channelPatch(
  current: Record<string, PushChannelConfig> | undefined,
  id: string,
  next: Partial<PushChannelConfig>,
): { channels: Record<string, PushChannelConfig> } {
  const existing = current?.[id];
  return {
    channels: {
      ...(current ?? {}),
      [id]: {
        enabled: existing?.enabled ?? false,
        recipients: existing?.recipients ?? [],
        per_scope_limit: existing?.per_scope_limit ?? DEFAULT_PER_SCOPE_LIMIT,
        ...next,
      },
    },
  };
}

/** One channel's audience as stored, or the pre-opt-in state when absent. */
export function channelAudience(
  config: AppConfig,
  id: string,
): PushChannelConfig | undefined {
  return config.notifications?.channels?.[id];
}

export function parseRecipients(text: string): string[] {
  return text
    .split(",")
    .map((entry) => entry.trim())
    .filter(Boolean);
}
