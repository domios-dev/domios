// Run: npx tsx packages/chat-ui/src/codeterm/ReminderCard.test.tsx
import { Window } from "happy-dom";
import React, { act } from "react";
import { createRoot } from "react-dom/client";

import { ReminderCard } from "./ReminderCard";
import { relativeTime } from "./relativeTime";

const windowInstance = new Window({ url: "http://localhost/reminder-card-test" });
const windowAny = windowInstance as unknown as Window & typeof globalThis;
(globalThis as any).window = windowAny;
(globalThis as any).document = windowAny.document;
(globalThis as any).HTMLElement = windowAny.HTMLElement;
(globalThis as any).Node = windowAny.Node;
(globalThis as any).IS_REACT_ACT_ENVIRONMENT = true;

let passed = 0;
let failed = 0;
function assert(cond: unknown, name: string) {
  if (cond) {
    passed++;
    console.log(`  ✓ ${name}`);
  } else {
    failed++;
    console.error(`  ✗ ${name}`);
  }
}

async function renderCard(props: Parameters<typeof ReminderCard>[0]): Promise<HTMLElement> {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(<ReminderCard {...props} />);
  });
  return host;
}

// relativeTime humanizer
{
  const now = 1_000_000_000_000;
  assert(relativeTime(now - 5 * 60_000, now) === "5m ago", "relativeTime past minutes");
  assert(relativeTime(now + 2 * 3_600_000, now) === "in 2h", "relativeTime future hours");
  assert(relativeTime(now, now) === "just now", "relativeTime just now");
  assert(relativeTime(undefined) === null, "relativeTime null for missing");
  assert(relativeTime("not-a-number") === null, "relativeTime null for garbage");
}

await (async () => {
  const setAt = String(Date.now() - 10 * 60_000);
  const fireAt = String(Date.now());
  const host = await renderCard({
    whoSet: "Main",
    model: "opus",
    setAt,
    fireAt,
    id: "dmsg-7",
    body: "water the plants",
  });
  const text = host.textContent ?? "";
  assert(host.querySelector('[data-testid="chat-reminder-card"]') != null, "renders reminder card container");
  assert(text.includes("Reminder"), "renders Reminder title");
  assert(text.includes("from Main"), "renders who-set");
  assert(text.includes("water the plants"), "renders reminder body");
  assert(text.includes("set ") && text.includes("fired "), "renders set/fired subline");
  assert(host.querySelector('[data-reminder-id="dmsg-7"]') != null, "carries reminder id");
})();

await (async () => {
  const host = await renderCard({ body: "bare reminder" });
  const text = host.textContent ?? "";
  assert(text.includes("Reminder"), "renders title without creator metadata");
  assert(!text.includes("from "), "omits who-set when unknown");
  assert(text.includes("bare reminder"), "renders body without creator metadata");
})();

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
