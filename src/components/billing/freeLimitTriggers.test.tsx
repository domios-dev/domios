import { afterEach, describe, expect, it } from "vitest";
import { performTabMenuAction } from "../Terminal/tabMenuActions";
import { freeLimitGate } from "./freeLimitGate";
import { fakeDaemon, mountApp, modal, modalLine, seedState, toastTitles, unmountAll, WORKSPACE, type Daemon } from "./freeLimitHarness";
import type { FreeLimits } from "./freeLimitModel";

const FREE: FreeLimits = { enforcing: true, max: { workspaces: 2, teams: 6, "agent.tabs": 35 } };
const DORMANT: FreeLimits = { ...FREE, enforcing: false };
const WORKSPACES = "Your plan includes 2 workspaces.";
const TEAMS = "Your plan includes 6 Teams across all workspaces.";
const TABS = "Your plan allows 35 tabs running at once.";

const atLimits = () => seedState({ workspaces: 2, teams: 6, tabs: 35 });

afterEach(unmountAll);

describe("a daemon limit refusal opens the plans modal from every creation trigger", () => {
  // The client has no limits yet, so nothing is pre-checked and the daemon's refusal is what opens the modal.
  const refusedBy = async () => {
    const daemon = fakeDaemon(atLimits(), FREE);
    return { daemon, app: await mountApp(daemon) };
  };

  it.each([
    ["new tab", (a: Actions): Promise<unknown> => a.createTab(WORKSPACE), "mux_create_tab", TABS],
    ["workspace +", (a: Actions): Promise<unknown> => a.createWorkspace("next", "/elsewhere"), "mux_create_workspace", WORKSPACES],
    ["agent spawn", (a: Actions): Promise<unknown> => a.spawnAgentTab(WORKSPACE, "claude", "/repo").catch(() => null), "mux_spawn_tab", TABS],
    ["General Agent new session", (a: Actions): Promise<unknown> => a.spawnAgentTab(WORKSPACE, "claude", "/repo", undefined, undefined, undefined, "fresh_general").catch(() => null), "spawn_general_agent_session", TABS],
    ["Team pair", (a: Actions): Promise<unknown> => a.createOrchestrationPair({ orchestratorAgent: "claude", plannerAgent: "claude", workspaceId: WORKSPACE }).catch(() => null), "mux_create_orchestration_pair", TABS],
  ] as const)("%s", async (_name, trigger, command, line) => {
    const { daemon, app } = await refusedBy();
    await app.run(() => trigger(app.actions()));
    expect(daemon.created(command)).toBe(1);
    expect(modalLine()).toBe(line);
    expect(toastTitles(app.state() as never)).toEqual([]);
  });

  it("Team pair names the Teams limit when Teams are what is full", async () => {
    const daemon = fakeDaemon(seedState({ workspaces: 1, teams: 6, tabs: 3 }), FREE);
    const app = await mountApp(daemon);
    await app.run(() => app.actions().createOrchestrationPair({ orchestratorAgent: "claude", plannerAgent: "claude", workspaceId: WORKSPACE }).catch(() => null));
    expect(modalLine()).toBe(TEAMS);
  });

  it("Claude spawn shortcut opens the modal instead of a failure toast", async () => {
    const { app } = await refusedBy();
    await app.run(async () => app.actions().spawnClaudeTab(WORKSPACE, "/repo"));
    expect(modalLine()).toBe(TABS);
    expect(toastTitles(app.state() as never)).not.toContain("Session start failed");
  });

  it.each([
    ["Make Orchestrator", { type: "makeOrchestrator" } as const, TEAMS],
    ["Fork", { type: "fork" } as const, TABS],
  ])("tab menu %s", async (_name, action, line) => {
    const { app } = await refusedBy();
    await app.run(() => performTabMenuAction("tab-0", action, { sessionBindingNonce: "opaque-nonce" }));
    expect(modalLine()).toBe(line);
  });
});

