export type ScheduleFreq = "daily" | "every-2" | "every-3" | "weekly" | "off";

function pad2(n: number): string {
  return n.toString().padStart(2, "0");
}

export function cronToSchedule(schedule: string): { freq: ScheduleFreq; time: string; custom: boolean } {
  const s = schedule.trim();
  let m: RegExpMatchArray | null;
  m = s.match(/^(\d+) (\d+) \* \* \*$/);
  if (m) return { freq: "daily", time: `${pad2(+m[2])}:${pad2(+m[1])}`, custom: false };
  m = s.match(/^(\d+) (\d+) \*\/2 \* \*$/);
  if (m) return { freq: "every-2", time: `${pad2(+m[2])}:${pad2(+m[1])}`, custom: false };
  m = s.match(/^(\d+) (\d+) \*\/3 \* \*$/);
  if (m) return { freq: "every-3", time: `${pad2(+m[2])}:${pad2(+m[1])}`, custom: false };
  m = s.match(/^(\d+) (\d+) \* \* 0$/);
  if (m) return { freq: "weekly", time: `${pad2(+m[2])}:${pad2(+m[1])}`, custom: false };
  return { freq: "daily", time: "06:00", custom: true };
}

export function dreamScheduleLabel(schedule: string): string {
  const parsed = cronToSchedule(schedule);
  if (parsed.custom) return schedule;
  const label = { daily: "Nightly", "every-2": "Every 2 nights", "every-3": "Every 3 nights", weekly: "Sundays", off: "Off" }[parsed.freq];
  return `${label} at ${parsed.time}`;
}
