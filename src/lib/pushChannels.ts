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
        recipients: existing?.recipients ?? [],
        per_scope_limit: existing?.per_scope_limit ?? DEFAULT_PER_SCOPE_LIMIT,
        ...next,
      },
    },
  };
}

/** One channel's chosen audience. An absent entry is an empty audience, and an
 *  empty audience is nobody: nothing here falls back to the admission list. */
export function channelRecipients(config: AppConfig, id: string): string[] {
  return config.notifications?.channels?.[id]?.recipients ?? [];
}

/** The ids this channel's surface may offer, read from that service's inbound
 *  admission list. A source of options only — never the audience itself, which
 *  is why selecting nobody here still means nobody rather than everyone. */
export function configuredUsers(config: AppConfig, id: string): string[] {
  const integrations = config.integrations as unknown as
    | Record<string, unknown>
    | undefined;
  if (integrations?.[`${id}_enabled`] !== true) return [];
  const admitted = integrations[`${id}_allowed_users`];
  if (!Array.isArray(admitted)) return [];
  const offered: string[] = [];
  for (const entry of admitted) {
    const id = String(entry).trim();
    if (id && !offered.includes(id)) offered.push(id);
  }
  return offered;
}

/** Opening a channel's column answers who too, but only when the answer is
 *  unambiguous. Several configured ids is a choice the user makes; preselecting
 *  all of them would make opening one level a broadcast nobody asked for. */
export function preselectedAudience(configured: readonly string[]): string[] {
  return configured.length === 1 ? [configured[0]] : [];
}

/** Add or drop one id, keeping the rest of the selection as it stands. */
export function toggleRecipient(
  current: readonly string[],
  id: string,
  selected: boolean,
): string[] {
  if (!selected) return current.filter((entry) => entry !== id);
  return current.includes(id) ? [...current] : [...current, id];
}

/** The accessible name of one offered recipient, composed from the caption the
 *  registry gave the channel and the id it offers, so no control is named by
 *  hand and no two channels' rows read alike. */
export function recipientLabel(channelLabel: string, id: string): string {
  return `${channelLabel}: ${id}`;
}
