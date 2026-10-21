import type { DelayedMessage } from "./delayedSend";

interface FeedDependencies {
  load: (tabId: string) => Promise<DelayedMessage[]>;
  publish: (messages: DelayedMessage[]) => void;
  subscribe: (changed: (tabId?: string) => void) => Promise<() => void>;
}

function sameMessage(a: DelayedMessage, b: DelayedMessage): boolean {
  const { recurrence: ar, ticks: at = [], ...af } = a;
  const { recurrence: br, ticks: bt = [], ...bf } = b;
  const keys = Object.keys(af) as (keyof typeof af)[];
  return keys.length === Object.keys(bf).length && keys.every(key => af[key] === bf[key])
    && ar?.everyMs === br?.everyMs && ar?.untilMs === br?.untilMs && ar?.count === br?.count
    && at.length === bt.length && at.every((tick, index) => {
      const other = bt[index];
      return tick.atMs === other.atMs && tick.outcome === other.outcome
        && tick.spinalVerdict === other.spinalVerdict && tick.spinalReason === other.spinalReason;
    });
}

export function createDelayedMessageFeed(tabId: string, dependencies: FeedDependencies) {
  let active = true;
  let generation = 0;
  let unlisten: (() => void) | undefined;
  let previous: Map<string, DelayedMessage> | undefined;
  async function refresh() {
    if (!active) return;
    const request = ++generation;
    let messages: DelayedMessage[];
    try {
      messages = await dependencies.load(tabId);
    } catch {
      return;
    }
    if (!active || request !== generation) return;
    if (previous?.size === messages.length && messages.every(message => {
      const old = previous?.get(message.id);
      return old !== undefined && sameMessage(old, message);
    })) return;
    previous = new Map(messages.map(message => [message.id, message]));
    dependencies.publish(messages);
  }
  const ready = dependencies.subscribe((changedTabId) => {
    if (!changedTabId || changedTabId === tabId) void refresh();
  }).then((stop) => {
    if (active) unlisten = stop;
    else stop();
  }).catch(() => {}).then(refresh);
  return {
    ready,
    refresh,
    wake() {
      if (!active) return Promise.resolve();
      return refresh();
    },
    dispose() {
      active = false;
      unlisten?.();
    },
  };
}
