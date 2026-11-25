import { countTaskStrideLines, taskStridesLabel, type TaskStridesBlockData } from "./taskStrides";

export interface SystemReminderSection {
  /** Marker name without the `codeterm-` prefix; empty for text outside any marker. */
  kind: string;
  title: string;
  body: string;
  taskStrides?: TaskStridesBlockData;
}

export interface ReadableSystemReminder {
  title: string;
  sections: SystemReminderSection[];
  text: string;
}

const MARKER_BLOCK = /<codeterm-([a-z][a-z-]*)((?:\s+[a-z_]+="[^"]*")*)\s*>([\s\S]*?)<\/codeterm-\1>/g;
const MARKER_TOKEN = /<\/?codeterm-[a-z][a-z-]*(?:\s+[a-z_]+="[^"]*")*\s*>/;
const STRAY_MARKER = new RegExp(MARKER_TOKEN.source, "g");
const ATTRIBUTE = /([a-z_]+)="([^"]*)"/g;

const MARKER_TITLES = new Map<string, (attrs: Record<string, string>) => string>([
  ["worker-charter", (attrs) => attrs.task ? `Worker charter · ${attrs.task}` : "Worker charter"],
  ["task-strides", (attrs) => {
    const count = Number(attrs.count);
    if (Number.isInteger(count) && count > 0) return taskStridesLabel(count);
    return attrs.task ? `Task strides · ${attrs.task}` : "Task strides";
  }],
  ["orchestrator-heartbeat", (attrs) => attrs.plan ? `Plan heartbeat · ${attrs.plan}` : "Plan heartbeat"],
  ["state-reconciliation", (attrs) => attrs.plan ? `State reconciliation · ${attrs.plan}` : "State reconciliation"],
  ["role-identity", (attrs) => attrs.plan ? `Role · ${attrs.plan}` : "Role"],
]);

function attributes(source: string): Record<string, string> {
  return Object.fromEntries(Array.from(source.matchAll(ATTRIBUTE), (match) => [match[1], match[2]]));
}

function markerTitle(kind: string, attrs: Record<string, string>): string {
  const known = MARKER_TITLES.get(kind);
  if (known) return known(attrs);
  const words = kind.replaceAll("-", " ");
  return words.charAt(0).toUpperCase() + words.slice(1);
}

function plainSection(text: string): SystemReminderSection[] {
  const body = text.replace(STRAY_MARKER, "").trim();
  return body ? [{ kind: "", title: "", body }] : [];
}

/** Readable form of Domios-internal `<codeterm-*>` reminder markers; null when the text carries none. */
export function readableSystemReminder(text: string): ReadableSystemReminder | null {
  if (!MARKER_TOKEN.test(text)) return null;
  const sections: SystemReminderSection[] = [];
  let cursor = 0;
  for (const match of text.matchAll(MARKER_BLOCK)) {
    sections.push(...plainSection(text.slice(cursor, match.index)));
    const kind = match[1];
    const attrs = attributes(match[2]);
    const body = match[3].replace(STRAY_MARKER, "").trim();
    const taskStrides = kind === "task-strides"
      ? (() => {
        const declaredCount = Number(attrs.count);
        const count = Number.isInteger(declaredCount) && declaredCount > 0
          ? declaredCount
          : Math.max(1, countTaskStrideLines(body));
        return { ...(attrs.task ? { taskId: attrs.task } : {}), count, body };
      })()
      : undefined;
    sections.push({
      kind,
      title: markerTitle(kind, attrs),
      body,
      ...(taskStrides ? { taskStrides } : {}),
    });
    cursor = match.index + match[0].length;
  }
  sections.push(...plainSection(text.slice(cursor)));
  const titled = sections.find((section) => section.title);
  return {
    title: titled?.title ?? "Reminder",
    sections,
    text: sections.map((section) => section.body).filter(Boolean).join("\n\n"),
  };
}
