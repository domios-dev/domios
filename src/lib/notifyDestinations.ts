/** Mirrors `NotifyDestination::ALL` in `codeterm-types`: the closed set of
 *  places one level may reach, in column order. Every surface that renders or
 *  names a destination maps over this list rather than restating it. */
export const NOTIFY_DESTINATIONS = [
  { id: "bell", label: "Ring the bell" },
  { id: "desktop", label: "System notification" },
  { id: "telegram", label: "Telegram" },
] as const;

export type NotifyDestination = (typeof NOTIFY_DESTINATIONS)[number];
export type NotifyDestinationId = NotifyDestination["id"];

/** The accessible name of one cell. Composed from the level and the
 *  destination it governs, so no control carries a name a reader cannot place. */
export function destinationCellLabel(levelLabel: string, destinationLabel: string): string {
  return `${levelLabel}: ${destinationLabel}`;
}
