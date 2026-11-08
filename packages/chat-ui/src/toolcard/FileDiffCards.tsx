import { ExpandableReader } from "../ExpandableReader";
import { type FileDiff, type FileDiffHunk, type ToolEvent } from "@codeterm/chat-core";
import { Copy, FileDiff as FileDiffIcon, FileText, SquareTerminal } from "lucide-react";
import { CardChevron } from "../CompactCard";
import { RowMenuIsolate } from "../messageMenu";
import { useId, useMemo, useState, type CSSProperties, type ReactNode } from "react";
import { SyntaxHighlighter, createElement } from "../syntaxHighlighter";
import { BoundedMessageContent } from "../BoundedMessageContent";
import { ChatCardMenu, copyPlainText } from "../ChatCardMenu";
import { useChatBlockHost, type ChatCardMenuItem } from "../blocks/ChatBlockHost";
import { FloatingTooltip } from "../FloatingTooltip";
import { knownLanguageFromPath } from "../codeLanguage";
import { DIFF_HIGHLIGHT_MAX_LINES } from "../MarkdownBody";
import { SYNTAX_HIGHLIGHT_ROOT_CLASS, themeAwarePrismStyle } from "../syntaxTheme";
import { ToolOutputContent } from "../ToolOutputContent";
import { toolKindVisual } from "./toolKindVisual";
import { rawToolPayload } from "./toolParsing";

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