describe("the pre-check opens the modal without a create call", () => {
  it.each([
    ["new tab", (a: Actions): Promise<unknown> => a.createTab(WORKSPACE), "mux_create_tab", TABS],
    ["workspace +", (a: Actions): Promise<unknown> => a.createWorkspace("next", "/elsewhere"), "mux_create_workspace", WORKSPACES],
    ["agent spawn", (a: Actions): Promise<unknown> => a.spawnAgentTab(WORKSPACE, "claude", "/repo"), "mux_spawn_tab", TABS],
    ["Team pair", (a: Actions): Promise<unknown> => a.createOrchestrationPair({ orchestratorAgent: "claude", plannerAgent: "claude", workspaceId: WORKSPACE }).catch(() => null), "mux_create_orchestration_pair", TEAMS],
    ["Fork", (): Promise<unknown> => performTabMenuAction("tab-0", { type: "fork" }), "fork_agent_tab", TABS],
  ] as const)("%s at the limit under enforcement", async (_name, trigger, command, line) => {
    const daemon = fakeDaemon(atLimits(), FREE);
    const app = await mountApp(daemon);
    freeLimitGate.setLimits(FREE);
    await app.run(() => trigger(app.actions()));
    expect(daemon.created(command)).toBe(0);
    expect(modalLine()).toBe(line);
  });

  it("a folder that is already a workspace still reaches the daemon, which reuses it", async () => {
    const daemon = fakeDaemon(atLimits(), FREE);
    const app = await mountApp(daemon);
    freeLimitGate.setLimits(FREE);
    await app.run(() => app.actions().createWorkspace("again", "/repo/1"));
    expect(daemon.created("mux_create_workspace")).toBe(1);
  });

  it("under the dormant gate creation proceeds past every free limit", async () => {
    const daemon = fakeDaemon(atLimits(), DORMANT);
    const app = await mountApp(daemon);
    freeLimitGate.setLimits(DORMANT);
    await app.run(async () => {
      await app.actions().createTab(WORKSPACE);
      await app.actions().createWorkspace("third", "/elsewhere");
      await app.actions().spawnAgentTab(WORKSPACE, "claude", "/repo");
      await app.actions().createOrchestrationPair({ orchestratorAgent: "claude", plannerAgent: "claude", workspaceId: WORKSPACE });
    });
    expect(modal()).toBeNull();
    expect(daemon.counts()).toEqual({ workspaces: 3, teams: 7, "agent.tabs": 39 });
  });

  it("a stale pre-check that lets the click through still opens the modal on the daemon's refusal", async () => {
    const daemon = fakeDaemon(atLimits(), FREE);
    const app = await mountApp(daemon);
    freeLimitGate.setLimits(DORMANT);
    await app.run(() => app.actions().createTab(WORKSPACE));
    expect(daemon.created("mux_create_tab")).toBe(1);
    expect(modalLine()).toBe(TABS);
  });
});

describe("errors that are not limits keep their toasts", () => {
  const failing = async (...commands: string[]): Promise<{ daemon: Daemon; app: Awaited<ReturnType<typeof mountApp>> }> => {
    const daemon = fakeDaemon(seedState({}), FREE);
    for (const command of commands) daemon.failWith(command, "opaque-daemon-failure");
    return { daemon, app: await mountApp(daemon) };
  };

  it("Claude spawn shortcut", async () => {
    const { app } = await failing("mux_spawn_tab");
    await app.run(async () => app.actions().spawnClaudeTab(WORKSPACE, "/repo"));
    expect(modal()).toBeNull();
    expect(toastTitles(app.state() as never)).toContain("Session start failed");
  });

  it("new tab and tab menu", async () => {
    const { daemon, app } = await failing("fork_agent_tab", "mux_create_tab");
    await expect(app.run(() => performTabMenuAction("tab-0", { type: "fork" }))).rejects.toBe("opaque-daemon-failure");
    await app.run(() => app.actions().createTab(WORKSPACE));
    expect(daemon.created("mux_create_tab")).toBe(1);
    expect(modal()).toBeNull();
  });
});

type Actions = ReturnType<typeof import("../../store/multiplexer")["useMultiplexer"]>;
