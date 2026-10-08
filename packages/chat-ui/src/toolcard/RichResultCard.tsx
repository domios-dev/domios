import { localThumbnailSource, type ThumbnailSource, type ToolEvent, type ToolRichResult } from "@codeterm/chat-core";
import { Check, ChevronRight, CircleX, LoaderCircle, Minus } from "lucide-react";
import { useId, type ReactNode } from "react";
import { useLocalImage } from "../localImage";
import { ToolOutputContent } from "../ToolOutputContent";
import { ImageResultBody } from "./ImageResultBody";
import { toolKindVisual } from "./toolKindVisual";

function fileName(path: string): string {
  const segments = path.split(/[\\/]/).filter(Boolean);
  return segments[segments.length - 1] ?? path;
}

function thumbnailNote(source: ThumbnailSource, failed: boolean): string {
  if (source.kind === "local") return failed ? "not shown: local file could not be read" : "not shown yet: reading local file";
  if (source.reason === "remote") return "not shown: path is not local, nothing was fetched";
  if (source.reason === "relative") return "not shown: path is not absolute";
  if (source.reason === "unsupported") return "not shown: not an image file";
  return "not shown: no local path reported";
}

function rowSummary(rich: ToolRichResult | undefined, event: ToolEvent): string {
  if (rich?.kind === "web") return rich.query || rich.url || rich.action || event.summary;
  if (rich?.kind === "image") return (rich.path && fileName(rich.path)) || rich.prompt || rich.status || event.summary;
  return event.summary;
}

function StatusGlyph({ status }: { status: ToolEvent["status"] }) {
  if (status === "pending") return <LoaderCircle aria-hidden="true" size={12} className="motion-safe:animate-spin" style={{ color: "var(--ct-yellow)" }} />;
  if (status === "error") return <CircleX aria-hidden="true" size={12} style={{ color: "var(--ct-red)" }} />;
  if (status === "unresolved") return <Minus aria-hidden="true" size={12} style={{ color: "var(--ct-muted-fg)" }} />;
  return <Check aria-hidden="true" size={12} style={{ color: "var(--ct-green)" }} />;
}

function Field({ name, children }: { name: string; children: ReactNode }) {
  return (
    <>
      <dt style={{ color: "var(--ct-muted-fg)" }}>{name}</dt>
      <dd data-field={name} className="m-0 min-w-0 whitespace-pre-wrap break-all" style={{ color: "var(--ct-fg)" }}>{children}</dd>
    </>
  );
}

export function RichResultCard({ event, expanded, onToggle }: { event: ToolEvent; expanded: boolean; onToggle: () => void }) {
  const regionId = useId();
  const visual = toolKindVisual[event.kind];
  const Glyph = visual.glyph;
  const rich = event.richResult?.kind === event.kind ? event.richResult : undefined;
  const source = localThumbnailSource(rich?.kind === "image" ? rich.path : undefined);
  const thumbnail = useLocalImage(source.kind === "local" ? source.path : undefined);
  const summary = rowSummary(rich, event) || "Result";
  const alt = `${rich?.origin === "viewed" ? "Viewed image" : "Generated image"}: ${rich?.path ? fileName(rich.path) : "local file"}`;
  const imageShown = Boolean(thumbnail.src);
  const statusName = event.status === "unresolved" ? "no result" : event.status;

  return (
    <section
      data-testid="chat-rich-result"
      data-rich-kind={event.kind}
      data-tool-status={event.status}
      aria-label={`${visual.label} result`}
      className="mx-auto mb-1.5 w-full max-w-4xl min-w-0 overflow-hidden rounded-md"
      style={{
        background: "color-mix(in srgb, var(--ct-surface) 60%, transparent)",
      }}
    >
      <div className="flex min-w-0 items-center gap-2 pl-3">
        {thumbnail.src && (
          <img
            data-testid="chat-rich-result-thumbnail"
            src={thumbnail.src}
            alt={alt}
            onError={thumbnail.markFailed}
            className="h-9 w-9 shrink-0 rounded-md border object-cover"
            style={{ borderColor: "var(--ct-border)" }}
            draggable={false}
          />
        )}
        <button
          type="button"
          aria-expanded={expanded}
          aria-controls={regionId}
          aria-label={`${expanded ? "Hide" : "Show"} ${visual.label} result ${summary}`}
          onClick={onToggle}
          className="flex min-w-0 flex-1 items-center gap-2 py-1.5 pr-3 text-left text-xs hover:bg-[var(--ct-surface)] focus-visible:outline focus-visible:outline-1 focus-visible:outline-[var(--ct-accent)] motion-safe:transition-colors motion-reduce:transition-none"
        >
          <Glyph aria-hidden="true" size={13} className="shrink-0" style={{ color: visual.color }} />
          <span className="shrink-0 font-mono text-[11px] font-semibold" style={{ color: visual.color }}>{visual.label}</span>
          <span data-testid="chat-rich-result-summary" className="min-w-0 flex-1 truncate font-mono text-[12px]" style={{ color: "var(--ct-fg)" }}>{summary}</span>
          {rich?.resultCount !== undefined && (
            <span data-testid="chat-rich-result-count" className="shrink-0 font-mono text-[10.5px]" style={{ color: "var(--ct-muted-fg)" }}>
              {rich.resultCount} {rich.resultCount === 1 ? "result" : "results"}
            </span>
          )}
          <span data-testid="chat-rich-result-status" role="img" aria-label={statusName} className="flex shrink-0"><StatusGlyph status={event.status} /></span>
          <ChevronRight aria-hidden="true" size={14} className={`shrink-0 ${expanded ? "rotate-90" : ""}`} style={{ color: "var(--ct-muted-fg)" }} />
        </button>
      </div>
      <div id={regionId} role="region" aria-label={`${visual.label} result details`} hidden={!expanded} className="pb-2 pl-8 pr-3">
        {expanded && event.result && <ToolOutputContent text={event.result} />}
        {expanded && thumbnail.src && rich?.path && source.kind === "local" && (
          <ImageResultBody src={thumbnail.src} path={rich.path} localPath={source.path} name={fileName(source.path)} alt={alt} />
        )}
        {expanded && (rich ? (
          <dl data-testid="chat-rich-result-detail" className="m-0 grid grid-cols-[max-content_minmax(0,1fr)] gap-x-3 gap-y-0.5 font-mono text-[11.5px]">
            {rich.origin && !imageShown && <Field name="source">{rich.origin === "viewed" ? "image view" : "image generation"}</Field>}
            {rich.query !== undefined && <Field name="query">{rich.query}</Field>}
            {rich.kind === "web" && <Field name="results">{rich.resultCount ?? "not reported"}</Field>}
            {rich.action && <Field name="action">{rich.action}</Field>}
            {rich.url && <Field name="url">{rich.url}</Field>}
            {rich.pattern && <Field name="pattern">{rich.pattern}</Field>}
            {rich.status && <Field name="status">{rich.status}</Field>}
            {rich.prompt && <Field name="prompt">{rich.prompt}</Field>}
            {rich.kind === "image" && !imageShown && <Field name="path">{rich.path ?? "not reported"}</Field>}
            {rich.kind === "image" && !imageShown && <Field name="thumbnail">{thumbnailNote(source, thumbnail.failed)}</Field>}
          </dl>
        ) : (
          <p data-testid="chat-rich-result-unavailable" className="m-0 break-all font-mono text-[11.5px]" style={{ color: "var(--ct-muted-fg)" }}>
            Result details unavailable{event.detail ? `: ${event.detail}` : ""}
          </p>
        ))}
      </div>
    </section>
  );
}
