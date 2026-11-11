export { askGeneralAgent, focusTab, generalAgentTab, workspaceOfTab } from "../../lib/generalAgent";

/** Who the General Agent is asked about: a connection has an id once both sides agreed. */
export interface NeighborSubject {
  name: string;
  email?: string;
  friendshipId?: string;
}
const who = (subject: NeighborSubject) => subject.email ? `${subject.name} (${subject.email})` : subject.name;

export function permissionInterviewTask(subject: NeighborSubject): string {
  const id = subject.friendshipId;
  const apply = id
    ? [
      `Check \`domios account neighbors status\` for friend_receivers["${id}"]. If unknown, wait for authoritative status; if elsewhere, name the receiving machine and ask me to confirm before using receive-here for this friend with --expected-receiver-revision and --takeover-confirmed. Do not enroll all friends or bind a manual agent. If no receiver is chosen, use receive-here for this friend with the observed --expected-receiver-revision before applying grants.`,
      `Their connection id is ${id}. Write a short, specific scope into your pad and apply it with \`domios account neighbors grant ${id} --from-pad <file>\`; add \`--allow-tab-requests true\` only if I agreed. If I want nothing shared, run \`domios account neighbors deny ${id}\`.`,
    ]
    : ["They have not accepted yet, so there is no connection id. Agree the scope now and keep it in your pad. Once they accept, find their connection id with `domios account neighbors list`, check that friend's effective receiver, and apply local grants only on the receiving machine."];
  return [
    `I just added ${who(subject)} as a Neighbor in Domios. Set up what they may do. Read \`domios docs NEIGHBORS\` first.`,
    "Interview me here, one short question at a time: what their agents may ask for, what information may be shared, what is off limits, and whether they may ask to open tabs. Start from deny and keep grants narrow. Permissions and files stay local to each machine. Gather the grant fields into one Apply; first tell me it restarts this friend's concierge with a fresh session.",
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