export function changedLines(hunk: FileDiffHunk): string {
  const start = hunk.newLines > 0 ? hunk.newStart : hunk.oldStart;
  const count = hunk.newLines > 0 ? hunk.newLines : hunk.oldLines;
  return count > 1 ? `Lines ${start}–${start + count - 1}` : `Line ${start}`;
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
  const highlightLanguage = language === "html"
    && /(?:^|\n)\s*(?:(?:async\s+)?function\s+[\w$]+\s*\(|(?:const|let|var)\s+[\w$]+\s*[=,;]|(?:document|window)\.)/.test(code.slice(0, 6000))
    && !/<\/?(?:script|[a-z][\w-]*)(?:\s|>)/i.test(code.slice(0, 6000)) ? "javascript" : language;
  return (
    <SyntaxHighlighter
      language={highlightLanguage}
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

/** One hunk's highlighted lines without a card, for a stream branch. */
export function DiffHunkLines({ hunk, path }: { hunk: FileDiffHunk; path: string }) {
  return (
    <div data-testid="chat-branch-hunk" className={`${SYNTAX_HIGHLIGHT_ROOT_CLASS} min-w-max`}>
      <HunkLines hunk={hunk} language={knownLanguageFromPath(path)} />
    </div>
  );
}

type HunkActions = (diff: FileDiff, hunk: FileDiffHunk) => ReactNode;

function FilePathLabel({ path }: { path: string }) {
  const parts = path.replace(/\\/g, "/").split("/").filter(Boolean);
  return <>
    {parts.length > 1 && <span style={{ color: "var(--ct-muted-fg)" }}>{parts[parts.length - 2]}/</span>}
    <span>{parts[parts.length - 1] ?? path}</span>
  </>;
}

function FileEditCard({ event, diff, showResult, initiallyExpanded, renderHunkActions, rawCommand }: { event: ToolEvent; diff: FileDiff; showResult: boolean; initiallyExpanded: boolean; renderHunkActions?: HunkActions; rawCommand: boolean }) {
  const [expanded, setExpanded] = useState(initiallyExpanded);
  const [technicalOpen, setTechnicalOpen] = useState(false);
  const host = useChatBlockHost();
  const regionId = useId();
  const visual = toolKindVisual.edit;
  const Glyph = visual.glyph;
  const name = diff.movePath ? `${diff.path} → ${diff.movePath}` : diff.path;
  const heading = `${name} +${diff.additions} −${diff.deletions}`;
  const language = knownLanguageFromPath(diff.movePath ?? diff.path);
  const failed = event.status === "error";
  const path = diff.movePath ?? diff.path;
  const menu: ChatCardMenuItem[] = [
    { label: "Copy path", icon: <Copy size={14} aria-hidden />, onSelect: () => copyPlainText(path) },
  ];
  if (host.openFileChange) {
    const open = host.openFileChange;
    menu.push({ label: "View changes", icon: <FileDiffIcon size={14} aria-hidden />, onSelect: () => open(path, "diff") });
    if (diff.kind !== "delete") menu.push({ label: "View file", icon: <FileText size={14} aria-hidden />, onSelect: () => open(path, "file") });
  }
  if (rawCommand) menu.push({ label: "Details", kind: "details", icon: <SquareTerminal size={14} aria-hidden />, checked: technicalOpen,
    onSelect: () => { setTechnicalOpen((open) => !open); setExpanded(true); } });
  return (
    <ExpandableReader label={heading} openerPlacement="header" ownsMenu>{(fullscreen, opener, _openReader, readerItems) => (
    <section
      data-testid="file-diff-card"
      data-tool-status={event.status}
      aria-label={`${visual.label} ${heading}`}
      className="ct-card min-w-0 overflow-hidden rounded-md"
      style={{
        background: "color-mix(in srgb, var(--ct-surface) 60%, transparent)",
      }}
    >
      <div className="ct-card__header flex min-w-0 items-center gap-2 py-1.5 pl-3 text-[12px]">
      <button
        type="button"
        data-testid="file-diff-toggle"
        aria-expanded={expanded}
        aria-controls={regionId}
        aria-label={`${expanded ? "Hide" : "Show"} diff for ${heading}`}
        onClick={() => setExpanded(!expanded)}
        className="flex min-w-0 flex-1 items-center gap-2 text-left font-mono focus-visible:outline focus-visible:outline-1 focus-visible:outline-[var(--ct-accent)]"
      >
        <Glyph aria-hidden size={16} className="shrink-0" style={{ color: visual.color }} />
        <span data-testid="file-diff-label" className="sr-only">{visual.label}</span>
        <span data-testid="file-diff-header" className="flex min-w-0 flex-1 items-baseline gap-2">
          <FloatingTooltip content={name} focusable={false} className="min-w-0 truncate" tooltipStyle={{ maxWidth: "min(28rem, calc(100vw - 24px))" }}>
            <span className="truncate" style={{ color: "var(--ct-fg)" }}><FilePathLabel path={diff.path} />{diff.movePath && <> → <FilePathLabel path={diff.movePath} /></>}</span>
          </FloatingTooltip>{" "}
          <span className="shrink-0" style={{ color: "var(--ct-green)" }}>+{diff.additions}</span>{" "}
          <span className="shrink-0" style={{ color: "var(--ct-red)" }}>−{diff.deletions}</span>
        </span>
      </button>
      {opener}
      <span className="ct-tool-status-slot"><ChatCardMenu label={`Actions for ${name}`} items={[...readerItems, ...menu]} joinRow /></span>
      <CardChevron expanded={expanded} label={`${expanded ? "Collapse" : "Expand"} diff for ${name}`} onToggle={() => setExpanded(!expanded)} />
      </div>
      <div id={regionId} role="region" aria-label={`Diff for ${name}`} hidden={!expanded && !fullscreen} className="overflow-x-auto pb-1.5">
        {(expanded || fullscreen) && diff.hunks.length === 0 && (
          <div className="px-3 font-mono text-[11px]" style={{ color: "var(--ct-muted-fg)" }}>Empty file</div>
        )}
        {(expanded || fullscreen) && diff.hunks.map((hunk, hunkIndex) => (
          <div key={hunkIndex} data-testid="file-diff-hunk" className={`${SYNTAX_HIGHLIGHT_ROOT_CLASS} group/hunk min-w-max`}>
            <div data-testid="file-diff-hunk-header" className="flex items-center gap-2 px-3 py-0.5 font-mono text-[10.5px]" style={{ color: "var(--ct-muted-fg)" }}>
              <span>{changedLines(hunk)}</span>
              {renderHunkActions && <span className="flex shrink-0 items-center gap-1 opacity-0 transition-opacity group-hover/hunk:opacity-100 group-focus-within/hunk:opacity-100 [@media(hover:none)]:opacity-100">{renderHunkActions(diff, hunk)}</span>}
            </div>
            <HunkLines hunk={hunk} language={language} />
          </div>
        ))}
        {(expanded || fullscreen) && showResult && failed && event.result && (
          <div data-testid="toolcard-result" className="px-3 pt-1"><ToolOutputContent text={event.result} maxHeight="12rem" /></div>
        )}
        {(expanded || fullscreen) && technicalOpen && rawCommand && <BoundedMessageContent as="pre" data-testid="message-raw-content" collapsedHeight="12rem" className="m-0 whitespace-pre-wrap break-words px-3 pt-1 font-mono text-xs" style={{ color: "var(--ct-muted-fg)" }}>{rawToolPayload(event)}</BoundedMessageContent>}
      </div>
    </section>
    )}</ExpandableReader>
  );
}

/** One edit card per changed file, built from the source's structured diff. */
export function FileDiffCards({ event, initiallyExpanded = false, renderHunkActions, rawCommand = true }: { event: ToolEvent; initiallyExpanded?: boolean; renderHunkActions?: HunkActions; rawCommand?: boolean }) {
  const diffs = event.fileDiffs ?? [];
  return (
    <div data-testid="file-diff-cards" className="mx-auto mb-1.5 flex w-full max-w-4xl min-w-0 flex-col gap-1">
      {diffs.length === 1
        ? <FileEditCard event={event} diff={diffs[0]} showResult initiallyExpanded={initiallyExpanded} renderHunkActions={renderHunkActions} rawCommand={rawCommand} />
        : <RowMenuIsolate>{diffs.map((diff, index) => <FileEditCard key={`${index}:${diff.path}`} event={event} diff={diff} showResult={index === 0} initiallyExpanded={initiallyExpanded} renderHunkActions={renderHunkActions} rawCommand={rawCommand} />)}</RowMenuIsolate>}
    </div>
  );
}
