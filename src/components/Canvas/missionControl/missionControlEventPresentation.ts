import type { PlanEvent } from "./missionControlState";

export function curatorPresentation(text: string): { preview: string; body: string } | null {
  let value: unknown;
  try { value = JSON.parse(text); } catch { return null; }
  if (!value || typeof value !== "object") return null;
  const round = value as Record<string, unknown>;
  if (typeof round.number !== "number" || typeof round.outcome !== "string") return null;
  const line = (key: string) => typeof round[key] === "string" ? round[key] as string : "";
  const list = (key: string) => Array.isArray(round[key])
    ? (round[key] as unknown[]).filter((item): item is string => typeof item === "string") : [];
  const move = line("move_line") || line("summary");
  const delay = typeof round.next_delay_secs === "number" && Number.isFinite(round.next_delay_secs)
    ? `${Math.round(round.next_delay_secs / 360) / 10}h` : "unknown";
  const preview = `round ${round.number} · ${line("outcome").replaceAll("_", " ")} · ${move} · next in ${delay}`;
  const sections = [
    preview,
    ...([ ["RETRO", "retro"], ["LEARN", "learn"], ["NEXT-CHECK", "next_checks"] ] as const)
      .flatMap(([label, key]) => list(key).length ? [`${label}:\n${list(key).join("\n")}`] : []),
    `ASKED: ${line("asked") || "none"}`,
    ...(line("ready") ? [`READY: ${line("ready")}`] : []),
  ];
  return { preview, body: sections.join("\n\n") };
}

export interface PresentedEvent {
  event: PlanEvent;
  tasks: Array<{ id: string; name: string }>;
}

export function collapseChecklistEvents(entries: readonly PlanEvent[]): PresentedEvent[] {
  const result: PresentedEvent[] = [];
  for (const entry of entries) {
    const prior = result[result.length - 1];
    const task = entry.task_id ? { id: entry.task_id, name: entry.task_name ?? entry.task_id } : null;
    if (entry.kind === "tab_todo_linked" && prior?.event.kind === entry.kind && prior.event.text === entry.text) {
      prior.event.count += entry.count;
      prior.event.last_ts = entry.last_ts;
      if (task && !prior.tasks.some(({ id }) => id === task.id)) prior.tasks.push(task);
    } else {
      result.push({ event: { ...entry }, tasks: task ? [task] : [] });
    }
  }
  return result;
}
