import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, expect, test, vi } from "vitest";
import type { FrontendUpdateOffer } from "../lib/frontendUpdate/runtime";

const store = vi.hoisted(() => ({
  offer: null as FrontendUpdateOffer | null,
  listeners: new Set<() => void>(),
  accept: vi.fn(),
}));

vi.mock("../lib/frontendUpdate/browser", () => ({
  frontendUpdateOffer: () => store.offer,
  subscribeFrontendUpdateOffer: (listener: () => void) => { store.listeners.add(listener); return () => store.listeners.delete(listener); },
  acceptFrontendUpdate: store.accept,
}));

import { FrontendUpdateNotice } from "./FrontendUpdateNotice";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
let container: HTMLDivElement;
let root: Root;

beforeEach(() => {
  container = document.createElement("div");
  document.body.append(container);
  root = createRoot(container);
});

afterEach(() => {
  act(() => root.unmount());
  container.remove();
  store.offer = null;
  store.accept.mockReset();
});

test("offers one Update action while a frontend release waits, and none otherwise", () => {
  act(() => root.render(<FrontendUpdateNotice />));
  expect(container.querySelector('[role="status"]')).toBeNull();
  act(() => {
    store.offer = { releaseId: "frontend-v1.14.6", version: "1.14.6" };
    for (const listener of store.listeners) listener();
  });
  const buttons = [...container.querySelectorAll("button")];
  expect(buttons.map((button) => button.textContent)).toEqual(["Update"]);
  act(() => buttons[0].click());
  expect(store.accept).toHaveBeenCalledTimes(1);
});
