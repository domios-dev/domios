import { expect, it } from "vitest";
import { createConsumerFlight } from "./consumerFlight";

it("shares a key until its final consumer leaves and permits a new flight after abort", async () => {
  const flights = createConsumerFlight<string>();
  const signals: AbortSignal[] = [];
  const start = (signal: AbortSignal) => {
    signals.push(signal);
    return new Promise<string>((_, reject) => signal.addEventListener("abort", () => reject(signal.reason), { once: true }));
  };
  const first = new AbortController();
  const second = new AbortController();
  const a = flights.run("repo", start, first.signal);
  const b = flights.run("repo", start, second.signal);
  const settled = Promise.allSettled([a, b]);
  await Promise.resolve();
  expect(signals).toHaveLength(1);
  first.abort("left");
  expect(signals[0].aborted).toBe(false);
  second.abort("left");
  expect(signals[0].aborted).toBe(true);
  expect((await settled).every(result => result.status === "rejected")).toBe(true);
  expect(await flights.run("repo", async () => "fresh")).toBe("fresh");
});

it("does not start an already released consumer", async () => {
  const controller = new AbortController();
  controller.abort("gone");
  await expect(createConsumerFlight().run("key", async () => { throw new Error("started"); }, controller.signal)).rejects.toBe("gone");
});
