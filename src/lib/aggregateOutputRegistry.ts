import type { Unsubscribe } from "./transportClient";

export function createAggregateOutputRegistry(connect: () => Unsubscribe) {
  let owned = false;
  let active = false;
  let close: Unsubscribe = () => {};
  const open = () => {
    if (!owned || active) return;
    active = true;
    close = connect();
  };
  return {
    attach() { owned = true; open(); },
    ended() { active = false; },
    recover: open,
    replace(next: () => Unsubscribe) {
      active = false;
      close();
      connect = next;
      open();
    },
    dispose() { owned = false; active = false; close(); close = () => {}; },
  };
}
