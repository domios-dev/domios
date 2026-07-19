import { useCallback, useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import {
  AlertTriangle,
  Check,
  ChevronDown,
  Copy,
  Globe,
  Power,
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
  /** Run a mutating action; resolve with an inline message, or throw to show an error. */
  onAction: (action: WebUiAction) => Promise<string | void>;
  touchUi?: boolean;
}

const TONE_COLOR: Record<WebUiTone, string> = {
  ready: "var(--ct-green, #3fb950)",
  warn: "var(--ct-yellow, #d29922)",
  conflict: "var(--ct-orange, #db6d28)",
  error: "var(--ct-red, #f85149)",
  neutral: "var(--ct-muted, #8b949e)",
};

export function MeshWebUiControl({ snapshot, loading, onOpenSurface, onAction, touchUi }: Props) {
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
    setOpen(false);
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
    if (!view.url) return;
    setOpen(false);
    setBusy("copy");
    setError(null);
    try {
      await navigator.clipboard.writeText(view.url);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1400);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(null);
    }
  }, [view.url]);

  const hasMenu = view.canSetup || view.canRepair || view.canCopyUrl || view.canDisable;
  const busyLabel =
    busy === "setup" ? "Setting up…"
    : busy === "repair" ? "Repairing…"
    : busy === "disable" ? "Disabling…"
    : null;

  const pos = (() => {
    if (!open || !triggerRef.current) return null;
    const rect = triggerRef.current.getBoundingClientRect();
    return { top: rect.bottom + 4, right: Math.max(4, window.innerWidth - rect.right) };
  })();

  const btnH = touchUi ? "h-8" : "h-6";
  const chipColor = TONE_COLOR[view.tone];

  return (
    <div
      data-testid="mesh-web-ui-action"
      data-web-ui-state={view.state}
      className="flex items-center gap-1.5 shrink-0"
      title={view.detail ?? `Web UI: ${view.label}`}
    >
      <button
        type="button"
        data-testid="mesh-web-ui-surface"
        onClick={onOpenSurface}
        className="flex items-center gap-1 rounded px-1 text-[var(--ct-muted-fg)] hover:bg-[var(--ct-hover)] cursor-pointer bg-transparent border-none"
        title="Show Web UI link & QR"
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
      </button>

      {hasMenu && (
        <button
          ref={triggerRef}
          type="button"
          data-testid="mesh-web-ui-menu-trigger"
          aria-haspopup="menu"
          aria-expanded={open}
          onClick={() => setOpen((v) => !v)}
          className={`flex ${btnH} w-6 items-center justify-center rounded border border-[var(--ct-border)] text-[var(--ct-muted-fg)] hover:text-[var(--ct-fg)] hover:border-[var(--ct-accent)] cursor-pointer bg-transparent`}
          title="Web UI options"
        >
          {busyLabel ? <BusyState label="" inline hideLabel className="scale-50" /> : <ChevronDown size={12} />}
        </button>
      )}

      {busyLabel && (
        <span data-testid="mesh-web-ui-progress" className="flex items-center gap-1 text-[10px] text-[var(--ct-muted-fg)]">
          <BusyState label="" inline hideLabel className="scale-50" />
          {busyLabel}
        </span>
      )}
      {copied && (
        <span data-testid="mesh-web-ui-copied" className="text-[10px] text-[var(--ct-green,#3fb950)]">Copied</span>
      )}
      {!busyLabel && message && (
        <span data-testid="mesh-web-ui-message" className="text-[10px] text-[var(--ct-muted-fg)] truncate max-w-[220px]">{message}</span>
      )}
      {!busyLabel && error && (
        <span data-testid="mesh-web-ui-error" className="flex items-center gap-1 text-[10px] text-[var(--ct-red,#f85149)] truncate max-w-[220px]">
          <AlertTriangle size={10} className="shrink-0" />
          {error}
        </span>
      )}

      {open && pos && createPortal(
        <div
          ref={menuRef}
          data-testid="mesh-web-ui-menu"
          role="menu"
          className="ct-portal-menu fixed w-[248px] overflow-hidden rounded-lg border border-[var(--ct-border)] shadow-2xl backdrop-blur-xl"
          style={{ top: pos.top, right: pos.right, background: "var(--ct-surface, #1e1e2e)" }}
        >
          <div className="px-3 pt-2.5 pb-1 text-[10px] uppercase tracking-wider text-[var(--ct-muted)]">
            Web UI · {view.label}
          </div>
          {view.detail && (
            <p className="px-3 pb-2 text-[11px] leading-snug text-[var(--ct-muted-fg)]">{view.detail}</p>
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
          {view.canCopyUrl && view.url && (
            <MenuItem
              testid="mesh-web-ui-copy-url"
              icon={copied ? <Check size={13} className="text-[var(--ct-green,#3fb950)]" /> : <Copy size={13} />}
              label="Copy URL"
              sub={view.url}
              onClick={() => void copyUrl()}
            />
          )}
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
