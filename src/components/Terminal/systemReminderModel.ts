import { readableSystemReminder, SYSTEM_ORIGIN_LABEL, type SystemReminderSection } from "@codeterm/chat-core";
import type { DelayedMessage, DeliveryTickView } from "./delayedSend";
import { formatCadence, presentTick } from "./systemHeartbeatProjection";

export type SystemReminderRole = "worker" | "orchestrator";

export type NextEligibility =
  | { kind: "at"; atMs: number }
  | { kind: "due" }
  | { kind: "unknown" };

export interface SystemReminderView {
  role: SystemReminderRole;
  planId: string | null;
  label: string;
  title: string;
  meta: string;
  accessibleLabel: string;
  why: string | null;
  cadence: string;
  condition: string;
  next: NextEligibility;
  nextText: string;
  lastSent: string | null;
  lastHeld: string | null;
  sections: SystemReminderSection[];
}

/** Heartbeat ids are `system:heartbeat:worker:<tab>` or `system:heartbeat:<plan>:<tab>`. */
export function systemReminderScope(id: string): { role: SystemReminderRole; planId: string | null } {
  const [, , scope] = id.split(":");
  if (!scope || scope === "worker") return { role: "worker", planId: null };
  return { role: "orchestrator", planId: scope };
}

/**
 * The scheduler's next due time is `deadline + cadence`, and every deadline is a scheduler check that left a tick.
 * Only a tick at exactly that anchor proves the projected time; without one the time is a placeholder.
 */
export function nextEligibility(message: Pick<DelayedMessage, "scheduledAtMs" | "recurrence" | "ticks">, nowMs: number): NextEligibility {
  const everyMs = message.recurrence?.everyMs ?? 0;
  const anchor = message.scheduledAtMs - everyMs;
  if (everyMs <= 0 || !(message.ticks ?? []).some((tick) => tick.atMs === anchor)) return { kind: "unknown" };
  return message.scheduledAtMs > nowMs ? { kind: "at", atMs: message.scheduledAtMs } : { kind: "due" };
}

function latest(ticks: readonly DeliveryTickView[], keep: (tick: DeliveryTickView) => boolean): DeliveryTickView | null {
  return ticks.reduce<DeliveryTickView | null>((found, tick) => keep(tick) && (!found || tick.atMs >= found.atMs) ? tick : found, null);
}

function readableSections(text: string): SystemReminderSection[] {
  const readable = readableSystemReminder(text);
  if (readable) return readable.sections;
  const body = text.trim();
  return body ? [{ kind: "", title: "", body }] : [];
}

export function presentSystemReminder(message: DelayedMessage, nowMs: number, clock: (ms: number) => string): SystemReminderView {
  const { role, planId } = systemReminderScope(message.id);
  const cadence = formatCadence(message.recurrence?.everyMs ?? 0);
  const title = role === "worker" ? "Worker check-in" : "Plan heartbeat";
  const condition = role === "worker"
    ? `Checked ${cadence} while this tab has a task in progress; a reminder with unchanged content waits out a cooldown before it repeats.`
    : `Checked ${cadence} while the plan has open work; the plan gate holds it when the team is healthy or nothing changed.`;
  const next = nextEligibility(message, nowMs);
  const nextShort = next.kind === "at" ? `next ${clock(next.atMs)}` : next.kind === "due" ? "due now" : cadence;
  const nextText = next.kind === "at"
    ? `Next eligible at ${clock(next.atMs)}; it sends then only if its condition holds.`
    : next.kind === "due"
      ? "Eligible now; it sends on the scheduler's next check if its condition holds."
      : `The scheduler's recent checks do not establish a next time; it follows the ${cadence} cadence.`;
  const ticks = message.ticks ?? [];
  const sent = latest(ticks, (tick) => tick.outcome === "sent");
  const held = latest(ticks, (tick) => tick.outcome !== "sent" && tick.spinalReason !== "not_due");
  return {
    role,
    planId,
    label: SYSTEM_ORIGIN_LABEL,
    title,
    meta: `${title} · ${nextShort}`,
    accessibleLabel: `${SYSTEM_ORIGIN_LABEL} reminder: ${title}, ${cadence}, ${next.kind === "unknown" ? "next time not established" : nextShort}`,
    why: message.reason?.trim() || null,
    cadence,
    condition,
    next,
    nextText,
    lastSent: sent ? clock(sent.atMs) : null,
    lastHeld: held && (!sent || held.atMs > sent.atMs) ? `${clock(held.atMs)} · ${presentTick(held).reason}` : null,
    sections: readableSections(message.text),
  };
}
