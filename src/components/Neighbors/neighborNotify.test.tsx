import { act } from "react";
import { createRoot } from "react-dom/client";
import { afterEach, expect, it, vi } from "vitest";
import type { NeighborConciergeClient, NeighborNotify } from "./neighborConciergeClient";
import { useNeighborNotify } from "./neighborNotify";

let unmount: (() => void) | undefined;
afterEach(() => { unmount?.(); unmount = undefined; });

it("keeps the newest notification choice when an older answer completes last", async () => {
  const answers: Array<{ resolve: (value: NeighborNotify) => void; reject: (failure: unknown) => void }> = [];
  const client = {
    onNotifyChanged: vi.fn(async () => () => {}),
    setNotify: vi.fn(() => new Promise<NeighborNotify>((resolve, reject) => answers.push({ resolve, reject }))),
  } as unknown as NeighborConciergeClient;
  const onError = vi.fn();
  let current: [NeighborNotify, (next: NeighborNotify) => void] | undefined;
  const Probe = () => { current = useNeighborNotify(client, "pair", "in_app", onError); return null; };
  const root = createRoot(document.createElement("div"));
  unmount = () => act(() => root.unmount());
  await act(async () => root.render(<Probe />));
  await act(async () => current![1]("off"));
  await act(async () => current![1]("in_app_and_bridges"));
  await act(async () => answers[1].resolve("in_app_and_bridges"));
  await act(async () => answers[0].reject(new Error("too late")));
  expect(current![0]).toBe("in_app_and_bridges");
  expect(onError).not.toHaveBeenCalled();
});
