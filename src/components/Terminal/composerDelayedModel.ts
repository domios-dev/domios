import type { DelayedMessage } from "./delayedSend";
import { isSystemDelayedMessage } from "./systemHeartbeatProjection";

export function composerDelayedModel(list: readonly DelayedMessage[], nowMs: number) {
  const chips = list.filter(message => !isSystemDelayedMessage(message)
    && (message.state === "stashed" || message.state === "failed"
      || (message.state === "queued" && message.scheduledAtMs > nowMs)))
    .sort((a, b) => Number(a.state === "stashed") - Number(b.state === "stashed")
      || a.scheduledAtMs - b.scheduledAtMs || a.id.localeCompare(b.id));
  return { chips, count: chips.length, alert: chips.some(message => message.state === "failed") };
}
