import { useEffect, useRef, useState } from "react";
import QRCode from "qrcode";
import { Copy, ExternalLink, ShieldCheck, Maximize2, Minimize2 } from "lucide-react";
import logoUrl from "../../assets/logo.png";
import type { AppConfig } from "../../lib/types";
import type { ServeStatus } from "../../lib/webAccess";
import { BusyState } from "../shared/BusyState";
import { InlineErrorNotice } from "../shared/ErrorNotice";
import { Skeleton, SkeletonBlock } from "../shared/Skeleton";
import { trapDialogTab } from "../shared/dialogFocusTrap";
import { Button } from "../shared/Button";
import { UI_TEXT } from "../shared/design";
import { buildWebUiTarget, canShareWebUiRoute, classifyWebUiRoute, displayedWebUrl, shouldShowWebSetupHelper, webRouteLabel, webUiListenerReady, type NetworkInfo } from "./webUiTarget";
import { deriveWebUiView } from "./meshWebUi";

export type WebUiAction = "setup" | "repair" | "disable";

export interface WebAccessActions {
  open: (link: string) => void;
  copy: (link: string) => Promise<void>;
  qr: (link: string) => Promise<string>;
}

export const generateWebQr = (link: string) => QRCode.toDataURL(link, { errorCorrectionLevel: "H", margin: 2, width: 1024, color: { dark: "#0d1117", light: "#ffffff" } });

interface Props {
  config: AppConfig;
  network: NetworkInfo | null;
  serve: ServeStatus | null;
  loading: boolean;
  actions: WebAccessActions;
  onAction: (action: WebUiAction) => Promise<string | void>;
  onSetup: () => Promise<void>;
  onClose: () => void;
  embedded?: boolean;
  qrSize?: number;
  onExpand?: () => void;
}

/** The card's loaded layout in placeholder blocks, at its final size: host line, the framed QR square, the address
 *  field, the route line, Open and Copy link, and one setup action. Swapping in the content moves nothing around it. */
function WebAccessSkeleton({ embedded, qrSize }: { embedded: boolean; qrSize: number }) {
  return <>
    <span role="status" className="sr-only">Loading Mesh UI</span>
    <Skeleton data-testid="web-access-skeleton" className={`flex flex-col ${embedded ? "gap-2" : "gap-3"}`}>
      <SkeletonBlock width="55%" height={14} className="-mt-1" />
      <div className="my-2 flex justify-center">
        {/* The QR's gradient frame (3px) and white mat (12px) around the code, as in the loaded card. */}
        <SkeletonBlock shape="tile" width={qrSize + 30} height={qrSize + 30} />
      </div>
      {!embedded && <SkeletonBlock width="100%" height={34} />}
      <SkeletonBlock width="70%" height={14} />
      <div className={`${embedded ? "my-1" : "my-3"} grid grid-cols-2 gap-2`}>
        <SkeletonBlock shape="tile" width="100%" height="var(--ct-control-height)" />
        <SkeletonBlock shape="tile" width="100%" height="var(--ct-control-height)" />
      </div>
      <SkeletonBlock shape="tile" width="100%" height="var(--ct-control-height)" />
    </Skeleton>
  </>;
}

