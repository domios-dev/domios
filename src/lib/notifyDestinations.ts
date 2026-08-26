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

/** The channels a message can actually leave through. The daemon already
 *  answers this per entry; a channel with no transport is not a destination
 *  anyone may pick, so it is never offered rather than offered and refused. */
export function offerableChannels(
  channels: readonly PushChannelDescriptor[],
): PushChannelDescriptor[] {
  return channels.filter((channel) => channel.control_available);
}

/** The surface's columns: the local surfaces, then whatever the registry
 *  resolved and can carry. A channel appears here because the daemon listed it,
 *  never because a column was written down. */
export function destinationColumns(
  channels: readonly PushChannelDescriptor[],
): DestinationColumn[] {
  return [
    ...LOCAL_DESTINATIONS.map(({ id, label }) => ({ id, label, kind: "local" as const })),
    ...offerableChannels(channels).map(({ id, label }) => ({
      id,
      label,
      kind: "channel" as const,
    })),
  ];
}

/** The surfaces the app draws itself: one toggle each, because they are two
 *  fixed things rather than a set that grows. */
export function localDestinations(
  columns: readonly DestinationColumn[],
): DestinationColumn[] {
  return columns.filter((column) => column.kind === "local");
}

/** The push side, which is a registry and therefore one picker rather than a
 *  control per member: a level names the channels it reaches, none or several. */
export function pushDestinations(
  columns: readonly DestinationColumn[],
): DestinationColumn[] {
  return columns.filter((column) => column.kind === "channel");
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

/** The visible caption of the push row, and the tail of the picker's own name. */
export const PUSH_DESTINATION_LABEL = "Push channels";

/** The accessible name of one level's push picker: it is a second control on a
 *  row that already has one, so it names the level it belongs to. */
export function pushPickerLabel(levelLabel: string): string {
  return destinationCellLabel(levelLabel, PUSH_DESTINATION_LABEL);
}

/** What the closed picker reads. Selecting nothing is a configuration, so it
 *  says so rather than reading like an unanswered control. */
export const NO_CHANNEL_SUMMARY = "No channel";

export function destinationSummary(selected: readonly DestinationColumn[]): string {
  if (selected.length === 0) return NO_CHANNEL_SUMMARY;
  return selected.map((destination) => destination.label).join(", ");
}

/** Drop one channel's key entirely. A closed column is an absent key rather
 *  than a stored false, which is the shape the daemon already reads back. */
function withoutChannel(choice: AgentLevelChoice, channel: string): AgentLevelChoice {
  const next: Record<string, unknown> = { ...choice };
  delete next[channel];
  return next as unknown as AgentLevelChoice;
}

/** Move one column of one row. The two local surfaces are named fields and keep
 *  an explicit answer in both directions; a channel is a flattened key, so
 *  closing it removes the key the same way the daemon serialises it. */
export function setLevelDestination(
  current: AgentLevelChoice[],
  level: string,
  column: DestinationColumn,
  routed: boolean,
): AgentLevelChoice[] {
  return current.map((choice) => {
    if (choice.level !== level) return choice;
    if (column.kind === "local") return { ...choice, [column.id]: routed };
    if (routed) return { ...choice, [column.id]: true };
    return withoutChannel(choice, column.id);
  });
}
