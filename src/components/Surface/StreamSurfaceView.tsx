import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { frameBytes, type FrameDecoder } from "./frameFeed";
import { createLatestWins, type LatestWins } from "./latestWins";
import { PointAndAsk } from "./PointAndAsk";
import { StreamCanvas } from "./StreamCanvas";
import { decideKey, type PagePoint, type StreamInput } from "./streamModel";
import { SurfaceCapsule } from "./SurfaceCapsule";
import {
  addressSubmission,
  askPopoverPosition,
  blockedNotice,
  capsuleModel,
  CHROME_ITEM,
  elementCard,
  failureNotice,
  fileNameFromPath,
  grabSendArgs,
  resolveToolbar,
  screenshotSavedNotice,
  sentNotice,
  surfaceLabel,
  toolbarInvocation,
  type AskIntent,
  type GrabSelection,
  type PopoverAnchor,
  type SurfaceChromeSpec,
  type SurfaceState,
} from "./surfaceChromeModel";
import { SurfaceNoticeBar } from "./SurfaceNoticeBar";
import { inputArgs, SURFACE_METHOD, type ScreenshotResult, type SurfaceStreamTransport } from "./surfaceStreamTransport";
import { SurfaceToolbar } from "./SurfaceToolbar";
import { useSurfaceStream } from "./useSurfaceStream";

export interface StreamSurfaceViewProps {
  transport: SurfaceStreamTransport;
  chrome: SurfaceChromeSpec;
  surfaceTitle: string;
  initialState: SurfaceState;
  decode?: FrameDecoder;
  saveScreenshot?(shot: ScreenshotResult): void;
}

interface Picked {
  selection: GrabSelection;
  position: { left: number; top: number };
}

const EDITABLE_SELECTOR = "input, textarea, select, [contenteditable='true']";

function downloadScreenshot(shot: ScreenshotResult) {
  const url = URL.createObjectURL(new Blob([frameBytes(shot.base64)], { type: "image/png" }));
  const link = document.createElement("a");
  link.href = url;
  link.download = fileNameFromPath(shot.path);
  link.click();
  URL.revokeObjectURL(url);
}

