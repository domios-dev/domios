import { expect, it } from "vitest";
import { canRetryChatStart, createChatStartFlight } from "./chatStartFlight";

it("deduplicates one subscription and refuses concurrent subscriptions per tab", async () => {
  const run = createChatStartFlight();
  let finish!: (value: string) => void;
  let starts = 0;
  const start = () => { starts++; return new Promise<string>(resolve => { finish = resolve; }); };
  const first = run("tab", "subscription", start);
  expect(run("tab", "subscription", start)).toBe(first);
  await expect(run("tab", "other", start)).rejects.toThrow("already running");
  expect(starts).toBe(1);
  finish("ready");
  expect(await first).toBe("ready");
  expect(await run("tab", "new", async () => "reattached")).toBe("reattached");
});

it("releases failed starts and only permits explicit or lifecycle retries", async () => {
  const run = createChatStartFlight();
  await expect(run("tab", "failed", async () => { throw new Error("launch failed"); })).rejects.toThrow("launch failed");
  for (const cause of ["listener_owned", "recover_gap", "pending_recover_gap", "resume_fallback", "resync:connected"]) {
    expect(canRetryChatStart("failed", cause)).toBe(false);
  }
  for (const cause of ["retry", "authority_changed", "history_settled", "resync:epoch_changed"]) {
    expect(canRetryChatStart("failed", cause)).toBe(true);
  }
  expect(await run("tab", "retry", async () => "ready")).toBe("ready");
});
