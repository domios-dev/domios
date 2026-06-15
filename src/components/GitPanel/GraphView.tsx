import { useMemo } from "react";
import { type GitCommit, type GitRef, WORKDIR_SHA } from "./gitApi";
import { assignLanes, laneColorVar, type GraphRow } from "./laneGraph";

const ROW_H_DESKTOP = 26;
const ROW_H_TOUCH = 44;
const COL_W = 14;
const DOT_R = 4;
const LEFT_PAD = 10;

interface GraphViewProps {
  commits: GitCommit[];
  selectedSha: string | null;
  onSelect: (sha: string) => void;
  hasMore: boolean;
  loadingMore: boolean;
  onLoadMore: () => void;
  dirty?: boolean;
  headSha?: string | null;
  branch?: string | null;
  touch?: boolean;
}

export function GraphView({
  commits,
  selectedSha,
  onSelect,
  hasMore,
  loadingMore,
  onLoadMore,
  dirty,
  headSha,
  branch,
  touch,
}: GraphViewProps) {
  const rowH = touch ? ROW_H_TOUCH : ROW_H_DESKTOP;
  const rows = useMemo(
    () => assignLanes(commits.map((c) => ({ sha: c.sha, parents: c.parents }))),
    [commits],
  );
  const byIndex = useMemo(() => {
    const m = new Map<string, number>();
    rows.forEach((r, i) => m.set(r.sha, i));
    return m;
  }, [rows]);

  const railWidth = useMemo(() => {
    const maxLane = rows.reduce(
      (mx, r) => Math.max(mx, r.col, r.lanesBefore.length - 1),
      0,
    );
    return LEFT_PAD * 2 + (maxLane + 1) * COL_W;
  }, [rows]);

  const commitBySha = useMemo(() => {
    const m = new Map<string, GitCommit>();
    for (const c of commits) m.set(c.sha, c);
    return m;
  }, [commits]);
  const headRow = useMemo(
    () => rows.find((r) => r.sha === headSha) ?? rows[0],
    [rows, headSha],
  );
  const headCommit = headSha ? commitBySha.get(headSha) : null;
  const showWorkdirRow = (dirty || !!branch) && !!headRow;

  return (
    <div className="absolute inset-0 overflow-auto">
      {showWorkdirRow && headRow && (
        <WorkdirRow
          railWidth={railWidth}
          rowH={rowH}
          col={headRow.col}
          branch={branch}
          dirty={!!dirty}
          headRefs={headCommit?.refs ?? []}
          selected={selectedSha === WORKDIR_SHA}
          onSelect={() => onSelect(WORKDIR_SHA)}
        />
      )}
      {rows.map((row, i) => {
        const commit = commitBySha.get(row.sha)!;
        return (
          <GraphRowView
            key={row.sha}
            row={row}
            nextRow={rows[i + 1]}
            commit={commit}
            railWidth={railWidth}
            rowH={rowH}
            selected={row.sha === selectedSha}
            onSelect={onSelect}
            childRowOf={byIndex}
            forceIncoming={showWorkdirRow && row.sha === headSha}
          />
        );
      })}
      {hasMore && (
        <div className="flex justify-center py-3">
          <button
            type="button"
            disabled={loadingMore}
            onClick={onLoadMore}
            className="ct-btn ct-btn-ghost ct-btn-sm"
          >
            {loadingMore ? "Loading…" : "Load more"}
          </button>
        </div>
      )}
    </div>
  );
}

function laneX(col: number): number {
  return LEFT_PAD + col * COL_W + COL_W / 2;
}

interface GraphRowViewProps {
  row: GraphRow;
  nextRow: GraphRow | undefined;
  commit: GitCommit;
  railWidth: number;
  rowH: number;
  selected: boolean;
  onSelect: (sha: string) => void;
  childRowOf: Map<string, number>;
  forceIncoming?: boolean;
}

