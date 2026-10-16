import { useEffect, useRef, useState } from "react";
import QRCode from "qrcode";
import { Copy, ExternalLink, ShieldCheck, X } from "lucide-react";
import logoUrl from "../../assets/logo.png";
import type { AppConfig } from "../../lib/types";
import type { ServeStatus } from "../../lib/webAccess";
import { InlineErrorNotice } from "../shared/ErrorNotice";
import { trapDialogTab } from "../shared/dialogFocusTrap";
import { UI_BUTTON, UI_TEXT } from "../shared/design";
import { buildWebUiTarget, classifyWebUiRoute, displayedWebUrl, type NetworkInfo } from "./webUiTarget";
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
}

export function WebAccessPanel({ config, network, serve, loading, actions, onAction, onSetup, onClose, embedded = false, qrSize = 228 }: Props) {
  const target = buildWebUiTarget(config, network, serve);
  const route = classifyWebUiRoute(config, network, serve);
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
    let active = true;
    void actions.qr(target.link).then((image) => {
      if (active) setQr({ link: target.link, image });
    }).catch((cause: unknown) => { if (active) setError(cause); });
    return () => { active = false; };
  }, [target.link, actions.qr]);

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
    <div className={embedded ? "w-full" : "absolute inset-0 z-40 flex items-center justify-center overflow-y-auto bg-black/55 p-4 backdrop-blur-[2px]"} onClick={embedded ? undefined : onClose}>
      <div
        ref={panelRef}
        role={embedded ? "region" : "dialog"}
        aria-modal={embedded ? undefined : true}
        aria-label={embedded ? "Web" : undefined}
        aria-labelledby={embedded ? undefined : "web-access-heading"}
        data-testid="web-access-panel"
        data-web-ui-state={view.state}
        className={`relative flex w-full flex-col gap-3 rounded-2xl border border-[var(--ct-accent)] bg-[var(--ct-bg)] p-4 shadow-2xl ${embedded ? "text-base" : "z-50 max-h-full max-w-[420px] overflow-y-auto"}`}
        onClick={(event) => event.stopPropagation()}
        onKeyDown={(event) => {
          if (embedded) return;
          if (event.code === "Escape") { event.stopPropagation(); onClose(); }
          trapDialogTab(event, panelRef.current);
        }}
      >
        <div className="flex items-center justify-between gap-2">
          <h2 id={embedded ? undefined : "web-access-heading"} ref={headingRef} tabIndex={embedded ? undefined : -1} className={embedded ? "text-3xl font-semibold" : "text-xl font-semibold"}>Web</h2>
          {!embedded && <button type="button" aria-label="Close Web" onClick={onClose} className={`${UI_BUTTON.ghost} ${UI_BUTTON.icon}`}><X size={16} /></button>}
        </div>
        <p className="-mt-2 truncate text-sm text-[var(--ct-muted-fg)]">{host}</p>
        <p role="status" className="text-sm" data-testid="web-access-status">{view.label}</p>
        {view.detail && !embedded && <p className={UI_TEXT.metaMuted}>{view.detail}</p>}
        <div className="my-2 flex shrink-0 justify-center">
          <div className="rounded-2xl p-[3px] shadow-[0_10px_30px_rgba(31,111,235,0.25)]" style={{ background: "linear-gradient(135deg, var(--ct-accent, #58a6ff) 0%, var(--ct-magenta, #bc8cff) 100%)" }}>
            <div className="relative rounded-[15px] bg-white p-3">
              {qr?.link === target.link ? <>
                <img src={qr.image} alt={`Open Domios Web on ${host}`} width={qrSize} height={qrSize} className="block" style={{ width: qrSize, height: qrSize }} />
                <div aria-hidden="true" className="pointer-events-none absolute inset-0 flex items-center justify-center">
                  <div className="rounded-[11px] bg-white p-1.5 shadow-[0_2px_8px_rgba(0,0,0,0.18)]"><img src={logoUrl} alt="" className="h-9 w-9 rounded-lg" /></div>
                </div>
              </> : <span role="status" className="flex items-center justify-center text-sm text-slate-400" style={{ width: qrSize, height: qrSize }}>Generating QR…</span>}
            </div>
          </div>
        </div>
        {!embedded && <input readOnly aria-label="Web address" value={displayUrl} onFocus={(event) => event.currentTarget.select()} className="w-full min-w-0 rounded border border-[var(--ct-border)] bg-transparent p-2 text-xs" />}
        <p className="flex items-start gap-1.5 text-sm text-[var(--ct-muted-fg)]" data-testid="web-access-route"><ShieldCheck size={14} className="shrink-0" />{route.kind.replaceAll("_", " ")} · {route.reason}</p>
        {embedded && <p className="text-sm text-[var(--ct-muted-fg)]">Machines connect via this machine's gateway.</p>}
        <div className="my-3 grid grid-cols-2 gap-2">
          <button type="button" style={{ minHeight: 48, fontSize: 16 }} className={`${UI_BUTTON.accent} ${UI_BUTTON.lg} min-h-12 text-base`} onClick={() => actions.open(target.link)}><ExternalLink size={15} aria-hidden="true" />Open</button>
          <button type="button" style={{ minHeight: 48, fontSize: 16 }} disabled={busy} className={`${UI_BUTTON.ghost} ${UI_BUTTON.lg} min-h-12 text-base`} onClick={() => void run(async () => { await actions.copy(target.link); return "Link copied."; })}><Copy size={15} aria-hidden="true" />Copy link</button>
        </div>
        <div className="flex flex-col gap-2">
          {view.canSetup && <button type="button" style={{ minHeight: 48, fontSize: 16 }} disabled={busy} className={`${UI_BUTTON.accent} ${UI_BUTTON.lg} min-h-12 text-base`} onClick={() => void run(() => onAction("setup"))}>{view.operatorBootstrap ? "Set up Tailscale operator" : "Enable HTTPS (Tailscale Serve)"}</button>}
          {view.canRepair && <button type="button" style={{ minHeight: 48, fontSize: 16 }} disabled={busy} className={`${UI_BUTTON.ghost} ${UI_BUTTON.lg} min-h-12 text-base`} onClick={() => void run(() => onAction("repair"))}>Repair route</button>}
          {view.canDisable && <button type="button" style={{ minHeight: 48, fontSize: 16 }} disabled={busy} className={`${UI_BUTTON.ghost} ${UI_BUTTON.lg} min-h-12 text-base`} onClick={() => void run(() => onAction("disable"))}>Disable Web UI</button>}
          <button type="button" style={{ minHeight: 48, fontSize: 16 }} disabled={busy} className={`${UI_BUTTON.ghost} ${UI_BUTTON.lg} min-h-12 text-base`} onClick={() => void run(onSetup)}>Set up Tailscale with AI</button>
        </div>
        {busy && <p role="status">Working…</p>}
        {message && <p role="status">{message}</p>}
        {error != null && <InlineErrorNotice error={error} operation="setting up web access" blocking />}
      </div>
    </div>
  );
}
