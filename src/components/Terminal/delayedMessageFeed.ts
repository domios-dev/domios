import type { DelayedMessage } from "./delayedSend";

interface FeedDependencies {
  load: (tabId: string) => Promise<DelayedMessage[]>;
  publish: (messages: DelayedMessage[]) => void;
  subscribe: (changed: (tabId?: string) => void) => Promise<() => void>;
}

export function createDelayedMessageFeed(tabId: string, dependencies: FeedDependencies) {
  let active = true;
  let generation = 0;
  let unlisten: (() => void) | undefined;
  async function refresh() {
    if (!active) return;
    const request = ++generation;
    let messages: DelayedMessage[];
    try {
      messages = await dependencies.load(tabId);
    } catch {
      messages = [];
    }
    if (active && request === generation) dependencies.publish(messages);
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
      dependencies.publish([]);
      return refresh();
    },
    dispose() {
      active = false;
      unlisten?.();
    },
  };
}
