import type { FileDiff, FileDiffHunk } from "@codeterm/chat-core";
import { ChevronRight } from "lucide-react";
import { useId, useState, type CSSProperties } from "react";

type LineTone = "add" | "del" | "context";

const TONE_STYLE: Record<LineTone, CSSProperties> = {
  add: { background: "var(--ct-diff-add-bg, color-mix(in srgb, var(--ct-green) 12%, transparent))" },
  del: { background: "var(--ct-diff-del-bg, color-mix(in srgb, var(--ct-red) 12%, transparent))" },
  context: {},
};

const MARKER_COLOR: Record<LineTone, string> = {
  add: "var(--ct-green)",
  del: "var(--ct-red)",
  context: "var(--ct-muted-fg)",
};

function lineTone(line: string): LineTone {
  const marker = line.charAt(0);
  return marker === "+" ? "add" : marker === "-" ? "del" : "context";
}

function hunkHeader(hunk: FileDiffHunk): string {
  return `@@ -${hunk.oldStart},${hunk.oldLines} +${hunk.newStart},${hunk.newLines} @@`;
}

function hunkLabel(count: number): string {
  if (count === 0) return "empty";
  return count === 1 ? "1 hunk" : `${count} hunks`;
}

function FileDiffCard({ diff }: { diff: FileDiff }) {
  const [expanded, setExpanded] = useState(false);
  const regionId = useId();
  const name = diff.movePath ? `${diff.path} → ${diff.movePath}` : diff.path;
  const heading = `${name} +${diff.additions} −${diff.deletions}`;
  return (
    <section
      data-testid="file-diff-card"
      aria-label={heading}
      className="min-w-0 overflow-hidden rounded-md border-l-2"
      style={{ borderLeftColor: "var(--ct-border)", background: "color-mix(in srgb, var(--ct-surface) 60%, transparent)" }}
    >
      <button
        type="button"
        data-testid="file-diff-toggle"
        aria-expanded={expanded}
        aria-controls={regionId}
        aria-label={`${expanded ? "Hide" : "Show"} diff for ${heading}`}
        onClick={() => setExpanded(!expanded)}
        className="flex w-full min-w-0 items-center gap-2 px-3 py-1.5 text-left font-mono text-[12px] focus-visible:outline focus-visible:outline-1 focus-visible:outline-[var(--ct-accent)]"
      >
        <ChevronRight aria-hidden="true" size={14} className={`shrink-0 ${expanded ? "rotate-90" : ""}`} style={{ color: "var(--ct-muted-fg)" }} />
        <span data-testid="file-diff-header" className="flex min-w-0 flex-1 items-baseline gap-2">
          <span className="min-w-0 truncate" title={name} style={{ color: "var(--ct-fg)" }}>{name}</span>{" "}
          <span className="shrink-0" style={{ color: "var(--ct-green)" }}>+{diff.additions}</span>{" "}
          <span className="shrink-0" style={{ color: "var(--ct-red)" }}>−{diff.deletions}</span>
        </span>
        <span className="shrink-0 text-[10.5px]" style={{ color: "var(--ct-muted-fg)" }}>{hunkLabel(diff.hunks.length)}</span>
      </button>
      <div id={regionId} role="region" aria-label={`Diff for ${name}`} hidden={!expanded} className="overflow-x-auto pb-1.5">
        {expanded && diff.hunks.length === 0 && (
          <div className="px-3 font-mono text-[11px]" style={{ color: "var(--ct-muted-fg)" }}>Empty file</div>
        )}
        {expanded && diff.hunks.map((hunk, hunkIndex) => (
          <div key={hunkIndex} data-testid="file-diff-hunk" className="min-w-max">
            <div data-testid="file-diff-hunk-header" className="px-3 py-0.5 font-mono text-[10.5px]" style={{ color: "var(--ct-muted-fg)" }}>{hunkHeader(hunk)}</div>
            {hunk.lines.map((line, lineIndex) => {
              const tone = lineTone(line);
              return (
                <div
                  key={lineIndex}
                  data-testid="file-diff-line"
                  data-tone={tone}
                  className="min-h-[1.4em] whitespace-pre px-3 font-mono text-[11.5px]"
                  style={{ ...TONE_STYLE[tone], color: "var(--ct-fg)" }}
                ><span style={{ color: MARKER_COLOR[tone] }}>{line.charAt(0)}</span>{line.substring(1)}</div>
              );
            })}
          </div>
        ))}
      </div>
    </section>
  );
}

export function FileDiffCards({ diffs }: { diffs: FileDiff[] }) {
  return (
    <div data-testid="file-diff-cards" className="mx-auto mb-1.5 flex w-full max-w-4xl min-w-0 flex-col gap-1">
      {diffs.map((diff, index) => <FileDiffCard key={`${index}:${diff.path}`} diff={diff} />)}
    </div>
  );
}
