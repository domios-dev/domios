import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it } from "vitest";
import { TeamUnit } from "./TeamUnit";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

it("renders roleless agents in an accessible Team agent group", async () => {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  try {
    await act(async () => {
      root.render(<TeamUnit
        team={{ id: "adoption", name: "Adoption", lifecycle: "active", created_at: 0, workspace_id: "ws" }}
        plan={null} tabs={{}} managers={null} managerCount={0}
        workers={null} workerCount={0} workingWorkerCount={0}
        agents={<button>Adopted agent</button>} agentCount={1}
        providers={[]} onOpenEffort={() => {}}
      />);
    });
    const group = host.querySelector('[role="group"][aria-label="Adoption agents"]');
    expect(group?.querySelector("button")?.textContent).toBe("Adopted agent");
    expect(host.querySelector('[role="group"][aria-label="Adoption utility shells"]')).toBeNull();
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});
