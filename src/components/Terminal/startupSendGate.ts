export const STARTUP_SEND_GRACE_MS = 5_000;

export function createStartupSendGate(now: () => number) {
  let launch: string | null = null;
  let since: number | null = null;
  return {
    observe(key: string, starting: boolean) {
      if (launch !== key) {
        launch = key;
        since = starting ? now() : null;
      } else if (!starting) {
        since = null;
      } else if (since === null) {
        since = now();
      }
    },
    remainingMs() {
      return since === null ? 0 : Math.max(0, STARTUP_SEND_GRACE_MS - (now() - since));
    },
    isStarting() { return since !== null; },
  };
}
