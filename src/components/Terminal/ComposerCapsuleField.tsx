import { useLayoutEffect, useRef, useState, type MouseEvent, type ReactNode, type RefObject } from "react";
import { LoaderCircle, RotateCw, TriangleAlert, X } from "lucide-react";
import { CapsuleDecor, CapsulePreview, LocalArtifactScope, capsuleLabelParts } from "@codeterm/chat-ui";
import {
  CAPSULE_LEAD,
  CAPSULE_TRAIL,
  capsuleSegments,
  labelChars,
  type CapsuleRegistry,
  type CapsuleSpan,
  type ComposerCapsule,
} from "./composerCapsules";

import { canRetryUpload, uploadCardPresence } from "./attachmentUploadModel";
import type { ComposerUpload } from "./composerUploadModel";

interface Hover {
  span: CapsuleSpan;
  capsule: ComposerCapsule;
  left: number;
  bottom: number;
}

function PaintedCapsule({ span, capsule, hovered, upload }: { span: CapsuleSpan; capsule: ComposerCapsule | null; hovered: boolean; upload?: ComposerUpload }) {
  const { name, detail } = capsuleLabelParts(span.label);
  const kindLabel = capsule && ["video", "image", "file"].includes(capsule.kind) ? capsule.kind : null;
  return (
    <span data-capsule-start={span.start} data-unknown={capsule ? undefined : ""} data-hovered={hovered || undefined} className="ct-capsule">
      {CAPSULE_LEAD}
      {upload ? <><span className="ct-capsule__plate" />{upload.state === "uploading"
        ? <span className="ct-capsule__icon"><LoaderCircle aria-hidden className="h-full w-full animate-spin motion-reduce:animate-none" /></span>
        : <TriangleAlert aria-hidden className="ct-capsule__icon" style={{ color: "var(--ct-error, #f85149)" }} />}</>
        : <CapsuleDecor kind={capsule?.kind ?? null} />}
      {kindLabel ? <span className="relative"><span style={{ visibility: "hidden" }}>{labelChars(name)}</span><span className="absolute left-0 text-xs">{kindLabel}</span></span> : labelChars(name)}
      {detail && <span className="ct-capsule__detail">{labelChars(detail)}</span>}
      {CAPSULE_TRAIL}
    </span>
  );
}

function UploadCard({ upload, span, onRemove, onRetry }: { upload: ComposerUpload; span: CapsuleSpan; onRemove: (span: CapsuleSpan) => void; onRetry: (id: number) => void }) {
  return (
    <div role="status" aria-label={`${upload.state === "uploading" ? "Uploading" : "Failed attachment"} ${upload.name}`} className="ct-capsule-button inline-flex items-center gap-1" title={upload.failure?.message}>
      {upload.state === "uploading" ? <LoaderCircle size={12} aria-hidden className="animate-spin motion-reduce:animate-none" /> : <TriangleAlert size={12} aria-hidden />}
      {canRetryUpload(upload) && <button type="button" aria-label={`Retry attachment ${upload.name}`} onClick={() => onRetry(upload.id)}><RotateCw size={12} aria-hidden /></button>}
      <button type="button" aria-label={`Remove attachment ${upload.name}`} onClick={() => onRemove(span)}><X size={12} aria-hidden /></button>
    </div>
  );
}

/**
 * The composer textarea with its capsules painted on an inert mirror; the textarea keeps
 * editing, IME and undo while its own glyphs go transparent under the mirror.
 */
