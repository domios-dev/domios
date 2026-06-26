import type { PresenceSource } from "./presenceOverlays";
import type { PresenceAgent } from "./presence/identity";
import type { Task } from "./types";

// EMERGENCE (north-star #2): the top-right multiplayer cluster shows the WHOLE
// swarm as ONE circle per DISTINCT agent — deduped across the many tasks an agent
// may touch (that's the cluster's job; per-task pins are the floating layer). Pure
// + CAMERA-DECOUPLED: depends only on the swarm + task data, never pan/zoom, so the
// screen-space cluster is stable while you fly around. Bounded like the pin budget
// so a 100-agent fleet collapses to a capped roster + an honest +N overflow.
export const MAX_SWARM_ROSTER = 32;

export interface SwarmMember {
  agent: PresenceAgent;
  /** The task to fly to — the agent's live task if any, else its lowest-id task. */
  taskId: string;
  taskTitle: string;
  branch: string | null;
  /** Any of this agent's tasks is executing → a live identity ring. */
  live: boolean;
  /** Distinct tasks this agent appears on (drives the "on N tasks" detail). */
  taskCount: number;
}

export interface SwarmRoster {
  /** Capped + sorted: live leads first. The rendered roster. */
  members: SwarmMember[];
  /** Distinct agents currently executing — the headline "N working". */
  liveCount: number;
  /** Distinct agents in the whole swarm, before the roster cap. */
  totalCount: number;
  /** Agents hidden by the roster cap (the +N overflow). */
  overflow: number;
}

export interface BuildSwarmRosterInput {
  /** Active tasks keyed by id — iterated directly: O(active swarm), not O(board). */
  presenceByTask: ReadonlyMap<string, PresenceSource>;
  taskById: ReadonlyMap<string, Task>;
  maxRoster?: number;
}

interface Acc {
  agent: PresenceAgent;
  taskId: string;
  taskLive: boolean;
  live: boolean;
  taskCount: number;
}

// A live task wins the "current task" slot; among equal liveness the lowest id wins
// so the fly-to target is deterministic (no flicker between equally-valid tasks).
function prefers(taskId: string, live: boolean, prev: Acc): boolean {
  if (live !== prev.taskLive) return live;
  return taskId.localeCompare(prev.taskId) < 0;
}

export function buildSwarmRoster({
  presenceByTask,
  taskById,
  maxRoster = MAX_SWARM_ROSTER,
}: BuildSwarmRosterInput): SwarmRoster {
  const byAgent = new Map<string, Acc>();
  for (const [taskId, src] of presenceByTask) {
    if (!src.agents.length) continue;
    for (const agent of src.agents) {
      const prev = byAgent.get(agent.id);
      if (!prev) {
        byAgent.set(agent.id, { agent, taskId, taskLive: src.live, live: src.live, taskCount: 1 });
        continue;
      }
      prev.taskCount += 1;
      prev.live = prev.live || src.live;
      if (prefers(taskId, src.live, prev)) {
        prev.taskId = taskId;
        prev.taskLive = src.live;
      }
    }
  }

  const members: SwarmMember[] = [];
  for (const e of byAgent.values()) {
    const task = taskById.get(e.taskId);
    members.push({
      agent: e.agent,
      taskId: e.taskId,
      taskTitle: task?.name?.trim() || e.taskId,
      branch: task?.lane?.branch?.trim() || null,
      live: e.live,
      taskCount: e.taskCount,
    });
  }

  // Live agents first (the swarm you watch), leads before peers, then by label —
  // a stable, scannable order; agent id is the final deterministic tiebreak.
  members.sort(
    (a, b) =>
      Number(b.live) - Number(a.live) ||
      Number(!!a.agent.isPeer) - Number(!!b.agent.isPeer) ||
      a.agent.label.localeCompare(b.agent.label) ||
      a.agent.id.localeCompare(b.agent.id),
  );

  const totalCount = members.length;
  const liveCount = members.reduce((n, m) => n + (m.live ? 1 : 0), 0);
  const shown = totalCount > maxRoster ? members.slice(0, maxRoster) : members;
  return { members: shown, liveCount, totalCount, overflow: totalCount - shown.length };
}
