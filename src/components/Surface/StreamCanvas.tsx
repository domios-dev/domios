import { useEffect, useId, useRef, type KeyboardEvent, type MouseEvent, type PointerEvent, type ReactNode } from "react";
import { decodeJpegFrame, type FrameDecoder, type FrameFeed } from "./frameFeed";
import { SnipOverlay } from "./SnipOverlay";
import { clickInput, decideKey, modifierMask, wheelInput, type PagePoint, type PageRect, type Rect, type StreamInput, type StreamViewport } from "./streamModel";
import { canvasHint, liveViewLabel, type PopoverAnchor } from "./surfaceChromeModel";
import { useStreamCanvas } from "./useStreamCanvas";

export interface StreamCanvasProps {
  visible?: boolean;
  label: string;
  pageTitle?: string;
  interactive: boolean;
  picking: boolean;
  snipping?: boolean;
  frames: FrameFeed;
  decode?: FrameDecoder;
  children?: ReactNode;
  onViewport(viewport: StreamViewport): void;
  onHidden?(): void;
  onInput(input: StreamInput): void;
  onCopy?(): void;
  onPaste?(): void;
  onPick(point: PagePoint, anchor: PopoverAnchor): void;
  onHover(point: PagePoint): void;
  onBlocked(): void;
  onExitInteraction(): void;
  onCancelPick(): void;
  onSnip?(rect: PageRect, anchor: PopoverAnchor, frame: Rect): void;
  onCancelSnip?(): void;
  onError?(error: unknown): void;
}

const noop = () => undefined;

