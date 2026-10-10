import { act, useState } from "react";
import { createRoot } from "react-dom/client";
import { expect, it } from "vitest";
import { clearChatReveal, currentChatReveal, requestChatMessageReveal } from "../../lib/chatMessageReveal";
import { useChatMessageReveal } from "./useChatMessageReveal";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
it("waits for the requested loaded turn's window before settling its reveal", async () => {
  const host = document.createElement("div"); document.body.append(host);
  const root = createRoot(host);
  const seen: string[] = [];
  function Harness() {
    const [prepared, setPrepared] = useState(false);
    useChatMessageReveal({ tabId: "tab", ready: true, contentVersion: prepared,
      revealMessage: (id) => { if (prepared) seen.push(id); return prepared; },
      prepareMessage: () => { setPrepared(true); return true; } });
    return null;
  }
  try {
    await act(async () => root.render(<Harness />));
    await act(async () => { requestChatMessageReveal("tab", "source-turn"); });
    expect(seen).toEqual(["source-turn"]);
    expect(currentChatReveal()?.outcome).toBe("revealed");
  } finally { await act(async () => root.unmount()); host.remove(); clearChatReveal(); }
});
