import process from "node:process";

async function readInput() {
  let input = "";
  for await (const chunk of process.stdin) input += chunk;
  return JSON.parse(input);
}

function atPath(value, path) {
  return path
    .replace(/^\./, "")
    .split(".")
    .filter(Boolean)
    .reduce((current, key) => current?.[key], value);
}

function findObject(value, predicate) {
  if (value && typeof value === "object") {
    if (!Array.isArray(value) && predicate(value)) return value;
    for (const child of Object.values(value)) {
      const found = findObject(child, predicate);
      if (found) return found;
    }
  }
  return null;
}

function requireState(condition) {
  if (!condition) process.exitCode = 1;
}

async function main() {
  const [mode, ...args] = process.argv.slice(2);
  const value = await readInput();

  if (mode === "field") {
    const paths = args.join(" ").split(/\s*\/\/\s*/);
    const selected = paths.map((path) => atPath(value, path)).find((item) => item != null);
    process.stdout.write(selected == null ? "null" : String(selected));
    return;
  }

  if (mode === "absent-orchestrator") {
    const [tabId, teamId] = args;
    requireState(Boolean(findObject(value, (item) =>
      item.id === tabId
      && item.agent_liveness === "absent"
      && item.agent_type == null
      && item.boot_stage === "shell_ready"
      && item.role_profile === "orchestrator"
      && item.team_id === teamId,
    )));
    return;
  }

  if (mode === "team-count") {
    const [teamId, expected] = args;
    requireState(Array.isArray(value.teams)
      && value.teams.filter((team) => team.id === teamId).length === Number(expected));
    return;
  }

  if (mode === "live-tab") {
    const [tabId] = args;
    requireState(Boolean(findObject(value, (item) =>
      item.id === tabId && item.agent_liveness === "live",
    )));
    return;
  }

  if (mode === "general-agent") {
    const tabs = Array.isArray(value) ? value : (value.tabs || []);
    const generalAgent = tabs.find((tab) => tab.role_profile === "general_agent");
    requireState(Boolean(generalAgent?.id));
    if (generalAgent?.id) process.stdout.write(generalAgent.id);
    return;
  }

  if (mode === "plan-state") {
    const [planId, teamId, ownerId, minimumHistory] = args;
    const plan = value.plan ?? value;
    const ownerMatches = ownerId === "null"
      ? plan.owner?.tab_id == null
      : plan.owner?.tab_id === ownerId;
    requireState(plan.id === planId
      && plan.team_id === teamId
      && plan.status === "active"
      && ownerMatches
      && (minimumHistory === undefined
        || (Array.isArray(plan.owner_history) && plan.owner_history.length >= Number(minimumHistory))));
    return;
  }

  throw new Error(`unknown JSON state mode: ${mode}`);
}

main().catch((error) => {
  console.error(error);
  process.exitCode = 1;
});
