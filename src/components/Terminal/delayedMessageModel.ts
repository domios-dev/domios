import type { DelayedMessage } from "./delayedSend";

export function canSendDelayedMessage(message: DelayedMessage | undefined): boolean {
  return message !== undefined && (message.state === "stashed" || message.state === "queued" || message.state === "failed");
}

export async function sendCurrentDelayedMessage(id: string, dependencies: {
  load: () => Promise<DelayedMessage[]>;
  send: (id: string) => Promise<void>;
}): Promise<void> {
  const current = (await dependencies.load()).find((message) => message.id === id);
  if (canSendDelayedMessage(current)) await dependencies.send(id);
}
