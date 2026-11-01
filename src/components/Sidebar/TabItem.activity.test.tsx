import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it } from "vitest";
import type { Tab } from "../../lib/types";
import { TabItem } from "./TabItem";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

it.each(["Polish the sidebar", undefined])("temporarily shows compaction in the description slot (description: %s)", async normalDescription => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const tab: Tab = {
    id: "swift", title: "Swift", tab_type: "Terminal", pty_id: null, cwd: null,
    agent_type: "fixture-agent", agent_session_id: "session", description: normalDescription,
  };
  try {
    for (const phase of ["work", "compacting", "work"] as const) {
      await act(async () => root.render(<TabItem
        tab={{ ...tab, agent_state: { state: "working", phase } }} hasAgent
        index={0} isActive={false} onClick={() => undefined} onClose={() => undefined}
        canClose workspaceId="fixture" />));
      const activation = host.querySelector<HTMLButtonElement>('button[aria-label="Open tab Swift"]');
      expect(activation).not.toBeNull();
      expect(activation?.querySelector('[data-testid="tab-title"]')?.parentElement?.textContent).toBe("Swift");
      const description = activation?.querySelector('[data-testid="tab-description-subtitle"]');
      expect(description?.textContent).toBe(phase === "compacting" ? "Compacting context…" : normalDescription);
      if (phase === "compacting") {
        expect(activation?.textContent).not.toContain("Polish the sidebar");
        expect(activation?.querySelector('[role="img"][aria-label="Compacting context…"]')).not.toBeNull();
        expect(host.querySelector('[role="status"]')?.textContent).toContain("Compacting context…");
      } else {
        expect(activation?.textContent).not.toContain("Compacting context…");
      }
    }
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});