export function StreamCanvas({
  visible = true,
  label,
  pageTitle,
  interactive,
  picking,
  snipping = false,
  frames,
  decode = decodeJpegFrame,
  children,
  onViewport,
  onHidden,
  onInput,
  onCopy,
  onPaste,
  onPick,
  onHover,
  onBlocked,
  onExitInteraction,
  onCancelPick,
  onSnip = noop,
  onCancelSnip = noop,
  onError = noop,
}: StreamCanvasProps) {
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const stageRef = useRef<HTMLDivElement | null>(null);
  const hintId = useId();
  const pointer = useRef<{ id: number; point: PagePoint; count: number; modifiers: number } | null>(null);
  const suppressClick = useRef(false);
  const mousePointerId = useRef<number | null>(null);
  const stream = useStreamCanvas({ visible, canvasRef, stageRef, frames, decode, onViewport, onHidden, onError });
  const latest = useRef({ interactive, stream, onInput });
  latest.current = { interactive, stream, onInput };

  useEffect(() => {
    if (interactive) canvasRef.current?.focus();
  }, [interactive]);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    // React registers wheel listeners as passive, which would make preventDefault a no-op.
    const onWheel = (event: WheelEvent) => {
      const current = latest.current;
      if (!current.interactive) return;
      event.preventDefault();
      const point = current.stream.mapPoint(event.clientX, event.clientY);
      if (point) current.onInput(wheelInput(point, event, current.stream.stageSize().height));
    };
    canvas.addEventListener("wheel", onWheel, { passive: false });
    return () => canvas.removeEventListener("wheel", onWheel);
  }, []);

  const onClick = (event: MouseEvent<HTMLCanvasElement>) => {
    if (suppressClick.current) {
      suppressClick.current = false;
      return;
    }
    const point = stream.mapPoint(event.clientX, event.clientY);
    if (picking) {
      if (point) onPick(point, { ...stream.localPoint(event.clientX, event.clientY), ...stream.stageSize() });
      return;
    }
    if (!interactive) {
      onBlocked();
      return;
    }
    if (point) onInput(clickInput(point, event.detail));
  };

  const releasePointer = () => {
    const held = pointer.current;
    if (!held) return;
    pointer.current = null;
    onInput({ kind: "pointer", phase: "up", ...held.point, button: "left", buttons: 0, clickCount: held.count, modifiers: held.modifiers });
  };

  useEffect(() => {
    if (!interactive || picking || snipping || !visible) releasePointer();
  }, [interactive, picking, snipping, visible]);

  const onPointerDown = (event: PointerEvent<HTMLCanvasElement>) => {
    suppressClick.current = false;
    if (!interactive || picking || snipping || event.button !== 0) return;
    const point = stream.mapPoint(event.clientX, event.clientY);
    if (!point) return;
    if (event.pointerType === "mouse") {
      mousePointerId.current = event.pointerId;
      return;
    }
    event.preventDefault();
    event.currentTarget.focus();
    event.currentTarget.setPointerCapture?.(event.pointerId);
    const count = Math.min(Math.max(event.detail, 1), 3);
    const modifiers = modifierMask(event);
    pointer.current = { id: event.pointerId, point, count, modifiers };
    onInput({ kind: "pointer", phase: "down", ...point, button: "left", buttons: 1, clickCount: count, modifiers });
  };

  const onMouseDown = (event: MouseEvent<HTMLCanvasElement>) => {
    const id = mousePointerId.current;
    mousePointerId.current = null;
    if (id === null || !interactive || picking || snipping || event.button !== 0) return;
    const point = stream.mapPoint(event.clientX, event.clientY);
    if (!point) return;
    event.preventDefault();
    event.currentTarget.focus();
    event.currentTarget.setPointerCapture?.(id);
    const count = Math.min(Math.max(event.detail, 1), 3);
    const modifiers = modifierMask(event);
    pointer.current = { id, point, count, modifiers };
    onInput({ kind: "pointer", phase: "down", ...point, button: "left", buttons: 1, clickCount: count, modifiers });
  };

  const onPointerMove = (event: PointerEvent<HTMLCanvasElement>) => {
    const held = pointer.current;
    if (!interactive || picking || snipping || (held && held.id !== event.pointerId)) return;
    const point = stream.mapPoint(event.clientX, event.clientY);
    if (!point) return;
    if (held) held.point = point;
    onInput({ kind: "pointer", phase: "move", ...point, button: "left", buttons: held ? 1 : 0, clickCount: held?.count ?? 1, modifiers: modifierMask(event) });
  };

  const onPointerUp = (event: PointerEvent<HTMLCanvasElement>) => {
    if (pointer.current?.id !== event.pointerId) return;
    pointer.current.point = stream.mapPoint(event.clientX, event.clientY) ?? pointer.current.point;
    suppressClick.current = true;
    releasePointer();
    if (event.currentTarget.hasPointerCapture?.(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId);
  };

  const onMouseMove = (event: MouseEvent<HTMLCanvasElement>) => {
    if (!picking) return;
    const point = stream.mapPoint(event.clientX, event.clientY);
    if (point) onHover(point);
  };

  const onKeyDown = (event: KeyboardEvent<HTMLCanvasElement>) => {
    const decision = decideKey(event.nativeEvent, { interactive, picking, origin: "surface" });
    if (decision.action === "keep") return;
    if (decision.action === "copy" && !onCopy) return;
    if (decision.action === "paste" && !onPaste) return;
    event.preventDefault();
    event.stopPropagation();
    if (decision.action === "forward") onInput(decision.input);
    else if (decision.action === "copy") onCopy?.();
    else if (decision.action === "paste") onPaste?.();
    else if (decision.action === "exit-interaction") onExitInteraction();
    else onCancelPick();
  };

  const mode = snipping ? "snipping" : picking ? "picking" : interactive ? "interactive" : "watch";
  return (
    <div ref={stageRef} className="ct-surface-stage">
      <canvas
        ref={canvasRef}
        role={interactive ? "application" : "img"}
        aria-roledescription="live view"
        aria-label={liveViewLabel(label, pageTitle)}
        aria-describedby={hintId}
        tabIndex={interactive || picking ? 0 : -1}
        data-mode={mode}
        className="ct-surface-canvas"
        onClick={onClick}
        onMouseMove={onMouseMove}
        onMouseDown={onMouseDown}
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
        onPointerCancel={releasePointer}
        onLostPointerCapture={releasePointer}
        onBlur={releasePointer}
        onKeyDown={onKeyDown}
      />
      <p id={hintId} className="sr-only">{canvasHint(interactive, picking, snipping)}</p>
      {snipping && <SnipOverlay drawRect={stream.drawRect} pageRect={stream.pageRect} onFinish={onSnip} onCancel={onCancelSnip} />}
      {children}
    </div>
  );
}
