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

/** Resolve a pane id/short-id to a human tab name. Returns undefined when unknown. */
export type PaneNameResolver = (paneRef: string) => string | undefined;

function pane(inv: CodetermInvocation, resolve?: PaneNameResolver): string {
  const p = inv.flags.pane;
  if (typeof p !== "string") return "current pane";
  return resolve?.(p) ?? p;
}

function sentTitle(inv: CodetermInvocation, resolve?: PaneNameResolver): string {
  const p = inv.flags.pane;
  if (typeof p !== "string") return "Sent to current pane";
  const name = resolve?.(p);
  return name ? `Sent to ${name}` : `Sent to pane ${p}`;
}

function deliveredStatus(result?: string): CardStatus | undefined {
  if (!result) return undefined;
  if (/\bOK\b|\(sent\)|delivered/i.test(result)) return { ok: true, label: "delivered" };
  if (/error|fail|not found/i.test(result)) return { ok: false, label: "failed" };
  return undefined;
}

type TemplateFn = (inv: CodetermInvocation, result?: string, resolve?: PaneNameResolver) => RenderedCard;

const sendTextTemplate: TemplateFn = (inv, result, resolve) => {
  const mesh = typeof inv.flags.mesh === "string" ? inv.flags.mesh : undefined;
  return {
    icon: mesh ? "🛰" : "→",
    title: mesh ? `${sentTitle(inv, resolve)} @ ${mesh}` : sentTitle(inv, resolve),
    detail: inv.positionals[0],
    status: deliveredStatus(result),
  };
};

const TEMPLATES: Record<string, TemplateFn> = {
  send: sendTextTemplate,
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
  "pane focus": (inv, _result, resolve) => {
    const ref = inv.positionals[0] ?? (typeof inv.flags.pane === "string" ? inv.flags.pane : undefined);
    const name = ref ? resolve?.(ref) : undefined;
    return { icon: "→", title: `Focused ${name ?? `pane ${ref ?? pane(inv, resolve)}`}` };
  },
  "cron create": (inv) => ({ icon: "⏰", title: `Scheduled ${inv.positionals[0] ?? "job"}` }),
  "keepawake start": () => ({ icon: "☕", title: "Keep-awake started" }),
};

/** Pick the longest matching template, else a branded fallback. */
export function renderTemplate(
  inv: CodetermInvocation,
  result?: string,
  resolve?: PaneNameResolver,
): RenderedCard {
  const two = inv.path.slice(0, 2).join(" ");
  const one = inv.path[0];
  const fn = TEMPLATES[two] ?? TEMPLATES[one];
  if (fn) return fn(inv, result, resolve);
  return {
    icon: "⬡",
    title: inv.path.join(" "),
    detail: inv.positionals.join(" ") || undefined,
    status: deliveredStatus(result),
  };
}