export function WebAccessPanel({ config, network, serve, loading, actions, onAction, onSetup, onClose, embedded = false, qrSize = 228, onExpand }: Props) {
  const ready = webUiListenerReady(network, serve);
  const target = buildWebUiTarget(config, network, serve);
  const route = classifyWebUiRoute(config, network, serve);
  const shareable = canShareWebUiRoute(route.kind, ready);
  const view = deriveWebUiView(serve?.snapshot, loading);
  const displayUrl = displayedWebUrl(target.link);
  const host = new URL(displayUrl).host;
  const [qr, setQr] = useState<{ link: string; image: string } | null>(null);
  const [error, setError] = useState<unknown>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const panelRef = useRef<HTMLDivElement>(null);
  const headingRef = useRef<HTMLHeadingElement>(null);

  useEffect(() => {
    if (embedded) return;
    const origin = document.activeElement as HTMLElement | null;
    headingRef.current?.focus();
    return () => origin?.focus();
  }, [embedded]);

  useEffect(() => {
    if (!shareable) return;
    let active = true;
    void actions.qr(target.link).then((image) => {
      if (active) setQr({ link: target.link, image });
    }).catch((cause: unknown) => { if (active) setError(cause); });
    return () => { active = false; };
  }, [shareable, target.link, actions.qr]);

  async function run(operation: () => Promise<string | void>) {
    setBusy(true);
    setError(null);
    setMessage(null);
    try {
      const result = await operation();
      if (result) setMessage(result);
    } catch (cause) {
      setError(cause);
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className={embedded ? "w-full" : "fixed inset-0 z-40 flex items-center justify-center bg-[color-mix(in_srgb,var(--ct-bg)_70%,transparent)] p-4"} onClick={embedded ? undefined : onClose}>
      <div
        ref={panelRef}
        role={embedded ? "region" : "dialog"}
        aria-modal={embedded ? undefined : true}
        aria-label={embedded ? "Mesh UI" : undefined}
        aria-labelledby={embedded ? undefined : "web-access-heading"}
        data-testid="web-access-panel"
        data-web-ui-state={view.state}
        className={`relative flex w-full flex-col text-[var(--ct-fg)] text-xs leading-relaxed ${embedded ? "ct-card gap-2 p-3" : "ct-menu gap-3 p-4 z-50 max-h-full max-w-[360px] overflow-y-auto"}`}
        onClick={(event) => event.stopPropagation()}
        onKeyDown={(event) => {
          if (embedded) return;
          if (event.code === "Escape") { event.stopPropagation(); onClose(); }
          trapDialogTab(event, panelRef.current);
        }}
      >
        <div className="flex items-center justify-between gap-2">
          <div className="flex min-w-0 items-center gap-2">
          <h2 id={embedded ? undefined : "web-access-heading"} ref={headingRef} tabIndex={embedded ? undefined : -1} className="text-sm font-semibold">Mesh UI</h2>
          {(loading || busy) && <BusyState inline hideLabel label="Loading" phase="work" />}
          </div>
          {embedded && onExpand && <Button type="button" aria-label="Expand Mesh UI" title="Expand Mesh UI" variant="icon" size="sm" onClick={onExpand}><Maximize2 size={16} /></Button>}
          {!embedded && <Button type="button" aria-label="Collapse Mesh UI" onClick={onClose} variant="icon" size="sm"><Minimize2 size={16} /></Button>}
        </div>
        {loading && !ready && <WebAccessSkeleton embedded={embedded} qrSize={qrSize} />}
        {ready && <p className="truncate text-xs text-[var(--ct-muted-fg)]">{host}</p>}
        {!ready && !loading && <p role="status">No verified web UI endpoint is available. Check the connection or set up Tailscale.</p>}
        {!ready && view.state !== "ready" && view.state !== "loading" && <p role="status" className="text-xs" data-testid="web-access-status">{view.label}</p>}
        {view.detail && !embedded && <p className={UI_TEXT.metaMuted}>{view.detail}</p>}
        {ready && <>
        {shareable && <div className="my-2 flex shrink-0 justify-center">
          <div className="rounded-2xl p-[3px] shadow-[0_10px_30px_rgba(31,111,235,0.25)]" style={{ background: "linear-gradient(135deg, var(--ct-accent, #58a6ff) 0%, var(--ct-magenta, #bc8cff) 100%)" }}>
            <div className="relative rounded-[15px] bg-white p-3">
              {qr?.link === target.link ? <>
                <img src={qr.image} alt={`Open Domios Web on ${host}`} width={qrSize} height={qrSize} className="block" style={{ width: embedded ? qrSize : `min(${qrSize}px, calc(100vw - 112px), max(80px, calc(100dvh - 380px)))`, height: embedded ? qrSize : "auto" }} />
                <div aria-hidden="true" className="pointer-events-none absolute inset-0 flex items-center justify-center">
                  <div className="rounded-[11px] bg-white p-1.5 shadow-[0_2px_8px_rgba(0,0,0,0.18)]"><img src={logoUrl} alt="" className="h-9 w-9 rounded-lg" /></div>
                </div>
              </> : <span role="status" className="block" style={{ width: qrSize, height: qrSize }}>
                <span className="sr-only">Generating QR…</span>
                <Skeleton inline><SkeletonBlock shape="tile" width={qrSize} height={qrSize} /></Skeleton>
              </span>}
            </div>
          </div>
        </div>}
        {!embedded && <input readOnly aria-label="Web address" value={displayUrl} onFocus={(event) => event.currentTarget.select()} className="w-full min-w-0 rounded border border-[var(--ct-border)] bg-transparent p-2 text-xs" />}
        <p className="flex items-start gap-1.5 text-xs text-[var(--ct-muted-fg)]" data-testid="web-access-route"><ShieldCheck size={14} className="shrink-0" />{webRouteLabel(route.kind)}</p>
        <div className={`${embedded ? "my-1" : "my-3"} grid grid-cols-2 gap-2`}>
          <Button type="button" variant="accent" className="whitespace-normal" onClick={() => actions.open(target.link)}><ExternalLink size={15} aria-hidden="true" />{shareable ? "Open" : "Open on this machine"}</Button>
          {shareable && <Button type="button" disabled={busy} variant="ghost" className="whitespace-normal" onClick={() => void run(async () => { await actions.copy(target.link); return "Link copied."; })}><Copy size={15} aria-hidden="true" />Copy link</Button>}
        </div>
        </>}
        <div className="flex flex-col gap-2">
          {view.canSetup && <Button type="button" disabled={busy} variant="accent" className="whitespace-normal" onClick={() => void run(() => onAction("setup"))}>{view.operatorBootstrap ? "Set up Tailscale operator" : "Enable HTTPS (Tailscale Serve)"}</Button>}
          {view.canRepair && <Button type="button" disabled={busy} variant="ghost" className="whitespace-normal" onClick={() => void run(() => onAction("repair"))}>Repair route</Button>}
          {shouldShowWebSetupHelper(route.kind, ready) && <Button type="button" disabled={busy} variant="ghost" className="whitespace-normal" onClick={() => void run(onSetup)}>Set up Tailscale with AI</Button>}
        </div>
        {message && <p role="status">{message}</p>}
        {error != null && !embedded && <InlineErrorNotice error={error} operation="setting up web access" blocking />}
      </div>
    </div>
  );
}
