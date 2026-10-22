import { formatDuration } from "../../../lib/timeLabels";

export function curatorPresentation(text: string): { preview: string; body: string } | null {
  let value: unknown;
  try { value = JSON.parse(text); } catch { return null; }
  if (!value || typeof value !== "object") return null;
  const round = value as Record<string, unknown>;
  if (typeof round.number !== "number" || typeof round.outcome !== "string") return null;
  const line = (key: string) => typeof round[key] === "string" ? round[key] as string : "";
  const list = (key: string) => Array.isArray(round[key])
    ? (round[key] as unknown[]).filter((item): item is string => typeof item === "string") : [];
  const skipped = !line("move_line").trim() || round.run_id === null || ["fallback", "failed"].includes(line("outcome"));
  const reason = (line("reason").trim() || line("summary").trim()).replaceAll("_", " ");
  const move = skipped ? `round ${round.number} skipped${reason ? `: ${reason}` : ""}` : `round ${round.number}`;
  const delay = typeof round.next_delay_secs === "number" && Number.isFinite(round.next_delay_secs)
    ? formatDuration(round.next_delay_secs * 1000) : "unknown";
  const preview = [move, !skipped && line("outcome").replaceAll("_", " "), !skipped && line("move_line"), `next in ${delay}`].filter(Boolean).join(" · ");
  const sections = [
    preview,
    ...([ ["RETRO", "retro"], ["LEARN", "learn"], ["NEXT-CHECK", "next_checks"] ] as const)
      .flatMap(([label, key]) => list(key).length ? [`${label}:\n${list(key).join("\n")}`] : []),
    ...(line("asked") ? [`ASKED: ${line("asked")}`] : []),
    ...(line("ready") ? [`READY: ${line("ready")}`] : []),
  ];
  return { preview, body: sections.join("\n\n") };
}
