import { dc } from "../lib/env.mjs";
import { closePickerScript, connectPage, focusedPaneScript, openPickerScript, startTeamScript } from "../lib/cdp.mjs";
import { pairTabs } from "../lib/oracle.mjs";
import { readTab, readTabs, waitReady, withEvents } from "../lib/tabs.mjs";
import { JourneyFailure, observe } from "../lib/wait.mjs";

const workspaceEvent = (event) => event?.event_type === "overflow" || event?.event_type === "mux-state-changed" ||
  (typeof event?.event_type === "string" && event.event_type.startsWith("tab"));

function recordTeam(ctx, team) {
  ctx.state.teams.push(team);
  for (const [role, id] of [["orchestrator", team.orchestrator], ["planner", team.planner]]) {
    ctx.state.tabs.push({ id, provider: team.provider, journey: "J2", role, team: team.id, reply: role === "orchestrator" && team.leg === "api_no_context" });
  }
  ctx.save();
}

async function readyPair(ctx, team) {
  const [orchestrator, planner] = await Promise.all([waitReady(ctx, team.orchestrator), waitReady(ctx, team.planner)]);
  team.id = orchestrator.team_id ?? planner.team_id ?? null;
  if (!team.id || orchestrator.team_id !== planner.team_id) {
    throw new JourneyFailure(`pair members do not share one Team: ${orchestrator.team_id} / ${planner.team_id}`);
  }
  return { orchestrator: orchestrator.boot_stage, planner: planner.boot_stage };
}

// Exactly the body the dialog's store sends when no tab is selected (no caller claim).
export function apiNoContext(ctx, provider) {
  return ctx.check("J2", "api_no_context", provider, async () => {
    const response = await ctx.api("/api/orchestration/new", { method: "POST", body: {
      orchestrator_agent: provider, planner_agent: provider, workspace_id: ctx.state.workspace.id,
    }, signal: AbortSignal.timeout(ctx.deadlines.ready) });
    const pair = pairTabs(response);
    if (pair.fail) throw new JourneyFailure(pair.fail, response);
    const team = { leg: "api_no_context", provider, ...pair.done };
    const stages = await readyPair(ctx, team);
    recordTeam(ctx, team);
    return { summary: `team ${team.id} ${team.orchestrator}+${team.planner}`, evidence: { team, stages } };
  });
}

async function newTeamMembers(ctx, knownTeams, signal) {
  const tabs = await readTabs(ctx.api, signal);
  const members = tabs.filter((tab) => tab.workspace_id === ctx.state.workspace.id && tab.team_id && !knownTeams.has(tab.team_id));
  const byTeam = new Map();
  for (const tab of members) byTeam.set(tab.team_id, [...(byTeam.get(tab.team_id) ?? []), tab]);
  return [...byTeam.entries()].find(([, group]) => group.length >= 2) ?? null;
}

export function uiContext(ctx, { leg, contextTab, provider }) {
  return ctx.check("J2", leg, provider, async () => {
    if (!contextTab) throw new JourneyFailure("no context tab: the Manager/Worker fixture failed earlier");
    const context = await readTab(ctx.api, contextTab);
    if (!context) throw new JourneyFailure(`context tab ${contextTab} is absent`);
    const knownTeams = new Set((await readTabs(ctx.api)).map((tab) => tab.team_id).filter(Boolean));
    const focus = await dc(["tab", "focus", "--tab", contextTab]);
    if (focus.code !== 0) throw new JourneyFailure(`tab focus exit ${focus.code}: ${focus.stdout || focus.stderr}`);
    const page = await connectPage(ctx.dev);
    let previous = null;
    let outcome;
    try {
      await page.evaluate(focusedPaneScript(contextTab, ctx.deadlines.ui), ctx.deadlines.ui);
      ({ previous } = await page.evaluate(openPickerScript({
        workspaceId: ctx.state.workspace.id, workspaceCwd: ctx.state.workspace.cwd,
        choice: { mode: "pair", orchestratorProviderId: provider, plannerProviderId: provider },
      }, ctx.deadlines.ui), ctx.deadlines.ui));
      outcome = await page.evaluate(startTeamScript(ctx.deadlines.ready), ctx.deadlines.ready);
    } finally {
      await page.evaluate(closePickerScript(previous), 5_000).catch(() => {});
      page.close();
    }
    if (!outcome?.ok) {
      throw new JourneyFailure(`dialog refused with ${context.role ?? "unknown"} tab ${contextTab} selected: ${outcome?.alert ?? "no outcome"}`);
    }
    const [teamId, members] = await withEvents(ctx, ctx.deadlines.ready, (events, signal) => observe({
      events, filter: workspaceEvent, label: "new Team members in the workspace",
      read: () => newTeamMembers(ctx, knownTeams, signal),
      judge: (found) => (found ? { done: found } : { pending: "new Team members in the workspace" }),
    }));
    const orchestrator = members.find((tab) => tab.can_spawn) ?? members[0];
    const planner = members.find((tab) => tab.id !== orchestrator.id);
    const team = { leg, provider, id: teamId, orchestrator: orchestrator.id, planner: planner.id, context_role: context.role ?? null };
    const stages = await readyPair(ctx, team);
    recordTeam(ctx, team);
    return { summary: `team ${teamId} from ${context.role ?? "?"} context`, evidence: { team, stages } };
  });
}