function GraphRowView({
  row,
  nextRow,
  commit,
  railWidth,
  rowH,
  selected,
  onSelect,
  forceIncoming,
}: GraphRowViewProps) {
  // Lanes occupied AFTER this row routes its parents — what the next row sees.
  const lanesAfter = nextRow ? nextRow.lanesBefore : [];

  // Pass-through verticals: any lane that is occupied both before and after,
  // and isn't the commit's own column, draws a straight rail through the row.
  const passThrough: number[] = [];
  for (let c = 0; c < row.lanesBefore.length; c++) {
    if (c === row.col) continue;
    if (row.lanesBefore[c] != null && lanesAfter[c] != null) passThrough.push(c);
  }

  // Parent edges: from the dot down to each parent's lane in the next row.
  const parentCols: number[] = [];
  for (const parent of row.parents) {
    const lane = lanesAfter.indexOf(parent);
    if (lane !== -1 && !parentCols.includes(lane)) parentCols.push(lane);
  }

  const dotX = laneX(row.col);
  const midY = rowH / 2;

  return (
    <div
      className={`ct-git-row${selected ? " is-selected" : ""}`}
      style={{ height: rowH }}
      onClick={() => onSelect(row.sha)}
    >
      <svg
        className="ct-git-rail"
        width={railWidth}
        height={rowH}
        style={{ flex: `0 0 ${railWidth}px` }}
      >
        {passThrough.map((c) => (
          <line
            key={`pt-${c}`}
            x1={laneX(c)}
            y1={0}
            x2={laneX(c)}
            y2={rowH}
            stroke={laneColorVar(c)}
            strokeWidth={1.5}
          />
        ))}
        {/* incoming rail from top into this commit's lane */}
        {(forceIncoming || row.lanesBefore[row.col] != null) && (
          <line
            x1={dotX}
            y1={0}
            x2={dotX}
            y2={midY}
            stroke={laneColorVar(row.col)}
            strokeWidth={1.5}
          />
        )}
        {parentCols.map((c) => {
          const px = laneX(c);
          if (c === row.col) {
            return (
              <line
                key={`pe-${c}`}
                x1={dotX}
                y1={midY}
                x2={px}
                y2={rowH}
                stroke={laneColorVar(c)}
                strokeWidth={1.5}
              />
            );
          }
          const cy = midY + (rowH - midY) / 2;
          return (
            <path
              key={`pe-${c}`}
              d={`M ${dotX} ${midY} C ${dotX} ${cy}, ${px} ${midY}, ${px} ${rowH}`}
              fill="none"
              stroke={laneColorVar(c)}
              strokeWidth={1.5}
            />
          );
        })}
        <circle
          cx={dotX}
          cy={midY}
          r={DOT_R}
          fill={laneColorVar(row.col)}
          stroke="var(--ct-bg, #0d1117)"
          strokeWidth={1.5}
        />
      </svg>
      <div className="ct-git-meta">
        {commit.refs.map((ref) => (
          <RefChip key={`${ref.kind}-${ref.name}`} refItem={ref} />
        ))}
        <span className="ct-git-subject">{commit.subject}</span>
        <span className="ct-git-author">{commit.author}</span>
        <span className="ct-git-sha">{commit.sha.slice(0, 7)}</span>
        <span className="ct-git-date">{relativeDate(commit.date)}</span>
      </div>
    </div>
  );
}

// Pinned pseudo-row above HEAD: amber dot, no lane rails, distinct styling.
function WorkdirRow({
  railWidth,
  rowH,
  col,
  branch,
  dirty,
  headRefs,
  selected,
  onSelect,
}: {
  railWidth: number;
  rowH: number;
  col: number;
  branch?: string | null;
  dirty: boolean;
  headRefs: GitRef[];
  selected: boolean;
  onSelect: () => void;
}) {
  const dotX = laneX(col);
  const midY = rowH / 2;
  const refs = workdirRefs(branch, headRefs);
  return (
    <div
      className={`ct-git-row ct-git-row--workdir${selected ? " is-selected" : ""}`}
      style={{ height: rowH }}
      onClick={onSelect}
    >
      <svg
        className="ct-git-rail"
        width={railWidth}
        height={rowH}
        style={{ flex: `0 0 ${railWidth}px` }}
      >
        <line
          x1={dotX}
          y1={midY}
          x2={dotX}
          y2={rowH}
          stroke={laneColorVar(col)}
          strokeWidth={1.5}
        />
        <circle
          cx={dotX}
          cy={midY}
          r={DOT_R}
          fill="var(--ct-bg, #0d1117)"
          stroke={laneColorVar(col)}
          strokeWidth={1.8}
        />
      </svg>
      <div className="ct-git-meta">
        {refs.map((ref) => (
          <RefChip key={`${ref.kind}-${ref.name}`} refItem={ref} />
        ))}
        <span className="ct-git-subject ct-git-workdir-label">
          {dirty ? "Uncommitted changes" : "Working tree clean"}
        </span>
      </div>
    </div>
  );
}

function workdirRefs(branch: string | null | undefined, headRefs: GitRef[]): GitRef[] {
  const refs: GitRef[] = [];
  if (branch) refs.push({ kind: "branch", name: branch });
  if (headRefs.some((ref) => ref.kind === "head")) refs.push({ kind: "head", name: "HEAD" });
  return refs;
}

function RefChip({ refItem }: { refItem: GitRef }) {
  const label = refItem.kind === "head" ? "HEAD" : refItem.name;
  return <span className={`ct-git-chip kind-${refItem.kind}`}>{label}</span>;
}

function relativeDate(iso: string): string {
  const then = new Date(iso).getTime();
  if (Number.isNaN(then)) return "";
  const sec = Math.floor((Date.now() - then) / 1000);
  if (sec < 60) return `${sec}s`;
  const min = Math.floor(sec / 60);
  if (min < 60) return `${min}m`;
  const hr = Math.floor(min / 60);
  if (hr < 24) return `${hr}h`;
  const day = Math.floor(hr / 24);
  if (day < 30) return `${day}d`;
  const mo = Math.floor(day / 30);
  if (mo < 12) return `${mo}mo`;
  return `${Math.floor(mo / 12)}y`;
}
