import { mock } from "node:test";

export async function withChatToolClock(run: (timers: typeof mock.timers) => Promise<void>) {
  mock.timers.enable({ apis: ["setTimeout", "Date"], now: Date.now() });
  const schedule = mock.method(window, "setTimeout", globalThis.setTimeout);
  const cancel = mock.method(window, "clearTimeout", globalThis.clearTimeout);
  try {
    await run(mock.timers);
  } finally {
    schedule.mock.restore();
    cancel.mock.restore();
    mock.timers.reset();
  }
}
