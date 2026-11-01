import { readFileSync } from "node:fs";
import { pathToFileURL } from "node:url";

function nonEmpty(value) {
  return typeof value === "string" && value.trim() ? value.trim() : "";
}

function inspectedTabs(value) {
  value = value?.result;
  if (Array.isArray(value)) return value;
  if (value && Array.isArray(value.tabs)) return value.tabs;
  if (value && value.tab && typeof value.tab === "object") return [value.tab];
  return value && typeof value === "object" ? [value] : [];
}

function teamMembers(value) {
  if (value?.result && Array.isArray(value.result.members)) return value.result.members;
  return [];
}

export function resolveDevNotifyTarget({ explicitTab, shellTab, parentHint, tabModel, teamModel } = {}) {
  const explicit = nonEmpty(explicitTab);
  const shell = nonEmpty(shellTab);
  const tabs = inspectedTabs(tabModel);
  const current = tabs.find(tab => nonEmpty(tab?.id ?? tab?.tab_id) === shell) ?? (shell ? undefined : tabs[0]);
  const remotePeer = nonEmpty(current?.report_target_mesh_peer);
  const remoteTab = nonEmpty(current?.report_target_tab_id);

  if (explicit) {
    return {
      kind: "send",
      tab: explicit,
      mesh: explicit === remoteTab ? remotePeer : "",
    };
  }

  if (remotePeer && remoteTab) return { kind: "send", tab: remoteTab, mesh: remotePeer };
  // An agent runs its own checks and reads its own receipts; the parent route below is for
  // utility shells, whose parent is the agent that opened them.
  if (shell && nonEmpty(current?.agent_type)) return { kind: "send", tab: shell, mesh: "" };

  const parent = nonEmpty(current?.parent_tab_id);
  if (parent) return { kind: "send", tab: parent, mesh: "" };

  const reportTarget = nonEmpty(current?.report_target_tab_id);
  if (reportTarget) return { kind: "send", tab: reportTarget, mesh: "" };

  const owner = teamMembers(teamModel).find(member =>
    member?.anchors_team === true || ["manager", "orchestrator"].includes(nonEmpty(member?.role_type).toLowerCase()),
  );
  const ownerTab = nonEmpty(owner?.tab_id);
  if (ownerTab && ownerTab !== shell) return { kind: "send", tab: ownerTab, mesh: "" };

  const creator = nonEmpty(current?.spawned_by_tab_id);
  if (creator) return { kind: "send", tab: creator, mesh: "" };

  if (!current) {
    const launchParent = nonEmpty(parentHint);
    if (launchParent) return { kind: "send", tab: launchParent, mesh: "" };
  }

  if (shell) return { kind: "notify", tab: shell, mesh: "" };
  return { kind: "none", tab: "", mesh: "" };
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  const args = process.argv.slice(2);
  let tabModel = null;
  try {
    const input = readFileSync(0, "utf8");
    tabModel = input.trim() ? JSON.parse(input) : null;
  } catch {
    tabModel = null;
  }
  if (args[0] === "--team-id") {
    const shellTab = nonEmpty(args[1]);
    const tabs = inspectedTabs(tabModel);
    const current = tabs.find(tab => nonEmpty(tab?.id ?? tab?.tab_id) === shellTab);
    const hasRemoteRoute = nonEmpty(current?.report_target_mesh_peer) && nonEmpty(current?.report_target_tab_id);
    const hasDirectRoute = nonEmpty(current?.parent_tab_id) || nonEmpty(current?.report_target_tab_id);
    process.stdout.write(`${!hasRemoteRoute && !hasDirectRoute ? nonEmpty(current?.team_id) : ""}\n`);
  } else {
    const [explicitTab, shellTab, parentHint, rawTeamModel] = args;
    let teamModel = null;
    try {
      teamModel = rawTeamModel ? JSON.parse(rawTeamModel) : null;
    } catch {
      teamModel = null;
    }
    const target = resolveDevNotifyTarget({ explicitTab, shellTab, parentHint, tabModel, teamModel });
    process.stdout.write(`${target.kind}\t${target.tab}\t${target.mesh}\n`);
  }
}
