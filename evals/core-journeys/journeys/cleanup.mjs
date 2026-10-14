import { dc, dcJson } from "../lib/env.mjs";
import { readTabs } from "../lib/tabs.mjs";
import { JourneyFailure } from "../lib/wait.mjs";

async function ok(args, options) {
  const result = await dc(args, options);
  if (result.code !== 0) throw new JourneyFailure(`dc ${args.slice(0, 3).join(" ")} exit ${result.code}: ${(result.stdout || result.stderr).slice(0, 400)}`);
  return result;
}

async function ownedPlans(ctx) {
  const listed = await dcJson(["plan", "list"]).catch(() => []);
  const plans = Array.isArray(listed) ? listed : listed?.plans ?? [];
  const teams = new Set(ctx.state.teams.map((team) => team.id).filter(Boolean));
  const ids = new Set(ctx.state.plans);
  for (const plan of plans) if (teams.has(plan.team_id)) ids.add(plan.plan_id ?? plan.id);
  return [...ids].filter(Boolean);
}

export async function cleanup(ctx) {
  const { workspace } = ctx.state;
  if (!workspace?.id) return;
  for (const plan of await ownedPlans(ctx)) {
    await ctx.check("CLEAN", "plan_deleted", plan, async () => {
      const owner = await dc(["plan", "delete", plan, "--yes"]);
      if (owner.code !== 0 && ctx.state.j3?.orchestrator) await ok(["plan", "delete", plan, "--yes"], { asTab: ctx.state.j3.orchestrator });
      else if (owner.code !== 0) throw new JourneyFailure(`plan delete exit ${owner.code}: ${owner.stdout || owner.stderr}`);
      return "deleted";
    });
  }
  for (const team of ctx.state.teams.filter((entry) => entry.id)) {
    await ctx.check("CLEAN", "team_closed", team.id, async () => (await ok(["team", "close", team.id, "--force"]), "closed"));
  }
  await ctx.check("CLEAN", "workspace_closed", workspace.id, async () => {
    await ok(["workspace", "close", workspace.id, "--force"]);
    const left = (await readTabs(ctx.api)).filter((tab) => tab.workspace_id === workspace.id);
    if (left.length) throw new JourneyFailure(`tabs remain in the closed workspace: ${left.map((tab) => tab.id).join(", ")}`);
    return "closed; no owned tabs remain";
  });
}
