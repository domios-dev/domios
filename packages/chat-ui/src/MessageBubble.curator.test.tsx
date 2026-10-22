import React, { act } from "react";
import { createRoot } from "react-dom/client";
import { MessageBubble } from "./MessageBubble";
import { assert, createSuite, windowAny } from "./MessageBubble.providers.testkit";

const { test, run } = createSuite("Curator card");
test("system delivery hides instructions until expanded and preserves code without links", async () => {
  const host = windowAny.document.createElement("div");
  windowAny.document.body.appendChild(host);
  const root = createRoot(host);
  const instruction = "opaque instruction";
  const command = "codeterm plan curator grant pl_fixture tab_fixture --plan-skill opaque";
  try {
    await act(async () => root.render(<MessageBubble message={{ id: "curator-start", type: "user", timestamp: "2026-01-01T00:00:00Z",
      content: `${instruction}; grants: \`${command}\`.` }}
      origin={{ kind: "system", context: "curator:round_started:7" }} onOpenPlan={() => { throw new Error("command linked"); }} />));
    const toggle = host.querySelector<HTMLButtonElement>('button[aria-expanded="false"]');
    assert(toggle, "system card starts collapsed");
    assert(toggle.textContent?.includes("7"), "collapsed card retains the round identity");
    assert(!host.textContent?.includes(instruction), "agent instructions hidden");
    await act(async () => toggle.click());
    assert(host.textContent?.includes(instruction), "expansion exposes original instructions");
    assert(host.querySelector("code")?.textContent === command, "command preserved verbatim");
    assert(!host.querySelector("code button, code a"), "code contains no actionable reference");
    await act(async () => toggle.click());
    assert(!host.textContent?.includes(instruction), "instructions collapse again");
  } finally { await act(async () => root.unmount()); host.remove(); }
});
test("ordinary curator context never infers round metadata from its body", async () => {
  const host = windowAny.document.createElement("div");
  const root = createRoot(host);
  try {
    await act(async () => root.render(<MessageBubble message={{ id: "curator-other", type: "user", content: "Curator round 912 spawned" }} origin={{ kind: "system", context: "curator" }} />));
    assert(!host.querySelector("button")?.textContent?.includes("912"), "body cannot retitle the notice");
  } finally { await act(async () => root.unmount()); }
});
void run();
