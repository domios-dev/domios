import type { ToolEvent } from "@codeterm/chat-core";
import { ToolOutputContent } from "../ToolOutputContent";
import { ToolCardFrame } from "./ToolCardFrame";

function counts(event: ToolEvent): { matches: string; files: string } {
  const text = [event.summary, event.detail, event.result].filter(Boolean).join("\n");
  const matches = text.match(/(\d+)\s+matches?/i)?.[1] ?? text.match(/(\d+)\s+results?/i)?.[1];
  const files = text.match(/(\d+)\s+files?/i)?.[1];
  if (matches && files) return { matches, files };
  const resultLines = (event.result ?? "")
    .split("\n")
    .map((line) => line.trim())
    .filter((line) => line && !/^(?:Script completed|Wall time .+ seconds|Output:)$/i.test(line));
  const uniqueMatches = new Set(resultLines);
  const matchedFiles = new Set(
    resultLines.map((line) => line.match(/^(.+?):\d+(?::\d+)?:/)?.[1]).filter((file): file is string => Boolean(file)),
  );
  return {
    matches: matches ?? String(uniqueMatches.size),
    files: files ?? String(matchedFiles.size),
  };
}

function searchSummary(event: ToolEvent): string {
  const command = event.detail || event.summary;
  const query = command.match(/\b(?:rg|grep)\b(?:\s+-\S+)*\s+(?:"([^"]+)"|'([^']+)'|([^\s|]+))/)?.slice(1).find(Boolean);
  const scope = command.match(/\s((?:src|packages|cli|docs|tests)(?:\/[^\s|]*)?)(?=\s|$)/)?.[1];
  return `${query ? `“${query}”` : "matches"}${scope ? ` in ${scope}` : ""}`;
}

export function SearchCard({ event, expanded, onToggle }: { event: ToolEvent; expanded: boolean; onToggle: () => void }) {
  const result = counts(event);
  return <ToolCardFrame event={event} expanded={expanded} onToggle={onToggle} title="Search" summary={searchSummary(event)} chips={<span data-testid="toolcard-search-counts" className="shrink-0 rounded px-1.5 py-0.5 font-mono text-[10px]" style={{ color: "var(--ct-muted-fg)", background: "var(--ct-border)" }}>{result.matches} matches · {result.files} files</span>}>
    <div className="break-all font-mono text-xs" style={{ color: "var(--ct-fg)" }}>{event.detail || event.summary}</div>
    {event.status === "pending" ? <div data-testid="toolcard-spinner" style={{ color: "var(--ct-muted-fg)" }}>Searching…</div> : event.result && <div data-testid="toolcard-result"><ToolOutputContent text={event.result} maxHeight="12rem" /></div>}
  </ToolCardFrame>;
}
