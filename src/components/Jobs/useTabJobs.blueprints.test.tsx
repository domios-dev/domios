import { act } from "react";
import { createRoot } from "react-dom/client";
import { describe, expect, it } from "vitest";
import { PageDataContext, type PageData } from "../../hooks/PageDataContext";
import capturedWatch from "../Automation/blueprintWatchCaptured.testdata.json";
import type { AutomationGraph, AutomationGraphAdapter, BlueprintSnapshot } from "../Automation/types";
import { useTabJobs } from "./useTabJobs";

const watch = capturedWatch as unknown as AutomationGraph;
const persisted = watch.blueprints![0];
const dryRun = { action: "create", dry_run: true, id: null, name: persisted.name };
const applied = { action: "create", dry_run: false, id: persisted.id, name: persisted.name };
const page = { crons: { jobs: [], ready: true, failed: false, loading: false, refresh: async () => {} } } as unknown as PageData;

async function withTabJobs(run: (probe: {
  publish: (graph: AutomationGraph) => Promise<void>;
  current: () => ReturnType<typeof useTabJobs>;
  host: HTMLElement;
}) => Promise<void>) {
  (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
  let graph: AutomationGraph = { ...watch, all_teams: true, blueprints: [], scenarios: [], nodes: [] };
  let changed: (() => void) | undefined;
  let jobs: ReturnType<typeof useTabJobs> | undefined;
  const adapter: AutomationGraphAdapter = {
    read: async () => graph,
    subscribe: async (_, listener) => { changed = listener; return () => { changed = undefined; }; },
    stop: async () => {},
    delete: async () => {},
  };
  function Probe() {
    jobs = useTabJobs("owner-watch", watch.workspace_id, adapter);
    return <output aria-label="Blueprint chips">{jobs.blueprints.length + jobs.chips.length}</output>;
  }
  const host = document.createElement("div");
  const root = createRoot(host);
  try {
    await act(async () => root.render(<PageDataContext.Provider value={page}><Probe /></PageDataContext.Provider>));
    await run({
      host,
      current: () => { expect(jobs).toBeDefined(); return jobs!; },
      publish: async (next) => {
        graph = { ...next, all_teams: true };
        expect(changed).toBeTypeOf("function");
        await act(async () => changed!());
      },
    });
  } finally {
    await act(async () => root.unmount());
  }
}

describe("composer persisted Blueprint chips", () => {
  it("ignores dry-run output and command echoes, then shows one persisted watch after apply", async () => {
    await withTabJobs(async ({ host, current, publish }) => {
      const transcript = document.createElement("pre");
      transcript.textContent = `domios blueprint apply watch.yaml --dry-run --json\n${JSON.stringify(dryRun)}`;
      host.append(transcript);
      expect(current().blueprints).toEqual([]);
      expect(current().chips).toEqual([]);
      transcript.textContent += `\ndomios blueprint apply watch.yaml --json\n${JSON.stringify(applied)}`;
      await publish(watch);
      expect(current().blueprints.map(({ blueprint }) => blueprint.id)).toEqual(["blueprint-watch"]);
      expect(current().chips).toEqual([]);
      expect(host.querySelector("output")?.textContent).toBe("1");
    });
  });

  it("rejects dry-run and id-less entries even if included in a graph response", async () => {
    await withTabJobs(async ({ current, publish }) => {
      const invalid = [
        { ...persisted, ...dryRun },
        { ...persisted, id: null },
        { ...persisted, id: "" },
        { ...persisted, id: "  " },
        { ...persisted, dry_run: true },
      ] as unknown as BlueprintSnapshot[];
      await publish({ ...watch, blueprints: invalid });
      expect(current().blueprints).toEqual([]);
      expect(current().chips).toEqual([]);
      await publish({ ...watch, blueprints: [...invalid, persisted] });
      expect(current().blueprints.map(({ blueprint }) => blueprint.id)).toEqual(["blueprint-watch"]);
      expect(current().chips).toEqual([]);
    });
  });

  it("deduplicates repeated snapshots and the scenario projection by persisted id", async () => {
    await withTabJobs(async ({ current, publish }) => {
      await publish({ ...watch, blueprints: [persisted, persisted], scenarios: [...watch.scenarios, ...watch.scenarios] });
      expect(current().blueprints).toHaveLength(1);
      expect(current().chips).toEqual([]);
      await publish(watch);
      expect(current().blueprints).toHaveLength(1);
      expect(current().chips).toEqual([]);
    });
  });

  it("removes a deleted Blueprint on the next snapshot, including its stale scenario projection", async () => {
    await withTabJobs(async ({ host, current, publish }) => {
      await publish(watch);
      expect(host.querySelector("output")?.textContent).toBe("1");
      await publish({ ...watch, blueprints: [] });
      expect(current().blueprints).toEqual([]);
      expect(current().chips).toEqual([]);
      expect(host.querySelector("output")?.textContent).toBe("0");
    });
  });

  it("retains one scenario chip for a bound tab that does not own the persisted graph", async () => {
    await withTabJobs(async ({ current, publish }) => {
      const foreign = { ...persisted, owner: { kind: "tab" as const, tab_id: "another-owner", team_id: watch.team_id } };
      await publish({ ...watch, blueprints: [foreign], scenarios: [...watch.scenarios, ...watch.scenarios] });
      expect(current().blueprints).toEqual([]);
      expect(current().chips.map(({ id }) => id)).toEqual(["blueprint-watch"]);
    });
  });

  it("keeps distinct persisted ids with the same name", async () => {
    await withTabJobs(async ({ current, publish }) => {
      await publish({ ...watch, blueprints: [persisted, { ...persisted, id: "another-watch" }] });
      expect(current().blueprints.map(({ blueprint }) => blueprint.id)).toEqual(["blueprint-watch", "another-watch"]);
      expect(current().chips).toEqual([]);
    });
  });
});
