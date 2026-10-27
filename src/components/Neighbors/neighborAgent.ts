import { invoke } from "../../lib/bridge";
import type { Tab, Workspace } from "../../lib/types";

/** Who the General Agent is asked about: a connection has an id once both sides agreed. */
export interface NeighborSubject {
  name: string;
  email?: string;
  friendshipId?: string;
}

interface TabMap { workspaces: readonly Workspace[]; tabs: Record<string, Tab> }

export function workspaceOfTab(state: TabMap, tabId: string): string | null {
  return state.workspaces.find(workspace => workspace.tabs.includes(tabId))?.id ?? null;
}

/** The protected General tab, the only agent allowed to author Neighbor permissions. */
export function generalAgentTab(state: TabMap): { tabId: string; workspaceId: string } | null {
  for (const workspace of state.workspaces) {
    if (!workspace.protected) continue;
    const tabId = workspace.tabs.find(id => state.tabs[id]?.protected);
    if (tabId) return { tabId, workspaceId: workspace.id };
  }
  return null;
}

export function focusTab(tabId: string, workspaceId: string | null) {
  window.dispatchEvent(new CustomEvent("codeterm:focus-tab", { detail: { tabId, ...(workspaceId ? { workspaceId } : {}) } }));
}

const who = (subject: NeighborSubject) => subject.email ? `${subject.name} (${subject.email})` : subject.name;

export function permissionInterviewTask(subject: NeighborSubject): string {
  const id = subject.friendshipId;
  const apply = id
    ? [
      `Their connection id is ${id}. Write a short, specific scope into your pad and apply it with \`domios account neighbors grant ${id} --from-pad <file>\`; add \`--allow-tab-requests\` only if I agreed. If I want nothing shared, run \`domios account neighbors deny ${id}\`.`,
      `If \`domios account neighbors status\` shows no gateway for ${id}, ask which independent agent tab should receive their messages and bind it with \`domios account neighbors bind ${id} --tab <TAB>\`.`,
    ]
    : ["They have not accepted yet, so there is no connection id. Agree the scope now and keep it in your pad; when they accept I will open Configure on them and you apply it then."];
  return [
    `I just added ${who(subject)} as a Neighbor in Domios. Set up what they may do. Read \`domios docs NEIGHBORS\` first.`,
    "Interview me here, one short question at a time: what their agents may ask for, what information may be shared, what is off limits, and whether they may ask to open tabs. Start from deny and keep grants narrow.",
    ...apply,
    "Finish with two lines: what they can do and what they cannot.",
  ].join("\n\n");
}

export function talkAboutTask(subject: NeighborSubject): string {
  return [
    `I want to talk about my Neighbor ${who(subject)}${subject.friendshipId ? `, connection id ${subject.friendshipId}` : ""}.`,
    "Read `domios docs NEIGHBORS` if you need the commands, then ask me what I would like to do.",
  ].join(" ");
}

/** Hands a prepared task to the General Agent and brings its chat forward. */
export async function askGeneralAgent(state: TabMap, text: string): Promise<void> {
  const general = generalAgentTab(state);
  if (!general) throw new Error("The General Agent is not running. Open General and try again.");
  await invoke("chat_send_text", { tabId: general.tabId, text });
  focusTab(general.tabId, general.workspaceId);
}
