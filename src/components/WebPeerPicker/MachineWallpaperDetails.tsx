import { useEffect, useState, type CSSProperties } from "react";
import { Sparkles } from "lucide-react";
import { copyMachineAppearance, undoMachineAppearance, type AppearanceCopyReceipt, type AppearanceTarget } from "../../lib/copyMachineAppearance";
import { readMachineAppearance, type MachineAppearance } from "../../lib/chatWallpaperClient";
import { useWallpaperAsset } from "../../lib/useMachineAppearance";
import { wallpaperRenderStyle } from "../../lib/chatWallpaper";
import { DEFAULT_THEME, isLightColor } from "../../lib/themeCss";
import type { MachineIdentity } from "../../lib/machineIdentity";

export function MachineWallpaperSwatch({ machine, identity, preview }: {
  machine: AppearanceTarget;
  identity?: MachineIdentity;
  preview?: MachineAppearance;
}) {
  const [remote, setRemote] = useState<MachineAppearance>();
  const { peerId, available } = machine;
  useEffect(() => {
    if (!available || preview) return;
    const controller = new AbortController();
    void readMachineAppearance(peerId, controller.signal).then(result => {
      if (!controller.signal.aborted) setRemote(result);
    }).catch(() => undefined);
    return () => controller.abort();
  }, [available, peerId, preview]);
  const look = preview ?? remote;
  const theme = look?.theme ?? DEFAULT_THEME;
  const asset = useWallpaperAsset(peerId, look?.wallpaper?.asset);
  return <span className="ct-machine-wallpaper-swatch" aria-hidden="true">
    <span className="ct-chat-wallpaper" style={{
      "--ct-bg": theme.background,
      "--ct-fg": theme.foreground,
      ...wallpaperRenderStyle(theme, look?.wallpaper ?? undefined, isLightColor(theme.background), asset?.url, identity?.color),
    } as CSSProperties} />
  </span>;
}

export function useMachineAppearanceCopy() {
  const [busy, setBusy] = useState(false);
  const [receipt, setReceipt] = useState<AppearanceCopyReceipt>();
  const [status, setStatus] = useState<string>();
  const [failures, setFailures] = useState<AppearanceCopyReceipt["failed"]>([]);

  async function apply(source: AppearanceTarget, targets: AppearanceTarget[]) {
    setBusy(true);
    setReceipt(undefined);
    setStatus("Copying machine appearance…");
    setFailures([]);
    try {
      const result = await copyMachineAppearance(source, targets);
      setReceipt(result.applied.length ? result : undefined);
      setFailures(result.failed);
      setStatus(`Copied to ${result.applied.length} ${result.applied.length === 1 ? "machine" : "machines"}`);
    } catch (error) {
      setStatus(error instanceof Error ? error.message : "The appearance could not be copied.");
    } finally {
      setBusy(false);
    }
  }

  async function undo() {
    if (!receipt) return;
    setBusy(true);
    try {
      const remaining = await undoMachineAppearance(receipt);
      setFailures(remaining);
      setStatus(remaining.length ? "Some machines could not be restored." : "Previous looks restored");
      setReceipt(undefined);
    } finally {
      setBusy(false);
    }
  }

  return { busy, receipt, status, failures, apply, undo, expire: () => setReceipt(undefined) };
}

export function MachineAppearanceActions({ source, targets, copy, onAsk }: {
  source: AppearanceTarget;
  targets: AppearanceTarget[];
  copy: ReturnType<typeof useMachineAppearanceCopy>;
  onAsk?: () => void;
}) {
  const [confirm, setConfirm] = useState(false);
  return <div className="ct-machine-appearance-actions" onClick={event => event.stopPropagation()}>
    {confirm ? <div className="ct-machine-appearance-confirm">
      <span>Copy {source.name}’s theme and wallpaper to {targets.length} {targets.length === 1 ? "machine" : "machines"}?</span>
      <button type="button" role="menuitem" autoFocus onClick={() => { setConfirm(false); void copy.apply(source, targets); }}>Copy</button>
      <button type="button" role="menuitem" onClick={() => setConfirm(false)}>Cancel</button>
    </div> : <button type="button" role="menuitem" className="ct-menu-row" disabled={copy.busy || !source.available || !targets.length}
      onClick={() => setConfirm(true)}>Use {source.name}’s look on all machines</button>}
    {onAsk && <button type="button" role="menuitem" className="ct-menu-row ct-machine-wallpaper-ask" onClick={onAsk}>
      <Sparkles size={14} aria-hidden="true" /> Ask General for {source.name}’s wallpaper
    </button>}
    {copy.status && <div className="ct-machine-appearance-receipt" role="status">
      <span>{copy.status}</span>
      {copy.failures.map((item, index) => <span key={index}>{item.name} · {item.reason}</span>)}
    </div>}
  </div>;
}

export function MachineCopyNotice({ copy }: { copy: ReturnType<typeof useMachineAppearanceCopy> }) {
  if (!copy.receipt) return null;
  return <div className="ct-machine-switch-notice ct-machine-copy-notice" aria-live="polite"
    data-busy={copy.busy || undefined} onAnimationEnd={copy.expire}>
    <span>{copy.status}</span>
    <button type="button" disabled={copy.busy} onClick={() => void copy.undo()}>Undo</button>
  </div>;
}

export function NowOnMachine({ label, onDismiss }: { label: string; onDismiss: () => void }) {
  useEffect(() => {
    const escape = (event: KeyboardEvent) => {
      if (event.key === "Escape") onDismiss();
    };
    window.addEventListener("keydown", escape);
    return () => window.removeEventListener("keydown", escape);
  }, [onDismiss]);
  return <span role="status" aria-live="polite" className="ct-machine-switch-notice" onAnimationEnd={onDismiss}>
    Now on {label}
  </span>;
}
