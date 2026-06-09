import type { CodetermInvocation } from "./recognize";

export interface CardStatus {
  ok: boolean;
  label: string;
}
export interface RenderedCard {
  icon: string;
  title: string;
  detail?: string;
  status?: CardStatus;
}

function pane(inv: CodetermInvocation): string {
  const p = inv.flags.pane;
  return typeof p === "string" ? p : "current pane";
}

function deliveredStatus(result?: string): CardStatus | undefined {
  if (!result) return undefined;
  if (/\bOK\b|\(sent\)|delivered/i.test(result)) return { ok: true, label: "delivered" };
  if (/error|fail|not found/i.test(result)) return { ok: false, label: "failed" };
  return undefined;
}

type TemplateFn = (inv: CodetermInvocation, result?: string) => RenderedCard;

const TEMPLATES: Record<string, TemplateFn> = {
  "send-text": (inv, result) => ({
    icon: "→",
    title: `Sent to pane ${pane(inv)}`,
    detail: inv.positionals[0],
    status: deliveredStatus(result),
  }),
  "agent spawn": (inv) => ({
    icon: "🤖",
    title: `Spawned ${inv.positionals[0] ?? "claude"}`,
    detail: typeof inv.flags.task === "string" ? inv.flags.task : undefined,
  }),
  "agent report": (inv) => ({ icon: "📨", title: "Reported to parent", detail: inv.positionals[0] }),
  "agent providers": () => ({ icon: "🤖", title: "Listed agent providers" }),
  "mem save": (inv) => ({
    icon: "🧠",
    title: `Saved memory${typeof inv.flags.title === "string" ? `: ${inv.flags.title}` : ""}`,
    detail: inv.positionals[0],
  }),
  "mem search": (inv) => ({ icon: "🔍", title: "Searched mem", detail: inv.positionals[0] }),
  "mem get": (inv) => ({ icon: "📖", title: `Recalled ${inv.positionals[0] ?? ""}` }),
  "workspace create": (inv) => ({ icon: "🪟", title: `Created workspace ${inv.positionals[0] ?? ""}` }),
  "pane focus": (inv) => ({ icon: "→", title: `Focused pane ${inv.positionals[0] ?? pane(inv)}` }),
  "mesh send": (inv) => ({ icon: "🛰", title: `Mesh → ${inv.positionals[0] ?? ""}`, detail: inv.positionals[1] }),
  "cron create": (inv) => ({ icon: "⏰", title: `Scheduled ${inv.positionals[0] ?? "job"}` }),
  "keepawake start": () => ({ icon: "☕", title: "Keep-awake started" }),
};

/** Pick the longest matching template, else a branded fallback. */
export function renderTemplate(inv: CodetermInvocation, result?: string): RenderedCard {
  const two = inv.path.slice(0, 2).join(" ");
  const one = inv.path[0];
  const fn = TEMPLATES[two] ?? TEMPLATES[one];
  if (fn) return fn(inv, result);
  return {
    icon: "⬡",
    title: inv.path.join(" "),
    detail: inv.positionals.join(" ") || undefined,
    status: deliveredStatus(result),
  };
}
