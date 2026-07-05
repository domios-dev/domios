// Run: npx tsx src/components/Canvas/swarmRoster.test.ts

import type { PresenceAgent } from "./presence/identity";
import type { PresenceSource } from "./presenceOverlays";
import type { Task } from "./types";
import { MAX_SWARM_ROSTER, buildSwarmRoster } from "./swarmRoster";

let passed = 0;
let failed = 0;
function ok(label: string, cond: boolean) {
  if (cond) { console.log(`  ✓ ${label}`); passed++; }
  else { console.error(`  ✗ ${label}`); failed++; }
}

function agent(id: string, label = id, isPeer = false): PresenceAgent {
  return { id, label, role: isPeer ? "peer" : "lead", color: "#4f7df0", isPeer };
}
function task(id: string, branch: string | null = null): Task {
  return { id, name: `Task ${id}`, lane: branch ? { branch } : undefined } as unknown as Task;
}
function src(agents: PresenceAgent[], live: boolean): PresenceSource {
  return { agents, live, label: agents[0]?.label ?? "" };
}

// Dedup: an agent on many tasks collapses to ONE member with the right taskCount.
{
  const shared = agent("peer:claude", "claude-sonnet", true);
  const presence = new Map<string, PresenceSource>([
    ["t1", src([agent("a1"), shared], false)],
    ["t2", src([agent("a2"), shared], true)],
    ["t3", src([agent("a3"), shared], false)],
  ]);
  const taskById = new Map([["t1", task("t1", "feat/1")], ["t2", task("t2", "feat/2")], ["t3", task("t3")]]);
  const r = buildSwarmRoster({ presenceByTask: presence, taskById });
  ok("distinct agents deduped (3 leads + 1 shared peer = 4)", r.totalCount === 4);
  const claude = r.members.find((m) => m.agent.id === "peer:claude");
  ok("shared peer collapses to one member", !!claude);
  ok("shared peer taskCount counts all its tasks", claude?.taskCount === 3);
  ok("shared peer is live (one of its tasks is live)", claude?.live === true);
  ok("live agent's current task is the LIVE task (t2)", claude?.taskId === "t2");
  ok("current task carries its title", claude?.taskTitle === "Task t2");
  ok("current task carries its branch", claude?.branch === "feat/2");
}

// Sort: live first, leads before peers, then by label. liveCount headline correct.
{
  const presence = new Map<string, PresenceSource>([
    ["t1", src([agent("z-lead", "zeta")], false)],
    ["t2", src([agent("a-lead", "alpha")], true)],
    ["t3", src([agent("p", "mike", true)], true)],
  ]);
  const taskById = new Map([["t1", task("t1")], ["t2", task("t2")], ["t3", task("t3")]]);
  const r = buildSwarmRoster({ presenceByTask: presence, taskById });
  ok("liveCount counts distinct live agents", r.liveCount === 2);
  ok("first member is live", r.members[0]?.live === true);
  ok("live lead sorts before live peer", r.members[0]?.agent.id === "a-lead" && r.members[1]?.agent.id === "p");
  ok("idle agent sorts last", r.members[2]?.agent.id === "z-lead");
}

// Budget: 100 agents cap to MAX_SWARM_ROSTER with honest overflow.
{
  const presence = new Map<string, PresenceSource>();
  const taskById = new Map<string, Task>();
  for (let i = 0; i < 100; i++) {
    presence.set(`t${i}`, src([agent(`a${i}`, `agent-${i}`)], i % 2 === 0));
    taskById.set(`t${i}`, task(`t${i}`));
  }
  const r = buildSwarmRoster({ presenceByTask: presence, taskById });
  ok("totalCount reflects the whole swarm (100)", r.totalCount === 100);
  ok("rendered roster is capped at MAX_SWARM_ROSTER", r.members.length === MAX_SWARM_ROSTER);
  ok("overflow is honest (100 - cap)", r.overflow === 100 - MAX_SWARM_ROSTER);
  ok("capped roster keeps live agents first", r.members.every((m, i) => i === 0 || !(m.live && !r.members[i - 1].live)));
}

// Empty swarm → empty roster (the cluster hides).
{
  const r = buildSwarmRoster({ presenceByTask: new Map(), taskById: new Map() });
  ok("empty swarm → no members, no counts", r.members.length === 0 && r.totalCount === 0 && r.liveCount === 0 && r.overflow === 0);
}

console.log(`\n${passed} passed, ${failed} failed`);
if (failed > 0) process.exit(1);
