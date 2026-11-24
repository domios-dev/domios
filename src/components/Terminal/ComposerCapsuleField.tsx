import { Fragment, useLayoutEffect, useRef, useState, type MouseEvent, type ReactNode, type RefObject } from "react";
import { BusyState } from "../shared/BusyState";
import { RotateCw, TriangleAlert, X } from "lucide-react";
import { AttachmentCapsule, CapsulePreview, LocalArtifactScope, capsuleLabelParts } from "@codeterm/chat-ui";
import {
  CAPSULE_GAP,
  CAPSULE_LEAD,
  CAPSULE_TRAIL,
  capsuleSegments,
  labelChars,
  typedLabel,
  type CapsuleRegistry,
  type CapsuleSpan,
  type ComposerCapsule,
} from "./composerCapsules";

import { AnchoredPopover } from "../shared/AnchoredPopover";
import { canRetryUpload, uploadCardPresence } from "./attachmentUploadModel";
import type { ComposerUpload } from "./composerUploadModel";

interface Hover {
  span: CapsuleSpan;
  capsule: ComposerCapsule;
  x: number;
  y: number;
}

const MIRRORED_TEXT_STYLES = [
  "font-family", "font-size", "font-weight", "font-style", "font-stretch", "font-variant", "font-feature-settings", "font-variation-settings",
  "font-kerning", "letter-spacing", "word-spacing", "line-height", "text-indent", "text-transform", "tab-size", "text-rendering",
  "white-space", "overflow-wrap", "word-break", "line-break", "hyphens", "direction", "text-align", "-webkit-text-size-adjust",
  "padding-top", "padding-bottom",
] as const;

/** The mirror's text box taken from the textarea itself, so every glyph and wrap lands where the textarea puts its caret. */
export function capsuleMirrorBox(style: Pick<CSSStyleDeclaration, "getPropertyValue">, clientWidth: number, innerInsetPx: number): Record<string, string> {
  const box: Record<string, string> = Object.fromEntries(MIRRORED_TEXT_STYLES.map((name) => [name, style.getPropertyValue(name)]));
  const padding = (side: "left" | "right") => `${(Number.parseFloat(style.getPropertyValue(`padding-${side}`)) || 0) + innerInsetPx}px`;
  return { ...box, "box-sizing": "border-box", width: `${clientWidth}px`, "padding-left": padding("left"), "padding-right": padding("right") };
}

/**
 * Per-side inset an engine adds inside a textarea's text box beyond its CSS padding (WebKit on iOS pads the
 * inner editor); computed style cannot see it, so an empty, zero-width, unpadded, unwrapped probe exposes it as overflow.
 */
export function textareaInnerInset(textarea: HTMLTextAreaElement): number {
  const style = textarea.ownerDocument?.defaultView?.getComputedStyle?.(textarea);
  if (!style) return 0;
  const probe = textarea.ownerDocument.createElement("textarea");
  probe.wrap = "off";
  probe.tabIndex = -1;
  probe.setAttribute("aria-hidden", "true");
  probe.style.cssText = "position:absolute;visibility:hidden;pointer-events:none;width:0;height:0;min-width:0;min-height:0;margin:0;padding:0;border:0;overflow:hidden;resize:none";
  probe.style.font = style.font;
  textarea.after(probe);
  try {
    return probe.scrollWidth / 2;
  } finally {
    probe.remove();
  }
}

export function useComposerMirror(
  editorRef: RefObject<HTMLTextAreaElement | null>,
  mirrorRef: RefObject<HTMLDivElement | null>,
  text: string,
  active = true,
) {
  useLayoutEffect(() => {
    const editor = editorRef.current;
    const mirror = mirrorRef.current;
    if (!active || !editor || !mirror) return;
    let font: string | undefined;
    let innerInset = 0;
    const layout = (content?: DOMRectReadOnly) => {
      const style = editor.ownerDocument?.defaultView?.getComputedStyle?.(editor);
      if (!style) {
        syncMirrorScroll(editor, mirror);
        return;
      }
      if (style.font !== font) {
        font = style.font;
        innerInset = textareaInnerInset(editor);
      }
      // ResizeObserver preserves the fractional text width after the scrollbar takes its gutter.
      const width = content
        ? content.width + (parseFloat(style.paddingLeft) || 0) + (parseFloat(style.paddingRight) || 0)
        : editor.clientWidth;
      for (const [name, setting] of Object.entries(capsuleMirrorBox(style, width, innerInset))) {
        mirror.style.setProperty(name, setting);
      }
      syncMirrorScroll(editor, mirror);
    };
    const scroll = () => syncMirrorScroll(editor, mirror);
    layout();
    editor.addEventListener("scroll", scroll);
    const observer = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(([entry]) => layout(entry.contentRect));
    observer?.observe(editor);
    return () => {
      editor.removeEventListener("scroll", scroll);
      observer?.disconnect();
    };
  }, [active, editorRef, mirrorRef]);

  useLayoutEffect(() => {
    if (active && editorRef.current && mirrorRef.current) syncMirrorScroll(editorRef.current, mirrorRef.current);
  }, [active, editorRef, mirrorRef, text]);
}

function syncMirrorScroll(editor: HTMLTextAreaElement, mirror: HTMLDivElement) {
  mirror.scrollTop = editor.scrollTop;
  mirror.scrollLeft = editor.scrollLeft;
}

