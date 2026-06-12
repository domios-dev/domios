export interface GraphCommitInput {
  sha: string;
  parents: string[];
}

export interface GraphRow {
  sha: string;
  col: number;
  parents: string[];
  lanesBefore: (string | null)[];
}

// Assigns each commit a column and records the lane occupancy seen by that
// row, so the renderer can draw pass-through rails plus dot/edge geometry.
// Input must be in topo order (children before parents — `git log` default).
//
// Lane semantics: `lanes[i]` holds the sha a column is currently "waiting to
// emit". When a commit is reached we take the leftmost lane reserved for it
// (its column); its first parent inherits that column, extra parents claim
// fresh lanes. Converging parents reuse the lane that already expects them,
// which the renderer turns into a merge curve.
export function assignLanes(commits: GraphCommitInput[]): GraphRow[] {
  const lanes: (string | null)[] = [];
  const rows: GraphRow[] = [];

  const firstFreeLane = (): number => {
    for (let i = 0; i < lanes.length; i++) {
      if (lanes[i] === null) return i;
    }
    lanes.push(null);
    return lanes.length - 1;
  };

  for (const commit of commits) {
    const lanesBefore = lanes.slice();

    // Column = leftmost lane already reserved for this commit. If none (a tip),
    // claim the first free lane.
    let col = lanes.indexOf(commit.sha);
    if (col === -1) {
      col = firstFreeLane();
      lanes[col] = commit.sha;
    }

    // Clear every lane that was waiting for this commit; the merge edges from
    // those lanes are reconstructed from `lanesBefore` by the renderer.
    for (let i = 0; i < lanes.length; i++) {
      if (lanes[i] === commit.sha) lanes[i] = null;
    }

    rows.push({ sha: commit.sha, col, parents: commit.parents, lanesBefore });

    // Route parents into lanes. First parent keeps this commit's column when
    // free; each parent that already occupies a lane converges there.
    let firstParentPlaced = false;
    for (const parent of commit.parents) {
      if (lanes.indexOf(parent) !== -1) continue; // already converging
      if (!firstParentPlaced && (lanes[col] === null || lanes[col] === undefined)) {
        lanes[col] = parent;
        firstParentPlaced = true;
      } else {
        const free = firstFreeLane();
        lanes[free] = parent;
        firstParentPlaced = true;
      }
    }

    compactTrailingLanes(lanes);
  }

  return rows;
}

function compactTrailingLanes(lanes: (string | null)[]): void {
  while (lanes.length > 0 && lanes[lanes.length - 1] === null) {
    lanes.pop();
  }
}

export const LANE_COUNT = 8;

export function laneColorVar(col: number): string {
  return `var(--ct-graph-lane-${((col % LANE_COUNT) + LANE_COUNT) % LANE_COUNT})`;
}
