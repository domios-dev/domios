import { useEffect, useRef, useState } from "react";
import QRCode from "qrcode";
import { X } from "lucide-react";
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

export const generateWebQr = (link: string) => QRCode.toDataURL(link, { errorCorrectionLevel: "H", margin: 1, width: 320 });

interface Props {
  config: AppConfig;
  network: NetworkInfo | null;
  serve: ServeStatus | null;
  loading: boolean;
  actions: WebAccessActions;
  onAction: (action: WebUiAction) => Promise<string | void>;
  onSetup: () => Promise<void>;
  onClose: () => void;
}

export function WebAccessPanel({ config, network, serve, loading, actions, onAction, onSetup, onClose }: Props) {
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
    const origin = document.activeElement as HTMLElement | null;
    headingRef.current?.focus();
    return () => origin?.focus();
  }, []);

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
    <div className="absolute inset-0 z-40 bg-black/30" onClick={onClose}>
      <div
        ref={panelRef}
        role="dialog"
        aria-modal="true"
        aria-labelledby="web-access-heading"
        data-testid="web-access-panel"
        data-web-ui-state={view.state}
        className={"absolute inset-y-0 right-0 z-50 flex w-full max-w-[390px] flex-col gap-3 overflow-y-auto bg-[var(--ct-bg)] p-4 pb-[max(1rem,var(--ct-safe-bottom,0px))] shadow-2xl"}
        onClick={(event) => event.stopPropagation()}
        onKeyDown={(event) => {
          if (event.code === "Escape") { event.stopPropagation(); onClose(); }
          trapDialogTab(event, panelRef.current);
        }}
      >
        <div className="flex items-center justify-between gap-2">
          <h2 id="web-access-heading" ref={headingRef} tabIndex={-1} className="text-base font-semibold">Web</h2>
          <button type="button" aria-label="Close Web" onClick={onClose} className={`${UI_BUTTON.ghost} ${UI_BUTTON.icon}`}><X size={16} /></button>
        </div>
        <p role="status" className="text-sm" data-testid="web-access-status">{view.label}</p>
        {view.detail && <p className={UI_TEXT.metaMuted}>{view.detail}</p>}
        <p className="break-words text-sm" data-testid="web-access-route">{route.reason}</p>
        <div className="my-4 flex shrink-0 justify-center">
          {qr?.link === target.link
            ? <img src={qr.image} alt={`Open Domios Web on ${host}`} width={228} height={228} className="h-[228px] w-[228px] rounded-lg bg-white" />
            : <span role="status">Generating QR…</span>}
        </div>
        <input readOnly aria-label="Web address" value={displayUrl} onFocus={(event) => event.currentTarget.select()} className="w-full min-w-0 rounded border border-[var(--ct-border)] bg-transparent p-2 text-xs" />
        <div className="my-3 grid grid-cols-2 gap-2">
          <button type="button" className={`${UI_BUTTON.accent} ${UI_BUTTON.lg}`} onClick={() => actions.open(target.link)}>Open</button>
          <button type="button" disabled={busy} className={`${UI_BUTTON.ghost} ${UI_BUTTON.lg}`} onClick={() => void run(async () => { await actions.copy(target.link); return "Link copied."; })}>Copy link</button>
        </div>
        <div className="flex flex-col gap-2">
          {view.canSetup && <button type="button" disabled={busy} className={`${UI_BUTTON.accent} ${UI_BUTTON.lg}`} onClick={() => void run(() => onAction("setup"))}>{view.operatorBootstrap ? "Set up Tailscale operator" : "Enable HTTPS (Tailscale Serve)"}</button>}
          {view.canRepair && <button type="button" disabled={busy} className={`${UI_BUTTON.ghost} ${UI_BUTTON.lg}`} onClick={() => void run(() => onAction("repair"))}>Repair route</button>}
          {view.canDisable && <button type="button" disabled={busy} className={`${UI_BUTTON.ghost} ${UI_BUTTON.lg}`} onClick={() => void run(() => onAction("disable"))}>Disable Web UI</button>}
          <button type="button" disabled={busy} className={`${UI_BUTTON.ghost} ${UI_BUTTON.lg}`} onClick={() => void run(onSetup)}>Set up Tailscale with AI</button>
        </div>
        {busy && <p role="status">Working…</p>}
        {message && <p role="status">{message}</p>}
        {error != null && <InlineErrorNotice error={error} operation="setting up web access" blocking />}
      </div>
    </div>
  );
}
