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
  blueprints: ["Daily deploy"],
  lanes: ["API plan: Add route"],
};

describe("workspace close confirmation", () => {
  it("lists Team members, crons, lanes, and incomplete-close warnings", async () => {
    const host = document.createElement("div");
    document.body.appendChild(host);
    const root = createRoot(host);
    try {
      await act(async () => {
        root.render(
          <WorkspaceCloseConfirmDialog
            workspaceName="Project"
            agentTitles={[]}
            blueprints={["Tab owned pipeline"]}
            teams={[team]}
            warnings={["A remote Team member remains on Neighbor.", "A dirty lane will be retained."]}
            onConfirm={() => {}}
            onCancel={() => {}}
          />,
        );
      });
      expect(host.textContent).toContain("Team members (1)");
      expect(host.textContent).toContain("API Worker (Worker)");
      expect(host.textContent).toContain("Daily check");
      expect(host.textContent).toContain("Daily deploy");
      expect(host.textContent).toContain("Tab owned pipeline");
      expect(host.textContent).toContain("API plan: Add route");
      expect(host.textContent).toContain("Warnings (2)");
      expect(host.textContent).toContain("A remote Team member remains on Neighbor.");
      expect(host.textContent).toContain("A dirty lane will be retained.");
    } finally {
      await act(async () => root.unmount());
      host.remove();
    }
  });

  it("names both the workspace and machine for a remote close", async () => {
    const host = document.createElement("div");
    document.body.appendChild(host);
    const root = createRoot(host);
    try {
      await act(async () => {
        root.render(
          <WorkspaceCloseConfirmDialog
            workspaceName="Project"
            machineName="Desk · 1234abcd"
            agentTitles={[]}
            teams={[]}
            onConfirm={() => {}}
            onCancel={() => {}}
          />,
        );
      });
      expect(host.querySelector('[role="dialog"][aria-label="Close Project on Desk · 1234abcd"]')).not.toBeNull();
      expect(host.textContent).toContain("Project · Desk · 1234abcd");
    } finally {
      await act(async () => root.unmount());
      host.remove();
    }
  });
});
