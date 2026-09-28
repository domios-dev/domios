import { act } from "react";
import { createRoot } from "react-dom/client";
import { describe, expect, it } from "vitest";
import { WorkspaceCloseConfirmDialog } from "./WorkspaceCloseConfirmDialog";
import type { TeamCloseResource } from "./teamCloseResources";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const team: TeamCloseResource = {
  id: "team-1",
  name: "API Team",
  members: [{ tabId: "worker-1", title: "API Worker", role: "Worker", roleProfile: null }],
  crons: ["Daily check"],
  lanes: ["API plan: Add route"],
};

describe("workspace close confirmation", () => {
  it("lists Team members, crons, and lanes", async () => {
    const host = document.createElement("div");
    document.body.appendChild(host);
    const root = createRoot(host);
    try {
      await act(async () => {
        root.render(
          <WorkspaceCloseConfirmDialog
            workspaceName="Project"
            agentTitles={[]}
            teams={[team]}
            onConfirm={() => {}}
            onCancel={() => {}}
          />,
        );
      });
      expect(host.textContent).toContain("Team members (1)");
      expect(host.textContent).toContain("API Worker (Worker)");
      expect(host.textContent).toContain("Daily check");
      expect(host.textContent).toContain("API plan: Add route");
    } finally {
      await act(async () => root.unmount());
      host.remove();
    }
  });
});