export function StreamSurfaceView({ transport, chrome, surfaceTitle, initialState, decode, saveScreenshot = downloadScreenshot }: StreamSurfaceViewProps) {
  const stream = useSurfaceStream(transport, initialState);
  const { state, notice, setNotice } = stream;
  const [interactive, setInteractive] = useState(false);
  const [picking, setPicking] = useState(false);
  const [picked, setPicked] = useState<Picked | null>(null);
  const [busy, setBusy] = useState<ReadonlySet<string>>(() => new Set());
  const [sending, setSending] = useState(false);
  const hover = useRef<LatestWins<PagePoint> | null>(null);

  useEffect(() => {
    // Hover highlight is cosmetic; a failed hover must not raise a notice per mouse move.
    const queue = createLatestWins<PagePoint>((point) => transport.invoke(SURFACE_METHOD.grabHover, { ...point }).then(() => undefined));
    hover.current = queue;
    return () => {
      queue.dispose();
      hover.current = null;
    };
  }, [transport]);

  const label = surfaceLabel(state.owner, surfaceTitle);
  const items = resolveToolbar(chrome, { interactive, picking, state, busy });
  const report = (action: string, error: unknown) => setNotice({ text: failureNotice(action, error), tone: "error" });

  const markBusy = (id: string, on: boolean) => setBusy((current) => {
    const next = new Set(current);
    if (on) next.add(id);
    else next.delete(id);
    return next;
  });

  const runBusy = async <T,>(id: string, method: string, args: Record<string, unknown>): Promise<T> => {
    markBusy(id, true);
    try {
      return await transport.invoke<T>(method, args);
    } finally {
      markBusy(id, false);
    }
  };

  const invokeDeclared = (id: string, value?: string) => {
    const spec = chrome.toolbar.find((item) => item.id === id);
    if (!spec) return;
    const { method, args } = toolbarInvocation(spec, value);
    runBusy(id, method, args).catch((error: unknown) => report(spec.label, error));
  };

  const takeScreenshot = (full: boolean) => {
    runBusy<ScreenshotResult>(CHROME_ITEM.screenshot, SURFACE_METHOD.screenshot, { full })
      .then((shot) => {
        saveScreenshot(shot);
        setNotice({ text: screenshotSavedNotice(shot.path) });
      })
      .catch((error: unknown) => report("Screenshot", error));
  };

  const cancelPick = () => {
    setPicking(false);
    setPicked(null);
    transport.invoke(SURFACE_METHOD.grabCancel).catch((error: unknown) => report("Pick", error));
  };

  const onToggle = (id: string) => {
    if (id === CHROME_ITEM.pick) {
      if (picking) cancelPick();
      else setPicking(true);
    } else if (id === CHROME_ITEM.takeControl) {
      setInteractive((value) => !value);
    }
  };

  const onPick = (point: PagePoint, anchor: PopoverAnchor) => {
    transport.invoke<GrabSelection>(SURFACE_METHOD.grabSelect, { ...point })
      .then((selection) => setPicked({ selection, position: askPopoverPosition(anchor) }))
      .catch((error: unknown) => report("Pick", error));
  };

  const onSend = (comment: string, intent: AskIntent) => {
    if (!picked) return;
    setSending(true);
    transport.invoke(SURFACE_METHOD.grabSend, grabSendArgs(picked.selection, comment, intent))
      .then(() => setNotice({ text: sentNotice(state.owner) }))
      .catch((error: unknown) => report("Send", error))
      .finally(() => {
        setSending(false);
        cancelPick();
      });
  };

  const onInput = (input: StreamInput) => {
    transport.invoke(SURFACE_METHOD.input, inputArgs(input)).catch((error: unknown) => report("Input", error));
  };

  const onRootKeyDown = (event: KeyboardEvent<HTMLElement>) => {
    const target = event.target as Element;
    const origin = target.closest?.(EDITABLE_SELECTOR) ? "editable" : "surface";
    const decision = decideKey(event.nativeEvent, { interactive, picking, origin });
    if (decision.action === "exit-interaction") {
      event.preventDefault();
      setInteractive(false);
    } else if (decision.action === "cancel-pick") {
      event.preventDefault();
      cancelPick();
    }
  };

  return (
    <section className="ct-surface" aria-label={label} data-testid="stream-surface" onKeyDown={onRootKeyDown}>
      <SurfaceToolbar
        label={label}
        items={items}
        onButton={(id, modifiers) => (id === CHROME_ITEM.screenshot ? takeScreenshot(modifiers.shiftKey) : invokeDeclared(id))}
        onToggle={onToggle}
        onSubmit={(id, raw) => {
          const value = addressSubmission(raw);
          if (value) invokeDeclared(id, value);
        }}
        onSelect={(id, value) => invokeDeclared(id, value)}
      />
      <StreamCanvas
        label={label}
        pageTitle={state.title}
        interactive={interactive}
        picking={picking}
        frames={stream.frames}
        decode={decode}
        onViewport={stream.onViewport}
        onInput={onInput}
        onPick={onPick}
        onHover={(point) => { if (!picked) hover.current?.push(point); }}
        onBlocked={() => setNotice({ text: blockedNotice(state.owner) })}
        onExitInteraction={() => setInteractive(false)}
        onCancelPick={cancelPick}
        onError={(error) => report("Frame", error)}
      >
        {notice && <SurfaceNoticeBar notice={notice} onDismiss={() => setNotice(null)} />}
        {picked && (
          <PointAndAsk
            owner={state.owner}
            card={elementCard(picked.selection)}
            position={picked.position}
            sending={sending}
            onSend={onSend}
            onCancel={cancelPick}
          />
        )}
        <SurfaceCapsule capsule={capsuleModel(state)} />
      </StreamCanvas>
    </section>
  );
}
