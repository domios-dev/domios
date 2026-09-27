import React, { act } from "react";
import { createRoot } from "react-dom/client";
import { describe, expect, it } from "vitest";
import { GroupCloseConfirmDialog } from "./GroupCloseConfirmDialog";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

describe("Team close confirmation inventory", () => {
  it("lists Team members, crons, and lanes", async () => {
    const host = document.createElement("div");
    document.body.appendChild(host);
    const root = createRoot(host);
    try {
      await act(async () => {
        root.render(
          <GroupCloseConfirmDialog
            mode="team"
            orchestratorTitle="API Team"
            members={[{ tabId: "worker-1", title: "API Worker", role: "Worker", roleProfile: null }]}
            crons={["Daily check"]}
            lanes={["API plan: Add route"]}
            onConfirm={() => {}}
            onCancel={() => {}}
          />,
        );
      });
      expect(host.textContent).toContain("API Worker (Worker)");
      expect(host.textContent).toContain("Crons (1)");
      expect(host.textContent).toContain("Daily check");
      expect(host.textContent).toContain("Lanes (1)");
      expect(host.textContent).toContain("API plan: Add route");
    } finally {
      await act(async () => root.unmount());
      host.remove();
    }
  });
});
