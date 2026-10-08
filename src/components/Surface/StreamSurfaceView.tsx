import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { TextPromptDialog } from "../shared/TextPromptDialog";
import { useSurfaceTakeControl } from "../../lib/surfaceControlStore";
import { frameBytes, type FrameDecoder } from "./frameFeed";
import { createLatestWins, type LatestWins } from "./latestWins";
import { PointAndAsk } from "./PointAndAsk";
import { StreamCanvas } from "./StreamCanvas";
import { decideKey, type PagePoint, type StreamInput } from "./streamModel";
import {
  addressSubmission,
  askPopoverPosition,
  blockedNotice,
  CHROME_ITEM,
  elementCard,
  failureNotice,
  navigationFailureNotice,
  fileNameFromPath,
  grabSendArgs,
  isNeedsHuman,
  needsHumanNotice,
  profileSwitchedNotice,
  resolveToolbar,
  screenshotSavedNotice,
  selectChoice,
  sentNotice,
  surfaceLabel,
  toolbarInvocation,
  validateProfileName,
  withOwner,
  type AskIntent,
  type GrabSelection,
  type NeedsHumanKind,
  type PopoverAnchor,
  type SurfaceChromeSpec,
  type SurfaceState,
  type ToolbarItemSpec,
} from "./surfaceChromeModel";
import { SurfaceFlowLine } from "./SurfaceFlowLine";
import { SurfaceNoticeBar } from "./SurfaceNoticeBar";
import { SurfacePreviewPaused } from "./SurfacePreviewPaused";
import { inputArgs, SURFACE_METHOD, type ScreenshotResult, type SurfaceNotice, type SurfaceStreamTransport } from "./surfaceStreamTransport";
import { SurfaceToolbar } from "./SurfaceToolbar";
import { useSurfaceStream } from "./useSurfaceStream";

export interface StreamSurfaceViewProps {
  transport: SurfaceStreamTransport;
  chrome: SurfaceChromeSpec;
  surfaceTitle: string;
  initialState: SurfaceState;
  ownerName?: string | null;
  /** Shares Take control with the sidebar tether and drives the flow line. */
  surfaceTabId?: string;
  decode?: FrameDecoder;
  saveScreenshot?(shot: ScreenshotResult): void;
}

interface Picked {
  selection: GrabSelection;
  position: { left: number; top: number };
}

const EDITABLE_SELECTOR = "input, textarea, select, [contenteditable='true']";

interface ProfilesResult {
  current?: string | null;
}

export function downloadScreenshot(shot: ScreenshotResult) {
  const url = URL.createObjectURL(new Blob([frameBytes(shot.base64)], { type: "image/png" }));
  const link = document.createElement("a");
  link.href = url;
  link.download = fileNameFromPath(shot.path);
  link.click();
  URL.revokeObjectURL(url);
}

export function StreamSurfaceView({ transport, chrome, surfaceTitle, initialState, ownerName, surfaceTabId, decode, saveScreenshot = downloadScreenshot }: StreamSurfaceViewProps) {
  const stream = useSurfaceStream(transport, initialState);
  const { notice, setNotice } = stream;
  const state = withOwner(stream.state, ownerName);
  const [interactive, setInteractive] = useSurfaceTakeControl(surfaceTabId);
  const [picking, setPicking] = useState(false);
  const [picked, setPicked] = useState<Picked | null>(null);
  const [busy, setBusy] = useState<ReadonlySet<string>>(() => new Set());
  const [sending, setSending] = useState(false);
  const [naming, setNaming] = useState<ToolbarItemSpec | null>(null);
  const [dismissedNeed, setDismissedNeed] = useState<NeedsHumanKind | null>(null);
  const hover = useRef<LatestWins<PagePoint> | null>(null);
  const needKind = isNeedsHuman(state.needsHuman) ? state.needsHuman : null;

  useEffect(() => {
    setDismissedNeed((dismissed) => (dismissed === needKind ? dismissed : null));
  }, [needKind]);

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
  const needText = needKind !== dismissedNeed ? needsHumanNotice(state) : null;
  const shownNotice: SurfaceNotice | null = notice ?? (needText ? { text: needText, tone: "warn" } : null);
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

  const declared = (id: string) => chrome.toolbar.find((item) => item.id === id);

  const runDeclared = <T,>(spec: ToolbarItemSpec, value?: string): Promise<T | undefined> => {
    const { method, args } = toolbarInvocation(spec, value);
    return runBusy<T>(spec.id, method, args).catch((error: unknown) => {
      if (method === "nav") {
        const text = navigationFailureNotice(typeof args.target === "string" ? args.target : undefined, error);
        if (text) setNotice({ text, tone: "error" });
      } else {
        report(spec.label, error);
      }
      return undefined;
    });
  };

  const invokeDeclared = (id: string, value?: string) => {
    const spec = declared(id);
    if (spec) void runDeclared(spec, value);
  };

  const applySelection = (spec: ToolbarItemSpec, value: string) => {
    if (spec.source !== "profiles") {
      void runDeclared(spec, value);
      return;
    }
    void runDeclared<ProfilesResult>(spec, value).then((result) => {
      if (result) setNotice({ text: profileSwitchedNotice(result.current) });
    });
  };

  const onSelect = (id: string, value: string) => {
    const spec = declared(id);
    if (!spec) return;
    const choice = selectChoice(spec, value);
    if (choice.kind === "new-profile") setNaming(spec);
    else applySelection(spec, choice.value);
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
        onSelect={onSelect}
      />
      {surfaceTabId && <SurfaceFlowLine surfaceTabId={surfaceTabId} />}
      <StreamCanvas
        label={label}
        pageTitle={state.title}
        interactive={interactive}
        picking={picking}
        frames={stream.frames}
        decode={decode}
        onViewport={stream.onViewport}
        onHidden={stream.onHidden}
        onInput={onInput}
        onPick={onPick}
        onHover={(point) => { if (!picked) hover.current?.push(point); }}
        onBlocked={() => setNotice({ text: blockedNotice(state.owner) })}
        onExitInteraction={() => setInteractive(false)}
        onCancelPick={cancelPick}
        onError={(error) => report("Frame", error)}
      >
        {stream.paused && <SurfacePreviewPaused notice={stream.paused} />}
        {shownNotice && <SurfaceNoticeBar notice={shownNotice} onDismiss={() => (notice ? setNotice(null) : setDismissedNeed(needKind))} />}
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
      </StreamCanvas>
      {naming && (
        <TextPromptDialog
          title="New profile"
          label="Profile name"
          description="A named profile keeps logins and cookies across restarts."
          confirmLabel="Create"
          validate={validateProfileName}
          onCancel={() => setNaming(null)}
          onSubmit={(name) => {
            setNaming(null);
            applySelection(naming, name);
          }}
        />
      )}
    </section>
  );
}
