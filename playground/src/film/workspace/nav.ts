/** Where the visitor is in the one demo workspace, and how each click in the app's own UI moves them (spec L13.2). */

export type PlaceId = "chat" | "plan" | "machines" | "teams" | "usage" | "plugins";
export const PLACES: PlaceId[] = ["chat", "plan", "machines", "teams", "usage", "plugins"];

/** What the main pane shows; "teams" is the installed teams laid open in the sidebar beside the team lead's chat. */
export type View = "chat" | "plan" | "usage" | "plugins" | "teams" | "jobs" | "memory" | "mesh" | "neighbors";

/** Views the app opens from its own navigation that no site shortcut names; they report the place they sit in. */
const SIDE_VIEWS = new Set<View>(["jobs", "memory", "mesh", "neighbors"]);

export interface WorkspaceRef {
  id: string;
  tabs: string[];
}

export interface MachineRef<M extends string = string> {
  id: M;
  workspaces: WorkspaceRef[];
  home: string;
}

/** The world the model navigates: machines with their workspaces and tabs, plus the two fixed anchors. */
export interface World<M extends string = string> {
  machines: MachineRef<M>[];
  /** The machine the visitor starts on, and the one P1 shows. */
  local: M;
  remote: M;
  /** The workspace holding the installed teams P3 shows, and its landing tab. */
  teams: { machine: M; workspace: string; tab: string };
}

export interface Nav<M extends string = string> {
  machine: M;
  /** Selected workspace per machine. */
  workspace: Record<string, string>;
  /** Active tab per workspace. */
  tab: Record<string, string>;
  view: View;
  /** The task whose detail is open over the board. */
  task: string | null;
}

const machineOf = <M extends string>(world: World<M>, id: M) => world.machines.find((m) => m.id === id)!;

export function initialNav<M extends string>(world: World<M>): Nav<M> {
  const workspace: Record<string, string> = {};
  const tab: Record<string, string> = {};
  for (const machine of world.machines) {
    workspace[machine.id] = machine.workspaces.find((ws) => ws.tabs.includes(machine.home))?.id ?? machine.workspaces[0].id;
    for (const ws of machine.workspaces) tab[ws.id] = ws.tabs.includes(machine.home) ? machine.home : ws.tabs[0];
  }
  return { machine: world.local, workspace, tab, view: "chat", task: null };
}

export function activeWorkspace<M extends string>(nav: Nav<M>): string {
  return nav.workspace[nav.machine];
}

export function activeTab<M extends string>(nav: Nav<M>): string {
  return nav.tab[activeWorkspace(nav)];
}

export function placeOf<M extends string>(world: World<M>, nav: Nav<M>): PlaceId {
  if (nav.view === "mesh") return "machines";
  if (nav.view !== "chat" && !SIDE_VIEWS.has(nav.view)) return nav.view as PlaceId;
  return nav.machine === world.local ? "chat" : "machines";
}

/** A site chip or fog command: the place's canonical spot, keeping whatever the visitor already opened there. */
export function goPlace<M extends string>(world: World<M>, nav: Nav<M>, place: PlaceId): Nav<M> {
  if (placeOf(world, nav) === place && !SIDE_VIEWS.has(nav.view)) return nav;
  switch (place) {
    case "plan":
    case "plugins":
      return { ...nav, view: place };
    case "usage":
      // The usage Board is the installed team's own, so the sidebar opens on that team's workspace beside it.
      return { ...nav, machine: world.teams.machine, workspace: { ...nav.workspace, [world.teams.machine]: world.teams.workspace }, view: "usage" };
    case "machines":
      return { ...nav, machine: nav.machine === world.local ? world.remote : nav.machine, view: "chat" };
    case "teams":
      return {
        ...nav, machine: world.teams.machine, view: "teams",
        workspace: { ...nav.workspace, [world.teams.machine]: world.teams.workspace }, tab: { ...nav.tab, [world.teams.workspace]: world.teams.tab },
      };
    case "chat": {
      const local = machineOf(world, world.local);
      const leavingTeams = nav.view === "teams" && nav.workspace[world.local] === world.teams.workspace;
      const ws = leavingTeams ? local.workspaces.find((w) => w.tabs.includes(local.home))!.id : nav.workspace[world.local];
      return { ...nav, machine: world.local, workspace: { ...nav.workspace, [world.local]: ws }, view: "chat" };
    }
  }
}

/** The peer switcher picked a machine: same view, that machine's tabs. */
export function selectMachine<M extends string>(nav: Nav<M>, machine: M): Nav<M> {
  return machine === nav.machine ? nav : { ...nav, machine };
}

export function selectWorkspace<M extends string>(nav: Nav<M>, workspace: string): Nav<M> {
  return { ...nav, workspace: { ...nav.workspace, [nav.machine]: workspace } };
}

/** A tab row, an owner chip, or a routed message's sender: that tab's chat, on whichever machine runs it. */
export function focusTab<M extends string>(world: World<M>, nav: Nav<M>, tabId: string): Nav<M> {
  for (const machine of world.machines) {
    const ws = machine.workspaces.find((w) => w.tabs.includes(tabId));
    if (!ws) continue;
    return { ...nav, machine: machine.id, workspace: { ...nav.workspace, [machine.id]: ws.id }, tab: { ...nav.tab, [ws.id]: tabId }, view: "chat", task: null };
  }
  return nav;
}

/** A task link in chat or a card on the board: the board with that card's detail open. */
/** A view the app opens from its titlebar or sidebar footer, on the current machine. */
export function openView<M extends string>(nav: Nav<M>, view: View): Nav<M> {
  return { ...nav, view, task: view === "plan" ? nav.task : null };
}

export function openTask<M extends string>(nav: Nav<M>, taskId: string | null): Nav<M> {
  return { ...nav, view: "plan", task: taskId };
}
