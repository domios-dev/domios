import { useEffect, useId, useRef, type KeyboardEvent, type MouseEvent, type ReactNode } from "react";
import { decodeJpegFrame, type FrameDecoder, type FrameFeed } from "./frameFeed";
import { SnipOverlay } from "./SnipOverlay";
import { clickInput, decideKey, wheelInput, type PagePoint, type PageRect, type Rect, type StreamInput, type StreamViewport } from "./streamModel";
import { canvasHint, liveViewLabel, type PopoverAnchor } from "./surfaceChromeModel";
import { useStreamCanvas } from "./useStreamCanvas";

export interface StreamCanvasProps {
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
  const stream = useStreamCanvas({ canvasRef, stageRef, frames, decode, onViewport, onHidden, onError });
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

  const onMouseMove = (event: MouseEvent<HTMLCanvasElement>) => {
    if (!picking) return;
    const point = stream.mapPoint(event.clientX, event.clientY);
    if (point) onHover(point);
  };

  const onKeyDown = (event: KeyboardEvent<HTMLCanvasElement>) => {
    const decision = decideKey(event.nativeEvent, { interactive, picking, origin: "surface" });
    if (decision.action === "keep") return;
    event.preventDefault();
    event.stopPropagation();
    if (decision.action === "forward") onInput(decision.input);
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
        onKeyDown={onKeyDown}
      />
      <p id={hintId} className="sr-only">{canvasHint(interactive, picking, snipping)}</p>
      {snipping && <SnipOverlay drawRect={stream.drawRect} pageRect={stream.pageRect} onFinish={onSnip} onCancel={onCancelSnip} />}
      {children}
    </div>
  );
}