export function ComposerCapsuleField({ tabId, value, registry, active, textareaRef, marginPx, onRemove, onRetry = () => {}, uploads = [], children }: {
  tabId: string;
  value: string;
  registry: CapsuleRegistry;
  active: boolean;
  textareaRef: RefObject<HTMLTextAreaElement | null>;
  marginPx: number;
  onRemove: (span: CapsuleSpan) => void;
  children: ReactNode;
  uploads?: ComposerUpload[];
  onRetry?: (id: number) => void;
}) {
  const fieldRef = useRef<HTMLDivElement>(null);
  const mirrorRef = useRef<HTMLDivElement>(null);
  const cardRef = useRef<HTMLDivElement>(null);
  const [hover, setHover] = useState<Hover | null>(null);

  useLayoutEffect(() => {
    const textarea = textareaRef.current;
    const mirror = mirrorRef.current;
    if (!active || !textarea || !mirror) return;
    const sync = () => {
      mirror.style.width = `${textarea.clientWidth}px`;
      mirror.scrollTop = textarea.scrollTop;
    };
    sync();
    textarea.addEventListener("scroll", sync);
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(sync);
    observer?.observe(textarea);
    return () => {
      textarea.removeEventListener("scroll", sync);
      observer?.disconnect();
    };
  }, [active, textareaRef, value]);

  const track = (event: MouseEvent<HTMLDivElement>) => {
    if (!active || cardRef.current?.contains(event.target as Node)) return;
    const field = fieldRef.current?.getBoundingClientRect();
    const hit = field && Array.from(mirrorRef.current?.querySelectorAll<HTMLElement>("[data-capsule-start]") ?? []).flatMap((element) =>
      Array.from(element.getClientRects()).map((rect) => ({ element, rect })),
    ).find(({ rect }) => event.clientX >= rect.left && event.clientX <= rect.right && event.clientY >= rect.top && event.clientY <= rect.bottom);
    const start = Number(hit?.element.dataset.capsuleStart);
    const span = hit ? capsuleSegments(value, registry).find((segment) => "span" in segment && segment.span.start === start) : undefined;
    if (!field || !hit || !span || !("span" in span) || !span.capsule) {
      if (hover) setHover(null);
      return;
    }
    if (hover?.span.start === span.span.start && hover.span.label === span.span.label) return;
    setHover({ span: span.span, capsule: span.capsule, left: Math.max(0, hit.rect.left - field.left), bottom: field.bottom - hit.rect.top });
  };

  const hoverCapsule = hover ? registry[hover.span.label] : undefined;
  const hoveredUpload = uploads.find((upload) => upload.id === hoverCapsule?.uploadId);
  const segments = active ? capsuleSegments(value, registry) : [];
  const uploadEntries = uploads.flatMap((upload) => {
    const found = capsuleSegments(value, registry).find((segment) => "span" in segment && segment.span.label === upload.label);
    return found && "span" in found ? [{ upload, span: found.span, presence: uploadCardPresence(hoveredUpload?.id === upload.id) }] : [];
  });
  return (
    <div ref={fieldRef} className="relative min-w-0 flex-1 self-center" onMouseMove={track} onMouseLeave={() => setHover(null)}>
      {children}
      {uploadEntries.map((entry) => {
        const visible = entry.presence === "hover";
        return <div key={entry.upload.id} ref={visible ? cardRef : undefined}
          className={visible ? "absolute z-30 pb-1.5" : "sr-only focus-within:not-sr-only focus-within:absolute focus-within:bottom-full focus-within:z-30"}
          style={visible && hover ? { left: hover.left, bottom: hover.bottom } : undefined}>
          <UploadCard {...entry} onRetry={onRetry} onRemove={(span) => { onRemove(span); setHover(null); }} />
        </div>;
      })}
      {active && (
        <div
          ref={mirrorRef}
          aria-hidden
          data-testid="composer-capsule-mirror"
          className="pointer-events-none absolute left-0 overflow-hidden pr-1 text-[14px]"
          style={{ top: marginPx, bottom: marginPx, lineHeight: "20px", whiteSpace: "pre-wrap", overflowWrap: "break-word", color: "var(--ct-fg, #c9d1d9)" }}
        >
          {segments.map((segment) => ("text" in segment
            ? segment.text
            : <PaintedCapsule key={segment.span.start} span={segment.span} capsule={segment.capsule} hovered={hover?.span.start === segment.span.start} upload={uploads.find(upload => upload.id === segment.capsule?.uploadId)} />))}
          {value.endsWith("\n") ? " " : null}
        </div>
      )}
      {active && hover && hoverCapsule && hoverCapsule.uploadId === undefined && (
        <div
          ref={cardRef}
          data-testid="composer-capsule-card"
          className="absolute z-30 pb-1.5"
          style={{ left: hover.left, bottom: hover.bottom }}
        >
          <LocalArtifactScope tabId={tabId}>
            <CapsulePreview
              kind={hoverCapsule.kind}
              path={hoverCapsule.path}
              label={hover.span.label}
              actions={(
                <button
                  type="button"
                  aria-label={`Remove ${hover.span.label}`}
                  title="Remove"
                  onMouseDown={(event) => event.preventDefault()}
                  onClick={() => { onRemove(hover.span); setHover(null); }}
                  className="ct-liquid-interactive ml-auto inline-flex h-5 w-5 shrink-0 items-center justify-center rounded-full opacity-70 hover:opacity-100 focus-visible:opacity-100 focus-visible:outline focus-visible:outline-2 focus-visible:outline-[var(--ct-accent)]"
                  style={{ color: "var(--ct-fg)" }}
                >
                  <X size={12} aria-hidden />
                </button>
              )}
            />
          </LocalArtifactScope>
        </div>
      )}
    </div>
  );
}
