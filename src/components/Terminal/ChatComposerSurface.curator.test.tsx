import "./ChatComposer.composition.harness";
import assert from "node:assert/strict";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { ChatComposerSurface } from "./ChatComposerSurface";

async function main() {
  const host = document.createElement("div");
  const root = createRoot(host);
  let mounted = false;
  function Composer() { mounted = true; return <textarea aria-label="Message" />; }
  try {
    await act(async () => root.render(
      <ChatComposerSurface view={{ kind: "none" }} tab={{ role_profile: "curator" }}>
        <Composer />
      </ChatComposerSurface>,
    ));
    assert.equal(host.querySelector('[role="status"]')?.textContent, "Curator round, read-only");
    assert.equal(host.querySelector("textarea, input, button, [contenteditable]"), null);
    assert.equal(mounted, false);
    await act(async () => root.render(
      <ChatComposerSurface view={{ kind: "none" }} tab={{ role_profile: "planner" }}>
        <Composer />
      </ChatComposerSurface>,
    ));
    assert(host.querySelector('textarea[aria-label="Message"]'));
    console.log("Curator composer: 4 assertions passed");
  } finally {
    await act(async () => root.unmount());
  }
}
main().catch(error => { console.error(error); process.exitCode = 1; });
