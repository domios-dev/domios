import type { ToolEvent } from "@codeterm/chat-core";
import { toolCommandShape } from "../codeterm/commandShape";
import { isSend } from "../codeterm/templates";
import { ToolOutputContent } from "../ToolOutputContent";
import { CodetermActionDetail, type CodetermNavigationProps } from "./CodetermActionCard";
import { CommandGroupDetail } from "./CommandGroupCard";
import { FileDiffCards } from "./FileDiffCards";
import { ReadDetail } from "./ReadCard";
import { SearchDetail } from "./SearchCard";

export function StreamedCommandDetail({ event, ...navigation }: CodetermNavigationProps & { event: ToolEvent }) {
  if (event.fileDiffs?.length) return <FileDiffCards event={event} initiallyExpanded rawCommand={false} />;
  const shape = toolCommandShape(event);
  if (shape.kind === "group") return <CommandGroupDetail event={event} rows={shape.rows} {...navigation} />;
  if (shape.kind === "read") return <ReadDetail event={event} />;
  if (shape.kind === "search") return <SearchDetail event={event} />;
  const result = event.result && <div data-testid="toolcard-result"><ToolOutputContent text={event.result} maxHeight="12rem" /></div>;
  if (shape.kind === "codeterm") return isSend(shape.invocation) ? null
    : <CodetermActionDetail event={event} invocation={shape.invocation} fallback={result} {...navigation} />;
  return event.status === "pending" ? (
    <>
      <div data-testid="toolcard-spinner" className="flex items-center gap-1.5 text-xs" style={{ color: "var(--ct-muted-fg)" }}>
        <span aria-hidden="true" className="inline-block h-1.5 w-1.5 rounded-full" style={{ background: "var(--ct-accent)" }} />
        <span>Running…</span>
      </div>
      {result && <div className="mt-1.5">{result}</div>}
    </>
  ) : result;
}
