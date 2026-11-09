import { act } from "react";
import { createRoot } from "react-dom/client";
import { expect, it, vi } from "vitest";
import { attachment, FixtureTransport, fixtureGraph } from "../Automation/fixtures.testdata";
import type { Team } from "../../lib/types";
import { TeamJobsUnit } from "./TeamJobsUnit";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const team = { id: attachment.team_id, name: "Team", lifecycle: "active", created_at: 1, workspace_id: attachment.workspace_id } as Team;

it("holds the Team's Jobs section from the first commit when this page already read its graph", async () => {
  const adapter = Object.assign(new FixtureTransport(), { peek: () => fixtureGraph() });
  adapter.readOverride = () => new Promise(() => undefined);
  const host = document.createElement("div");
  const root = createRoot(host);
  await act(async () => root.render(<TeamJobsUnit team={team} plan={null} tabs={{}} managers={null} managerCount={0} workers={null} workerCount={0} workingWorkerCount={0}
    providers={[]} onOpenEffort={vi.fn()} sentinelScope={{ workspace_id: attachment.workspace_id, team_id: attachment.team_id }} onOpenSentinel={vi.fn()} sentinelAdapter={adapter} />));
  expect(adapter.reads).toBeGreaterThan(0);
  expect(host.querySelector('[data-testid="crons-section"]')).not.toBeNull();
  await act(async () => root.unmount());
});
