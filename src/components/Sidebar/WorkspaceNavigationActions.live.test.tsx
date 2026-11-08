import { act, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import { PageDataContext, type PageData } from "../../hooks/PageDataContext";
import type { CronJob } from "../../lib/types";
import type { AutomationGraph } from "../Automation/types";
import { LiveWorkspaceNavigationActions } from "./WorkspaceNavigationActions";

const adapter = vi.hoisted(() => ({
  read: vi.fn(),
  subscribe: vi.fn(async () => () => undefined),
  stop: vi.fn(),
  delete: vi.fn(),
  peek: vi.fn((): unknown => null),
}));
vi.mock("../Automation/hostAdapter", () => ({ hostAutomationAdapter: adapter }));

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

const graph = (workspace_id: string) => ({ workspace_id, team_id: null, all_teams: true, revision: "r", scenarios: [], nodes: [], edges: [] }) as unknown as AutomationGraph;
const cron = (id: string, workspace_id: string) => ({ id, name: id, workspace_id, team_id: null, enabled: true, created_at: 1 }) as unknown as CronJob;
const page = (jobs: CronJob[], ready = true) => ({ crons: { jobs, ready, failed: false, loading: false, refresh: async () => undefined } }) as unknown as PageData;
const never = () => new Promise<AutomationGraph>(() => undefined);

let mounted: { root: Root; host: HTMLElement } | null = null;
async function render(node: ReactNode) {
  if (!mounted) {
    const host = document.createElement("div");
    document.body.append(host);
    mounted = { root: createRoot(host), host };
  }
  await act(async () => mounted!.root.render(node));
}
async function unmount() {
  if (!mounted) return;
  await act(async () => mounted!.root.unmount());
  mounted.host.remove();
  mounted = null;
}
const actions = (workspaceId: string, data: PageData) => <PageDataContext.Provider value={data}>
  <LiveWorkspaceNavigationActions scope={{ workspaceId, teamId: null }} workspaceId={workspaceId} workspaceName={workspaceId} onOpenMap={() => undefined} onOpenJobs={() => undefined} />
</PageDataContext.Provider>;
const jobsButton = () => mounted!.host.querySelector<HTMLButtonElement>('button[title^="Open jobs"]')!;

afterEach(async () => {
  await unmount();
  adapter.read.mockReset();
  adapter.peek.mockReset();
  adapter.peek.mockReturnValue(null);
});

describe("the workspace Jobs indicator", () => {
  it("keeps the last known count through a remount whose read has not returned", async () => {
    const data = page([cron("a", "kept"), cron("b", "kept")]);
    adapter.read.mockResolvedValueOnce(graph("kept"));
    await render(actions("kept", data));
    expect(jobsButton().getAttribute("aria-label")).toBe("Jobs, 2");
    await unmount();
    adapter.read.mockImplementation(never);
    await render(actions("kept", data));
    expect(jobsButton().getAttribute("aria-label")).toBe("Jobs, 2");
    expect(jobsButton().textContent).toContain("2");
  });

  it("keeps the count while a source reloads and takes the fresh count when it returns", async () => {
    adapter.read.mockResolvedValue(graph("reload"));
    await render(actions("reload", page([cron("a", "reload")])));
    expect(jobsButton().getAttribute("aria-label")).toBe("Jobs, 1");
    await render(actions("reload", page([], false)));
    expect(jobsButton().getAttribute("aria-label")).toBe("Jobs, 1");
    await render(actions("reload", page([cron("a", "reload"), cron("b", "reload"), cron("c", "reload")])));
    expect(jobsButton().getAttribute("aria-label")).toBe("Jobs, 3");
    await render(actions("reload", page([])));
    expect(jobsButton().getAttribute("aria-label")).toBe("Jobs");
    expect(jobsButton().textContent).toBe("Jobs");
  });

  it("never shows another workspace's count", async () => {
    adapter.read.mockResolvedValueOnce(graph("first"));
    await render(actions("first", page([cron("a", "first")])));
    expect(jobsButton().getAttribute("aria-label")).toBe("Jobs, 1");
    adapter.read.mockImplementation(never);
    await render(actions("second", page([cron("a", "first")])));
    expect(jobsButton().getAttribute("aria-label")).toBe("Jobs");
  });

  it("counts from a graph this page already read, before its own read returns", async () => {
    adapter.peek.mockReturnValue(graph("seeded"));
    adapter.read.mockImplementation(never);
    await render(actions("seeded", page([cron("a", "seeded")])));
    expect(jobsButton().getAttribute("aria-label")).toBe("Jobs, 1");
  });

  it("gives the count one slot whether or not it has arrived", async () => {
    adapter.read.mockImplementation(never);
    await render(actions("slot", page([cron("a", "slot")])));
    const before = jobsButton().children.length;
    await unmount();
    adapter.peek.mockReturnValue(graph("slot"));
    await render(actions("slot", page([cron("a", "slot")])));
    expect(jobsButton().textContent).toContain("1");
    expect(jobsButton().children.length).toBe(before);
  });
});
