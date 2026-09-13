import type { FileDiff, FileDiffHunk, ToolEvent } from "@codeterm/chat-core";
import { ChevronRight } from "lucide-react";
import { useId, useMemo, useState, type CSSProperties, type ReactNode } from "react";
import { Prism as SyntaxHighlighter, createElement } from "react-syntax-highlighter";
import { RawCommandDisclosure } from "../codeterm/RawCommandDisclosure";
import { knownLanguageFromPath } from "../codeLanguage";
import { DIFF_HIGHLIGHT_MAX_LINES } from "../MarkdownBody";
import { SYNTAX_HIGHLIGHT_ROOT_CLASS, themeAwarePrismStyle } from "../syntaxTheme";
import { ToolOutputContent } from "../ToolOutputContent";
import { toolKindVisual } from "./toolKindVisual";

type LineTone = "add" | "del" | "context";

const TONE_STYLE: Record<LineTone, CSSProperties> = {
  add: { background: "var(--ct-diff-add-bg)" },
  del: { background: "var(--ct-diff-del-bg)" },
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

function rawInput(event: ToolEvent): string {
  const input = event.message.toolInput;
  if (input && Object.keys(input).length > 0) return JSON.stringify(input, null, 2);
  return event.detail || event.summary || "";
}

interface HighlightNode { type: string; value?: string | number; children?: HighlightNode[] }

// Prism rows carry their line's trailing "\n"; a row must expose the exact source line.
function withoutTrailingNewline<T extends HighlightNode>(node: T): T {
  if (node.type === "text") {
    return typeof node.value === "string" && node.value.endsWith("\n") ? { ...node, value: node.value.slice(0, -1) } : node;
  }
  const children = node.children;
  if (!children?.length) return node;
  return { ...node, children: [...children.slice(0, -1), withoutTrailingNewline(children[children.length - 1])] };
}

function DiffLine({ line, content }: { line: string; content: ReactNode }) {
  const tone = lineTone(line);
  return (
    <div
      data-testid="file-diff-line"
      data-tone={tone}
      className="min-h-[1.4em] whitespace-pre px-3 font-mono text-[11.5px]"
      style={{ ...TONE_STYLE[tone], color: "var(--ct-fg)" }}
    ><span data-testid="file-diff-marker" style={{ color: MARKER_COLOR[tone] }}>{line.charAt(0)}</span>{content}</div>
  );
}

/** Highlights only the code after each marker, so the marker column keeps its add/remove tint. */
function HunkLines({ hunk, language }: { hunk: FileDiffHunk; language: string }) {
  const code = useMemo(() => hunk.lines.map((line) => line.substring(1)).join("\n"), [hunk.lines]);
  if (language === "text" || hunk.lines.length === 0 || hunk.lines.length > DIFF_HIGHLIGHT_MAX_LINES) {
    return <>{hunk.lines.map((line, index) => <DiffLine key={index} line={line} content={line.substring(1)} />)}</>;
  }
  return (
    <SyntaxHighlighter
      language={language}
      style={themeAwarePrismStyle}
      useInlineStyles={false}
      PreTag="div"
      CodeTag="div"
      customStyle={{ margin: 0, padding: 0, background: "transparent", fontSize: "inherit", lineHeight: "inherit" }}
      codeTagProps={{ style: { fontFamily: "inherit" } }}
      renderer={({ rows, stylesheet, useInlineStyles }) => hunk.lines.map((line, index) => (
        <DiffLine
          key={index}
          line={line}
          content={rows[index] ? createElement({ node: withoutTrailingNewline(rows[index]), stylesheet, useInlineStyles, key: `c${index}` }) : line.substring(1)}
        />
      ))}
    >
      {code}
    </SyntaxHighlighter>
  );
}

function FileEditCard({ event, diff, showResult, initiallyExpanded }: { event: ToolEvent; diff: FileDiff; showResult: boolean; initiallyExpanded: boolean }) {
  const [expanded, setExpanded] = useState(initiallyExpanded);
  const regionId = useId();
  const visual = toolKindVisual.edit;
  const name = diff.movePath ? `${diff.path} → ${diff.movePath}` : diff.path;
  const heading = `${name} +${diff.additions} −${diff.deletions}`;
  const language = knownLanguageFromPath(diff.movePath ?? diff.path);
  const failed = event.status === "error";
  return (
    <section
      data-testid="file-diff-card"
      data-tool-status={event.status}
      aria-label={`${visual.label} ${heading}`}
      className="min-w-0 overflow-hidden rounded-md border-l-2"
      style={{
        borderLeftColor: failed ? "var(--ct-red)" : "var(--ct-border)",
        background: "color-mix(in srgb, var(--ct-surface) 60%, transparent)",
      }}
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
        <span data-testid="file-diff-label" className="shrink-0 font-semibold" style={{ color: visual.color }}>{visual.label}</span>
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
          <div key={hunkIndex} data-testid="file-diff-hunk" className={`${SYNTAX_HIGHLIGHT_ROOT_CLASS} min-w-max`}>
            <div data-testid="file-diff-hunk-header" className="px-3 py-0.5 font-mono text-[10.5px]" style={{ color: "var(--ct-muted-fg)" }}>{hunkHeader(hunk)}</div>
            <HunkLines hunk={hunk} language={language} />
          </div>
        ))}
        {expanded && showResult && failed && event.result && (
          <div data-testid="toolcard-result" className="px-3 pt-1"><ToolOutputContent text={event.result} maxHeight="12rem" /></div>
        )}
        {expanded && <div className="px-3 pt-1"><RawCommandDisclosure raw={rawInput(event)} /></div>}
      </div>
    </section>
  );
}

/** One edit card per changed file, built from the source's structured diff. */
export function FileDiffCards({ event, initiallyExpanded = false }: { event: ToolEvent; initiallyExpanded?: boolean }) {
  const diffs = event.fileDiffs ?? [];
  return (
    <div data-testid="file-diff-cards" className="mx-auto mb-1.5 flex w-full max-w-4xl min-w-0 flex-col gap-1">
      {diffs.map((diff, index) => <FileEditCard key={`${index}:${diff.path}`} event={event} diff={diff} showResult={index === 0} initiallyExpanded={initiallyExpanded} />)}
    </div>
  );
}
