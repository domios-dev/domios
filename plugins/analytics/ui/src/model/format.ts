export {
  formatClock,
  formatCompact,
  formatCount,
  formatSharePct,
} from "../../../../../src/components/Canvas/missionControl/missionControlBoardModel";

const DAY = new Intl.DateTimeFormat("en-US", { weekday: "short", month: "short", day: "numeric" });

function pad(value: number): string {
  return String(value).padStart(2, "0");
}

/** "Wed, Sep 9" reads as "Wed Sep 9". */
export function formatDay(ms: number): string {
  return DAY.format(new Date(ms)).replace(",", "");
}

export function formatStamp(seconds: number): string {
  const date = new Date(seconds * 1000);
  return `${formatDay(date.getTime())}, ${pad(date.getHours())}:${pad(date.getMinutes())}`;
}
