import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it } from "vitest";
import { TeamUnit } from "./TeamUnit";
import { PlanViewShell } from "../Canvas/PlanViewShell";
import { FIXTURE_PLAN } from "../Canvas/cards/fixtures.testdata";
import { canvasFocusStore } from "../Canvas/canvasFocusStore";
import { openTeamPlanCanvas } from "../Canvas/workspacePlanEntry";
import { MuxStateStoreContext } from "../../store/multiplexer/context";
import { createMuxStateStore } from "../../store/multiplexer/selectorStore";
import type { ExtendedState } from "../../store/multiplexer/tree";

Object.assign(globalThis, { IS_REACT_ACT_ENVIRONMENT: true });

it.each([true, false])("opens Board over a remembered Plan view, active plan: %s", async (withPlan) => {
  localStorage.setItem("codeterm.planViewMode", "plan");
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const plan = withPlan ? { ...FIXTURE_PLAN, team_id: "team-board" } : null;
  const state: ExtendedState = {
    workspaces: [], tabs: {}, teams: [], active_workspace_id: null, focused_tab_id: null,
    notifications: [], tabStatuses: {}, toasts: [], activeTabIds: {}, generation: 0, pendingCloses: [],
  };
  const store = createMuxStateStore(() => state);
  const shell = () => <PlanViewShell mode="workspace" plans={plan ? [plan] : []} tabs={{}}
    teamId="team-board" workspaceTarget={canvasFocusStore.getWorkspaceTarget()} />;
  const view = () => <MuxStateStoreContext.Provider value={store}>
      <TeamUnit team={{ id: "team-board", name: "Board team", lifecycle: "active", created_at: 0, workspace_id: "ws" }}
        plan={plan} tabs={{}} managers={null} managerCount={0} workers={null} workerCount={0}
        workingWorkerCount={0} providers={[]} onOpenEffort={openTeamPlanCanvas} />
      {shell()}
    </MuxStateStoreContext.Provider>;
  try {
    await act(async () => root.render(view()));
    expect(host.querySelector('[role="tab"][aria-selected="true"]')?.textContent).toBe("Plan");
    const button = host.querySelector<HTMLButtonElement>(`button[aria-label="Open the board for ${plan?.title ?? "Board team"}"]`);
    expect(button).not.toBeNull();
    await act(async () => button!.click());
    expect(canvasFocusStore.getWorkspaceTarget()).toMatchObject({ teamId: "team-board", jump: { view: "board" } });
    await act(async () => root.render(view()));
    expect(host.querySelector('[role="tab"][aria-selected="true"]')?.textContent).toBe("Board");
  } finally {
    await act(async () => root.unmount());
    host.remove();
    canvasFocusStore.closeWorkspace();
    localStorage.removeItem("codeterm.planViewMode");
  }
});

it("names local and remote Team cards with the machine shown after the title", async () => {
  const host = document.createElement("div");
  document.body.append(host);
  const root = createRoot(host);
  const props = { plan: null, tabs: {}, managers: null, managerCount: 0, workers: null,
    workerCount: 0, workingWorkerCount: 0, providers: [], onOpenEffort: () => {} };
  const team = { id: "remote-team", name: "Remote research", lifecycle: "active" as const, created_at: 0, workspace_id: "ws" };
  try {
    await act(async () => root.render(<>
      <TeamUnit {...props} team={{ ...team, id: "local-team", name: "Local research" }} />
      <TeamUnit {...props} team={{ ...team, remote_peer_id: "remote-peer", remote_peer_name: "Desk [dev-qa]" }} />
    </>));
    const local = host.querySelector('section[aria-label="Local research"]')!;
    const remote = host.querySelector('section[aria-label="Remote research, Desk"]')!;
    expect(local.querySelector(".sidebar-team-machine-pill")).toBeNull();
    const title = remote.querySelector('[data-testid="team-name"]')!;
    const machine = remote.querySelector<HTMLElement>('[title="Desk"]')!;
    expect(machine.textContent).toBe("Desk");
    expect(title.nextElementSibling).toBe(machine);
    expect(machine.style.getPropertyValue("--ct-plan-tone")).toBe("");
    const glyph = machine.querySelector<HTMLElement>('[aria-hidden="true"]')!;
    expect(glyph.style.color).toBe("var(--ct-accent)");
    expect(remote.innerHTML).not.toMatch(/ct-machine-/);
  } finally {
    await act(async () => root.unmount());
    host.remove();
  }
});
