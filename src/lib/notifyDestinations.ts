import type { AgentLevelChoice } from "./types";

/** Mirrors `NotifyDestination::LOCAL` in `codeterm-types`: the surfaces the app
 *  draws itself, in column order. Push channels are deliberately absent —
 *  there is no closed set of them to restate here. */
export const LOCAL_DESTINATIONS = [
  { id: "bell", label: "Ring the bell" },
  { id: "desktop", label: "System notification" },
] as const;

export type LocalDestinationId = (typeof LOCAL_DESTINATIONS)[number]["id"];

/** One registered push channel, as the daemon's registry reports it. */
export interface PushChannelDescriptor {
  /** The bridge service id. Never rendered. */
  id: string;
  /** The column caption, owned by the registry entry. */
  label: string;
  /** Whether this channel's own controls may be offered at all. */
  control_available: boolean;
}

export interface DestinationColumn {
  id: string;
  label: string;
  /** Which side of the surface this column is. A channel column also decides
   *  who, which a locally drawn surface never has to. */
  kind: "local" | "channel";
}

/** The grid's columns: the local surfaces, then whatever the registry resolved.
 *  A channel appears here because the daemon listed it, never because a column
 *  was written down. */
export function destinationColumns(
  channels: readonly PushChannelDescriptor[],
): DestinationColumn[] {
  return [
    ...LOCAL_DESTINATIONS.map(({ id, label }) => ({ id, label, kind: "local" as const })),
    ...channels.map(({ id, label }) => ({ id, label, kind: "channel" as const })),
  ];
}

/** Read one cell. An absent key reads as closed, exactly as the daemon reads
 *  it: a column nobody named is a column nobody chose, so a channel the registry
 *  gains arrives off rather than already reaching someone. */
export function isRouted(choice: AgentLevelChoice, destination: string): boolean {
  return Boolean(choice[destination]);
}

/** The accessible name of one cell. Composed from the level and the
 *  destination it governs, so no control carries a name a reader cannot place. */
export function destinationCellLabel(levelLabel: string, destinationLabel: string): string {
  return `${levelLabel}: ${destinationLabel}`;
}
