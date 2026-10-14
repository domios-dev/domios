import { useLayoutEffect, useRef, useState, type MouseEvent, type ReactNode, type RefObject } from "react";
import { X } from "lucide-react";
import { AttachmentCapsule, CapsuleDecor, LocalArtifactScope, capsuleLabelParts } from "@codeterm/chat-ui";
import {
  CAPSULE_LEAD,
  CAPSULE_TRAIL,
  capsuleSegments,
  labelChars,
  type CapsuleRegistry,
  type CapsuleSpan,
  type ComposerCapsule,
} from "./composerCapsules";

import { canRetryUpload } from "./attachmentUploadModel";
import type { ComposerUpload } from "./composerUploadModel";

interface Hover {
  span: CapsuleSpan;
  capsule: ComposerCapsule;
  left: number;
  bottom: number;
}

function PaintedCapsule({ span, capsule, hovered, onRemove }: { span: CapsuleSpan; capsule: ComposerCapsule | null; hovered: boolean; onRemove: () => void }) {
  const { name, detail } = capsuleLabelParts(span.label);
  const content = <>{CAPSULE_LEAD}{labelChars(name)}{detail && <span className="ct-capsule__detail">{labelChars(detail)}</span>}{CAPSULE_TRAIL}</>;
  return <span data-capsule-start={span.start} data-unknown={capsule ? undefined : ""} data-hovered={hovered || undefined}>
    {capsule && capsule.uploadId === undefined
      ? <AttachmentCapsule attachment={{ kind: capsule.kind, path: capsule.path, label: span.label }} labelContent={content} actions={<button type="button" aria-label={`Remove ${span.label}`} onClick={onRemove}><X size={12} aria-hidden /></button>} />
      : <span className="ct-capsule"><CapsuleDecor kind={capsule?.kind ?? null} />{content}</span>}
  </span>;
}

/**
 * The composer textarea with its capsules painted on an inert mirror; the textarea keeps
 * editing, IME and undo while its own glyphs go transparent under the mirror.
 */
export function ComposerCapsuleField({ tabId, value, registry, active, textareaRef, marginPx, onRemove, uploads = [], onRetryUpload, children }: {
  tabId: string;
  value: string;
  registry: CapsuleRegistry;
  active: boolean;
  textareaRef: RefObject<HTMLTextAreaElement | null>;
  marginPx: number;
  onRemove: (span: CapsuleSpan) => void;
  children: ReactNode;
  uploads?: ComposerUpload[];
  onRetryUpload?: (id: number) => void;
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
    if (!field || !hit || !span || !("span" in span) || !span.capsule || span.capsule.uploadId === undefined) {
      if (hover) setHover(null);
      return;
    }
    if (hover?.span.start === span.span.start && hover.span.label === span.span.label) return;
    setHover({ span: span.span, capsule: span.capsule, left: Math.max(0, hit.rect.left - field.left), bottom: field.bottom - hit.rect.top });
  };

  const hoverCapsule = hover ? registry[hover.span.label] : undefined;
  const hoveredUpload = uploads.find((upload) => upload.id === hoverCapsule?.uploadId);
  const segments = active ? capsuleSegments(value, registry) : [];
  return (
    <div ref={fieldRef} className="relative min-w-0 flex-1 self-center" onMouseMove={track} onMouseLeave={() => setHover(null)}>
      {children}
      {uploads.map((upload) => {
        const span = capsuleSegments(value, registry).find((segment) => "span" in segment && segment.span.label === upload.label);
        if (!span || !("span" in span)) return null;
        const visible = hoveredUpload?.id === upload.id;
        return <div key={upload.id} ref={visible ? cardRef : undefined}
          className={visible ? "absolute z-30 pb-1.5" : "sr-only focus-within:not-sr-only focus-within:absolute focus-within:bottom-full focus-within:z-30"}
          style={visible && hover ? { left: hover.left, bottom: hover.bottom } : undefined}>
          <div role="status" className="ct-capsule-button">
            <span>{upload.state === "uploading" ? "Uploading…" : upload.failure?.message}</span>
            {canRetryUpload(upload) && <button type="button" aria-label={`Retry attachment ${upload.name}`} onClick={() => onRetryUpload?.(upload.id)}>Retry</button>}
            <button type="button" aria-label={`Remove attachment ${upload.name}`} onClick={() => { onRemove(span.span); setHover(null); }}>Remove</button>
          </div>
        </div>;
      })}
      {active && (
        <div
          ref={mirrorRef}
          data-testid="composer-capsule-mirror"
          className="pointer-events-none absolute left-0 overflow-hidden pr-1 text-[14px]"
          style={{ top: marginPx, bottom: marginPx, lineHeight: "20px", whiteSpace: "pre-wrap", overflowWrap: "break-word", color: "var(--ct-fg, #c9d1d9)" }}
        >
          {segments.map((segment, index) => ("text" in segment
            ? <span key={index} aria-hidden>{segment.text}</span>
            : <LocalArtifactScope key={segment.span.start} tabId={tabId}><PaintedCapsule span={segment.span} capsule={segment.capsule} hovered={hover?.span.start === segment.span.start} onRemove={() => onRemove(segment.span)} /></LocalArtifactScope>))}
          {value.endsWith("\n") ? " " : null}
        </div>
      )}

    </div>
  );
}