function PaintedCapsule({ span, capsule, hovered, upload }: { span: CapsuleSpan; capsule: ComposerCapsule | null; hovered: boolean; upload?: ComposerUpload }) {
  const { name, detail } = capsuleLabelParts(typedLabel(span.label));
  return (
    <AttachmentCapsule
      inert
      attachment={{ kind: capsule?.kind ?? "file", path: capsule?.path ?? "", label: capsule?.displayName ?? span.label }}
      spanProps={{
        "data-capsule-start": span.start, "data-unknown": capsule ? undefined : "", "data-hovered": hovered || undefined,
        style: { fontSize: "inherit", lineHeight: "inherit" },
      }}
      decoration={upload ? <><span className="ct-capsule__plate" />{upload.state === "uploading"
        ? <span className="ct-capsule__icon"><BusyState inline hideLabel dotSize="small" label="Uploading" /></span>
        : <TriangleAlert aria-hidden className="ct-capsule__icon" style={{ color: "var(--ct-error, #f85149)" }} />}</> : undefined}
      labelContent={<>
        {CAPSULE_LEAD}
        {capsule?.displayName && /^[a-f\d]{16,}/i.test(name) ? <span className="relative">
          <span aria-hidden className="invisible">{labelChars(name)}</span>
          <span className="absolute inset-0 overflow-hidden text-ellipsis whitespace-nowrap">{capsule.displayName}</span>
        </span> : labelChars(name)}
        {detail && <span className="ct-capsule__detail">{labelChars(detail)}</span>}
        {CAPSULE_TRAIL}
      </>}
    />
  );
}

function UploadCard({ upload, span, onRemove, onRetry }: { upload: ComposerUpload; span: CapsuleSpan; onRemove: (span: CapsuleSpan) => void; onRetry: (id: number) => void }) {
  return (
    <div role="status" aria-label={`${upload.state === "uploading" ? "Uploading" : "Failed attachment"} ${upload.name}`} className="ct-capsule-button inline-flex items-center gap-1" title={upload.failure?.message}>
      {upload.state === "uploading" ? <BusyState inline hideLabel dotSize="small" label={`Uploading ${upload.name}`} /> : <TriangleAlert size={12} aria-hidden />}
      <span>{upload.state === "uploading" ? `Uploading ${upload.name}` : upload.failure?.message}</span>
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

  useComposerMirror(textareaRef, mirrorRef, value, active);

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
    setHover({ span: span.span, capsule: span.capsule, x: hit.rect.left, y: hit.rect.top });
  };

  const hoverCapsule = hover ? registry[hover.span.label] : undefined;
  const hoveredUpload = uploads.find((upload) => upload.id === hoverCapsule?.uploadId);
  const segments = active ? capsuleSegments(value, registry) : [];
  const uploadEntries = uploads.flatMap((upload) => {
    const found = capsuleSegments(value, registry).find((segment) => "span" in segment && segment.span.label === upload.label);
    return found && "span" in found ? [{ upload, span: found.span, presence: uploadCardPresence(hoveredUpload?.id === upload.id) }] : [];
  });
  return (
    <div ref={fieldRef} className="relative min-w-0 flex-1 self-center" onPointerMove={(event) => { if (event.pointerType === "mouse") track(event); }} onPointerUp={(event) => { if (event.pointerType === "touch") track(event); }} onPointerLeave={(event) => { if (event.pointerType === "mouse" && (!(event.relatedTarget instanceof Node) || !cardRef.current?.contains(event.relatedTarget))) setHover(null); }}>
      {children}
      {uploadEntries.map((entry) => {
        const visible = entry.presence === "hover";
        const card = <UploadCard {...entry} onRetry={onRetry} onRemove={(span) => { onRemove(span); setHover(null); }} />;
        return visible && hover ? <AnchoredPopover key={entry.upload.id} open anchor={{ x: hover.x, y: hover.y }} floatingRef={cardRef} placement="top-start" gap={0} onClose={() => setHover(null)} style={{ overflow: "auto" }}>
          {window.matchMedia?.("(hover: none)").matches && <button type="button" aria-label="Close attachment preview" onClick={() => setHover(null)} style={{ minWidth: 44, minHeight: 44 }}><X size={18} aria-hidden /></button>}
          {card}
        </AnchoredPopover> : <div key={entry.upload.id} className="sr-only focus-within:not-sr-only focus-within:absolute focus-within:bottom-full focus-within:z-30">{card}</div>;
      })}
      {active && (
        <div
          ref={mirrorRef}
          aria-hidden
          data-testid="composer-capsule-mirror"
          className="pointer-events-none absolute left-0 overflow-hidden"
          style={{ insetBlock: marginPx, color: "var(--ct-fg, #c9d1d9)" }}
        >
          {segments.map((segment) => ("text" in segment
            ? segment.text
            : <Fragment key={segment.span.start}>
              <PaintedCapsule span={segment.span} capsule={segment.capsule} hovered={hover?.span.start === segment.span.start} upload={uploads.find(upload => upload.id === segment.capsule?.uploadId)} />
              {segment.token.endsWith(CAPSULE_GAP) ? CAPSULE_GAP : null}
            </Fragment>))}
          {value.endsWith("\n") ? " " : null}
        </div>
      )}
      {active && hover && hoverCapsule && hoverCapsule.uploadId === undefined && (
        <AnchoredPopover open anchor={{ x: hover.x, y: hover.y }} floatingRef={cardRef} placement="top-start" gap={0} onClose={() => setHover(null)} onPointerLeave={(event) => { if (event.pointerType === "mouse") setHover(null); }} style={{ overflow: "auto" }}>
          <div data-testid="composer-capsule-card">
          <LocalArtifactScope tabId={tabId}>
            <CapsulePreview
              kind={hoverCapsule.kind}
              path={hoverCapsule.path}
              label={hover.span.label}
              onClose={() => setHover(null)}
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
        </AnchoredPopover>
      )}
    </div>
  );
}
