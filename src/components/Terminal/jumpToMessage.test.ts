import { afterEach, expect, it, vi } from "vitest";
import { jumpToMessage } from "./jumpToMessage";

afterEach(() => { document.body.innerHTML = ""; });

function surface() {
  const log = document.createElement("div");
  log.innerHTML = '<div data-message-id="a1" data-source-message-id="src-a1"></div><div data-message-id="u2"></div>';
  document.body.append(log);
  return log;
}

it("scrolls a loaded message into view and flashes it until its animation ends", () => {
  const log = surface();
  const row = log.querySelector<HTMLElement>('[data-message-id="a1"]')!;
  const scroll = vi.fn();
  row.scrollIntoView = scroll;
  expect(jumpToMessage(log, "src-a1")).toBe(true);
  expect(scroll).toHaveBeenCalledTimes(1);
  expect(row.hasAttribute("data-flash")).toBe(true);
  row.dispatchEvent(new Event("animationend"));
  expect(row.hasAttribute("data-flash")).toBe(false);
});

it("does nothing for a message that is not loaded", () => {
  const log = surface();
  expect(jumpToMessage(log, "gone")).toBe(false);
  expect(jumpToMessage(null, "a1")).toBe(false);
  expect(log.querySelector("[data-flash]")).toBeNull();
});
