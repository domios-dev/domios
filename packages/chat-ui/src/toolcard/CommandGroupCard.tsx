import { ExpandableReader } from "../ExpandableReader";
import { BoundedMessageContent } from "../BoundedMessageContent";
import type { ShellScript, ToolEvent } from "@codeterm/chat-core";
import { ChatCardMenu } from "../ChatCardMenu";
import { SquareTerminal } from "lucide-react";
import { CardChevron } from "../CompactCard";
import { useId, useState } from "react";
import { SyntaxHighlighter } from "../syntaxHighlighter";
import { commandGroupSummary, type CommandRow } from "../codeterm/commandShape";
import { DIFF_HIGHLIGHT_MAX_LINES } from "../MarkdownBody";
import { SYNTAX_HIGHLIGHT_ROOT_CLASS, themeAwarePrismStyle } from "../syntaxTheme";
import { ToolOutputContent } from "../ToolOutputContent";
import { CodetermActionDetail, type CodetermNavigationProps } from "./CodetermActionCard";
import { COMMAND_TREE_CAP, CommandTree, commandCallKey, openSegmentOf, rememberOpenSegment } from "./CommandTree";
import { StatusGlyph, statusName } from "./ToolActivityStream";
import { ToolOutcomeFacts } from "./ToolOutcomeFacts";
import { shellFileOperationPreview } from "./ShellFileOperations";

interface CommandGroupCardProps extends CodetermNavigationProps {
  event: ToolEvent;
  rows: CommandRow[];
  expanded: boolean;
  onToggle: (next: boolean) => void;
}

function ScriptBody({ script }: { script: ShellScript }) {
  const plain = script.language === "text" || script.lineCount > DIFF_HIGHLIGHT_MAX_LINES;
  return (
    <BoundedMessageContent collapsedHeight={"18rem"}
      data-testid="chat-command-script-body"
      data-language={script.language}
      className={`${SYNTAX_HIGHLIGHT_ROOT_CLASS} px-2 py-1 font-mono text-[11.5px]`}
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
    </BoundedMessageContent>
  );
}

function SegmentDetail({ event, row, index, navigation }: { event: ToolEvent; row: CommandRow; index: number; navigation: CodetermNavigationProps }) {
  if (row.role === "script") return <ScriptBody script={row.script} />;
  const text = (
    <BoundedMessageContent collapsedHeight={"12rem"} as="pre" data-testid="chat-command-group-segment-text" className="m-0 whitespace-pre-wrap break-words font-mono text-[11px]" style={{ color: "var(--ct-muted-fg)" }}>
      {row.segment.text}
    </BoundedMessageContent>
  );
  if (row.event) return <>
    {text}
    <span aria-label={statusName(row.event.status)}><StatusGlyph status={row.event.status} /></span>
    <ToolOutcomeFacts event={row.event} />
    {row.event.result && <ToolOutputContent text={row.event.result} maxHeight="12rem" />}
  </>;
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
  const regionId = useId();
  const [technicalOpen, setTechnicalOpen] = useState(false);
  const title = event.description || (event.commandBatch ? event.title : undefined) || commandGroupSummary(rows);
  const countLabel = `${rows.length} commands`;

  return (
    <ExpandableReader label={title} openerPlacement="header" ownsMenu>{(fullscreen, opener, _openReader, readerItems) => (
    <section

      data-testid="chat-command-group"
      data-tool-status={event.status}
      data-expanded={expanded || fullscreen}
      aria-label={`Command, ${countLabel}`}
      className="ct-card ct-command-inline mx-auto mb-1.5 w-full max-w-4xl min-w-0 overflow-hidden rounded-md"
    >
      <div className="ct-reader-row ct-command-inline__head flex w-full min-w-0 items-center gap-2 text-xs">
      <button
        type="button"
        data-testid="chat-command-group-toggle"
        aria-expanded={expanded || fullscreen}
        aria-controls={regionId}
        aria-label={`${expanded ? "Hide" : "Show"} commands in ${title}`}
        onClick={() => onToggle(!expanded)}
        className="flex min-w-0 flex-1 items-center gap-2 text-left"
      >
        <SquareTerminal aria-hidden size={16} className="shrink-0" style={{ color: "var(--ct-muted-fg)" }} />
        <span data-testid="chat-command-group-title" title={title} className="min-w-0 flex-1 truncate font-mono text-[12px]" style={{ color: "var(--ct-fg)" }}>
          {title}
        </span>
        <ToolOutcomeFacts event={event} />
        <span className="shrink-0 font-mono text-[10.5px]" style={{ color: "var(--ct-muted-fg)" }}>{countLabel}</span>
      </button>
      {opener}
      <span className="ct-tool-status-slot">
      <span data-testid="chat-command-group-status" role="status" aria-label={statusName(event.status)} className="ct-tool-status-icon flex items-center justify-center">
        <StatusGlyph status={event.status} />
      </span>
      <ChatCardMenu label={`Actions for ${title}`} items={[...readerItems, {
        label: "Details", kind: "details", icon: <SquareTerminal size={14} aria-hidden />, checked: technicalOpen,
        onSelect: () => setTechnicalOpen(value => !value),
      }]} joinRow />
      </span>
      <CardChevron expanded={expanded} label={`${expanded ? "Collapse" : "Expand"} command group`} onToggle={() => onToggle(!expanded)} />
      </div>
      {shellFileOperationPreview(event)}
      <div className="pb-1 pr-2">
        <CommandGroupDetail event={event} rows={rows} expanded={expanded || fullscreen} onExpand={() => onToggle(true)} {...navigation} />
      </div>
      <div id={regionId} role="region" aria-label="Command output" hidden={!technicalOpen} className="px-3 pb-2">
        {technicalOpen && event.result && <><div className="mb-1 text-[10.5px]" style={{ color: "var(--ct-muted-fg)" }}>Output</div><ToolOutputContent text={event.result} maxHeight="12rem" /></>}
      </div>
    </section>
    )}</ExpandableReader>
  );
}

export function CommandGroupDetail({ event, rows, expanded = true, onExpand, ...navigation }: CodetermNavigationProps & {
  event: ToolEvent;
  rows: CommandRow[];
  expanded?: boolean;
  onExpand?: () => void;
}) {
  const callKey = commandCallKey(event);
  const [openRow, setOpenRowState] = useState<number | null>(() => openSegmentOf(callKey));
  const toggleRow = (index: number) => {
    const next = openRow === index ? null : index;
    rememberOpenSegment(callKey, next);
    setOpenRowState(next);
  };
  const capped = !expanded;
  return <div role="group" aria-label={`${rows.length} commands`}>
    <CommandTree
      rows={rows}
      quiet
      onExpand={onExpand}
      cap={capped ? COMMAND_TREE_CAP : undefined}
      openIndex={openRow}
      onLine={toggleRow}
      renderDetail={(index) => <SegmentDetail event={event} row={rows[index]} index={index} navigation={navigation} />}
    />
  </div>;
}
