import { act } from "react";
import { createRoot } from "react-dom/client";
import { describe, expect, it } from "vitest";
import type { CronJob } from "../../lib/types";
import { PageDataContext, type PageData } from "../../hooks/PageDataContext";
import { useWorkspaceCrons } from "../../hooks/useWorkspaceCrons";
import { useCronJobs } from "./useCronJobs";

const jobs = [
  { id: "a", workspace_id: "w1" },
  { id: "b", workspace_id: "w2" },
] as unknown as CronJob[];
const page = { crons: { jobs, ready: true, failed: false, loading: false, refresh: async () => {} } } as unknown as PageData;

describe("page cron lists", () => {
  it("keep their identity across renders until the page data changes", async () => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    const machine: Array<readonly CronJob[] | null> = [];
    const workspace: CronJob[][] = [];
    function Probe({ tick }: { tick: number }) {
      machine.push(useCronJobs("w1").jobs);
      workspace.push(useWorkspaceCrons("w1").jobs);
      return <span>{tick}</span>;
    }
    const host = document.createElement("div");
    const root = createRoot(host);
    for (const tick of [1, 2]) {
      await act(async () => root.render(<PageDataContext.Provider value={page}><Probe tick={tick} /></PageDataContext.Provider>));
    }
    expect(machine[0]).toEqual([jobs[0]]);
    expect(machine[1]).toBe(machine[0]);
    expect(workspace[1]).toBe(workspace[0]);
    await act(async () => root.unmount());
  });
});
