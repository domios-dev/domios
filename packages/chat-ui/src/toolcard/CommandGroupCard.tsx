import { toolCommand, type ShellScript, type ToolEvent } from "@codeterm/chat-core";
import { ChevronRight } from "lucide-react";
import { useId, useState } from "react";
import { Prism as SyntaxHighlighter } from "react-syntax-highlighter";
import { commandGroupSummary, type CommandRow } from "../codeterm/commandShape";
import { RawCommandDisclosure } from "../codeterm/RawCommandDisclosure";
import { DIFF_HIGHLIGHT_MAX_LINES } from "../MarkdownBody";
import { SYNTAX_HIGHLIGHT_ROOT_CLASS, themeAwarePrismStyle } from "../syntaxTheme";
import { ToolOutputContent } from "../ToolOutputContent";
import { CodetermActionDetail, type CodetermNavigationProps } from "./CodetermActionCard";
import { COMMAND_TREE_CAP, CommandTree, commandCallKey, openSegmentOf, rememberOpenSegment } from "./CommandTree";
import { StatusGlyph, statusName } from "./ToolActivityStream";
import { ToolOutcomeFacts } from "./ToolOutcomeFacts";

interface CommandGroupCardProps extends CodetermNavigationProps {
  event: ToolEvent;
  rows: CommandRow[];
  expanded: boolean;
  onToggle: (next: boolean) => void;
}

function ScriptBody({ script }: { script: ShellScript }) {
  const plain = script.language === "text" || script.lineCount > DIFF_HIGHLIGHT_MAX_LINES;
  return (
    <div
      data-testid="chat-command-script-body"
      data-language={script.language}
      className={`${SYNTAX_HIGHLIGHT_ROOT_CLASS} max-h-72 overflow-auto px-2 py-1 font-mono text-[11.5px]`}
      style={{ background: "var(--ct-surface)" }}
    >
      {plain ? <pre className="m-0 whitespace-pre" style={{ color: "var(--ct-fg)" }}>{script.body}</pre> : (
        <SyntaxHighlighter
          language={script.language}
          style={themeAwarePrismStyle}
          useInlineStyles={false}
          PreTag="pre"
          customStyle={{ margin: 0, padding: 0, background: "transparent", fontSize: "inherit", lineHeight: "inherit" }}
          codeTagProps={{ style: { fontFamily: "inherit" } }}
        >
          {script.body}
        </SyntaxHighlighter>
      )}
    </div>
  );
}

function SegmentDetail({ event, row, index, navigation }: { event: ToolEvent; row: CommandRow; index: number; navigation: CodetermNavigationProps }) {
  if (row.role === "script") return <ScriptBody script={row.script} />;
  const text = (
    <pre data-testid="chat-command-group-segment-text" className="m-0 max-h-48 overflow-auto whitespace-pre-wrap break-words font-mono text-[11px]" style={{ color: "var(--ct-muted-fg)" }}>
      {row.segment.text}
    </pre>
  );
  if (row.role === "command") return text;
  const segment: ToolEvent = {
    id: `${event.id}:segment:${index}`,
    phase: "call",
    status: event.status,
    kind: "codeterm",
    summary: row.segment.text,
    detail: row.segment.text,
    message: event.message,
  };
  return <CodetermActionDetail event={segment} invocation={row.invocation} fallback={text} {...navigation} />;
}

/** One command call made of several shell segments: a compact tree line per segment, one outcome for the whole call. */
export function CommandGroupCard({ event, rows, expanded, onToggle, ...navigation }: CommandGroupCardProps) {
  const callKey = commandCallKey(event);
  const [openRow, setOpenRowState] = useState<number | null>(() => openSegmentOf(callKey));
  const regionId = useId();
  const toggleRow = (index: number) => {
    const next = openRow === index ? null : index;
    rememberOpenSegment(callKey, next);
    setOpenRowState(next);
  };
  const title = event.description || commandGroupSummary(rows);
  const countLabel = `${rows.length} commands`;
  const failed = event.status === "error";
  const capped = !expanded && (openRow ?? -1) < COMMAND_TREE_CAP;

  return (
    <section
      data-testid="chat-command-group"
      data-tool-status={event.status}
      aria-label={`Command, ${countLabel}`}
      className="mx-auto mb-1.5 w-full max-w-4xl min-w-0 overflow-hidden rounded-md border-l-2"
      style={{
        borderLeftColor: failed ? "var(--ct-red)" : event.status === "pending" ? "var(--ct-accent)" : "var(--ct-border)",
        background: "color-mix(in srgb, var(--ct-surface) 60%, transparent)",
      }}
    >
      <button
        type="button"
        data-testid="chat-command-group-toggle"
        aria-expanded={expanded}
        aria-controls={regionId}
        aria-label={`${expanded ? "Hide" : "Show"} output of ${title}`}
        onClick={() => onToggle(!expanded)}
        className="flex w-full min-w-0 items-center gap-2 px-3 py-1.5 text-left text-xs"
      >
        <span data-testid="chat-command-group-status" aria-label={statusName(event.status)} className="flex shrink-0">
          <StatusGlyph status={event.status} />
        </span>
        <span data-testid="chat-command-group-title" title={title} className="min-w-0 flex-1 truncate font-mono text-[12px]" style={{ color: "var(--ct-fg)" }}>
          {title}
        </span>
        <ToolOutcomeFacts event={event} />
        <span className="shrink-0 font-mono text-[10.5px]" style={{ color: "var(--ct-muted-fg)" }}>{countLabel}</span>
        <ChevronRight aria-hidden="true" size={14} className={`shrink-0 ${expanded ? "rotate-90" : ""}`} style={{ color: "var(--ct-muted-fg)" }} />
      </button>
      <CommandTree
        rows={rows}
        cap={capped ? COMMAND_TREE_CAP : undefined}
        openIndex={openRow}
        onLine={toggleRow}
        onMore={() => onToggle(true)}
        renderDetail={(index) => <SegmentDetail event={event} row={rows[index]} index={index} navigation={navigation} />}
      />
      <div id={regionId} role="region" aria-label="Command output" hidden={!expanded} className="px-3 pb-2">
        {expanded && (
          <RawCommandDisclosure
            raw={toolCommand(event).command}
            details={event.status === "pending"
              ? <div data-testid="toolcard-spinner" className="font-mono text-[11px]" style={{ color: "var(--ct-muted-fg)" }}>Running…</div>
              : event.result && <div data-testid="toolcard-result"><ToolOutputContent text={event.result} maxHeight="12rem" /></div>}
          />
        )}
      </div>
    </section>
  );
}
