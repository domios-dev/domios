import { useCallback, useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import {
  AlertTriangle,
  Check,
  ChevronDown,
  Copy,
  ExternalLink,
  Globe,
  Power,
  QrCode,
  Settings2,
  Wrench,
} from "lucide-react";
import { BusyState } from "../shared/BusyState";
import type { ServeReconcileSnapshot } from "../../lib/webAccess";
import { deriveWebUiView, type WebUiTone } from "./meshWebUi";

export type WebUiAction = "setup" | "repair" | "disable";

interface Props {
  snapshot: ServeReconcileSnapshot | null;
  loading: boolean;
  /** Open the QR / link surface (phone-scannable). Available in every state. */
  onOpenSurface: () => void;
  onOpenUrl?: (url: string) => void;
  accessUrl?: string | null;
  /** Run a mutating action; resolve with an inline message, or throw to show an error. */
  onAction: (action: WebUiAction) => Promise<string | void>;
  touchUi?: boolean;
  menuAlign?: "left" | "right";
}

const TONE_COLOR: Record<WebUiTone, string> = {
  ready: "var(--ct-green, #3fb950)",
  warn: "var(--ct-yellow, #d29922)",
  conflict: "var(--ct-orange, #db6d28)",
  error: "var(--ct-red, #f85149)",
  neutral: "var(--ct-muted, #8b949e)",
};

export function MeshWebUiControl({
  snapshot,
  loading,
  onOpenSurface,
  onOpenUrl,
  accessUrl,
  onAction,
  touchUi,
  menuAlign = "right",
}: Props) {
  const view = deriveWebUiView(snapshot, loading);
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState<WebUiAction | "copy" | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const triggerRef = useRef<HTMLButtonElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const onClick = (e: MouseEvent) => {
      if (
        menuRef.current && !menuRef.current.contains(e.target as Node) &&
        triggerRef.current && !triggerRef.current.contains(e.target as Node)
      ) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => { if (e.key === "Escape") setOpen(false); };
    document.addEventListener("mousedown", onClick, true);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onClick, true);
      document.removeEventListener("keydown", onKey);
    };
  }, [open]);

  const runAction = useCallback(async (action: WebUiAction) => {
    setBusy(action);
    setError(null);
    setMessage(null);
    try {
      const res = await onAction(action);
      if (typeof res === "string" && res) setMessage(res);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(null);
    }
  }, [onAction]);

  const copyUrl = useCallback(async () => {
    const url = accessUrl ?? view.url;
    if (!url) return;
    setBusy("copy");
    setError(null);
    try {
      await navigator.clipboard.writeText(url);
      setCopied(true);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(null);
    }
  }, [accessUrl, view.url]);

  const openUrl = accessUrl ?? view.url;
  const busyLabel =
    busy === "setup" ? "Setting up…"
    : busy === "repair" ? "Repairing…"
    : busy === "disable" ? "Disabling…"
    : null;

  const pos = (() => {
    if (!open || !triggerRef.current) return null;
    const rect = triggerRef.current.getBoundingClientRect();
    return menuAlign === "left"
      ? { top: rect.bottom + 4, left: Math.max(4, rect.left) }
      : { top: rect.bottom + 4, right: Math.max(4, window.innerWidth - rect.right) };
  })();

  const btnH = touchUi ? "h-8" : "h-6";
  const chipColor = TONE_COLOR[view.tone];

  return (
    <div
      data-testid="mesh-web-ui-action"
      data-web-ui-state={view.state}
      className="shrink-0"
      title={`Web UI: ${view.label}`}
    >
      <button
        ref={triggerRef}
        type="button"
        data-testid="mesh-web-ui-menu-trigger"
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={() => {
          setCopied(false);
          setOpen((value) => !value);
        }}
        className={`flex ${btnH} items-center gap-1.5 rounded-md border-none bg-transparent px-1.5 text-[var(--ct-muted-fg)] transition-colors hover:bg-[var(--ct-hover)] hover:text-[var(--ct-fg)] cursor-pointer`}
        title="Web UI options"
      >
        <Globe size={touchUi ? 15 : 13} className="shrink-0" style={{ color: chipColor }} />
        <span className="text-[11px] font-medium text-[var(--ct-fg)]">Web UI</span>
        <span
          data-testid="mesh-web-ui-chip"
          className="inline-flex items-center gap-1 rounded px-1.5 py-0.5 text-[10px] font-medium leading-none"
          style={{ color: chipColor, background: "color-mix(in srgb, currentColor 12%, transparent)" }}
        >
          {loading && !snapshot && <BusyState label="" inline hideLabel className="scale-50" />}
          {view.tone === "error" && <AlertTriangle size={9} />}
          {view.label}
        </span>
        {busyLabel ? <BusyState label="" inline hideLabel className="scale-50" /> : <ChevronDown size={12} />}
      </button>

      {open && pos && createPortal(
        <div
          ref={menuRef}
          data-testid="mesh-web-ui-menu"
          role="menu"
          className="ct-portal-menu fixed w-[248px] overflow-hidden rounded-lg border border-[var(--ct-border)] shadow-lg"
          style={{ ...pos, background: "var(--ct-surface, #1e1e2e)" }}
        >
          <div className="px-3 pt-2.5 pb-1 text-[10px] uppercase tracking-wider text-[var(--ct-muted)]">
            Web UI · {view.label}
          </div>
          {view.tone !== "ready" && view.detail && (
            <p className="px-3 pb-2 text-[11px] leading-snug text-[var(--ct-muted-fg)]">{view.detail}</p>
          )}
          {busyLabel && (
            <div data-testid="mesh-web-ui-progress" className="flex items-center gap-1 px-3 pb-2 text-[10px] text-[var(--ct-muted-fg)]">
              <BusyState label="" inline hideLabel className="scale-50" />
              {busyLabel}
            </div>
          )}
          {!busyLabel && message && (
            <div data-testid="mesh-web-ui-message" className="px-3 pb-2 text-[10px] text-[var(--ct-muted-fg)]">{message}</div>
          )}
          {!busyLabel && error && (
            <div data-testid="mesh-web-ui-error" className="flex items-center gap-1 px-3 pb-2 text-[10px] text-[var(--ct-red,#f85149)]">
              <AlertTriangle size={10} className="shrink-0" />
              {error}
            </div>
          )}
          {openUrl && (
            <MenuItem
              testid="mesh-web-ui-open"
              icon={<ExternalLink size={13} />}
              label="Open Web UI"
              sub="Open the authenticated Web UI"
              onClick={() => {
                setOpen(false);
                onOpenUrl?.(openUrl);
              }}
            />
          )}
          {view.canSetup && (
            <MenuItem
              testid="mesh-web-ui-setup"
              icon={<Settings2 size={13} />}
              label={view.operatorBootstrap ? "Setup (operator bootstrap)" : "Set up Web UI"}
              sub={view.operatorBootstrap ? view.bootstrapCommand ?? undefined : "Configure Tailscale Serve"}
              onClick={() => void runAction("setup")}
            />
          )}
          {view.canRepair && (
            <MenuItem
              testid="mesh-web-ui-repair"
              icon={<Wrench size={13} />}
              label="Repair route"
              sub="Reconcile the desired HTTPS route"
              onClick={() => void runAction("repair")}
            />
          )}
          {openUrl && (
            <MenuItem
              testid="mesh-web-ui-copy-url"
              icon={copied ? <Check size={13} className="text-[var(--ct-green,#3fb950)]" /> : <Copy size={13} />}
              label={copied ? "Copied" : "Copy URL"}
              sub="Copy the authenticated link"
              onClick={() => void copyUrl()}
            />
          )}
          <MenuItem
            testid="mesh-web-ui-surface"
            icon={<QrCode size={13} />}
            label="Show QR code"
            sub="Open the link and QR surface"
            onClick={() => {
              setOpen(false);
              onOpenSurface();
            }}
          />
          {view.canDisable && (
            <MenuItem
              testid="mesh-web-ui-disable"
              icon={<Power size={13} />}
              danger
              label="Disable Web UI"
              sub="Remove only the route this daemon owns"
              onClick={() => void runAction("disable")}
            />
          )}
        </div>,
        document.body,
      )}
    </div>
  );
}

function MenuItem({
  testid, icon, label, sub, danger, onClick,
}: {
  testid: string;
  icon: React.ReactNode;
  label: string;
  sub?: string;
  danger?: boolean;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      role="menuitem"
      data-testid={testid}
      onClick={onClick}
      className={`flex w-full items-start gap-2.5 px-3 py-2 text-left transition-colors hover:bg-[rgba(255,255,255,0.06)] cursor-pointer bg-transparent border-none ${
        danger ? "text-[var(--ct-red,#f85149)]" : "text-[var(--ct-fg)]"
      }`}
    >
      <span className="mt-0.5 shrink-0">{icon}</span>
      <span className="flex min-w-0 flex-col">
        <span className="text-[12px] leading-tight">{label}</span>
        {sub && <span className="mt-0.5 truncate font-mono text-[10px] text-[var(--ct-muted)]">{sub}</span>}
      </span>
    </button>
  );
}
