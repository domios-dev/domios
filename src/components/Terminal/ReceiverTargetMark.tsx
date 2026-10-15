import { InlineErrorNotice } from "../shared/ErrorNotice";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { PluginIcon } from "../Extensions/PluginIcon";
import {
  pluginTabMenuEntries,
  runPluginTabMenuItem,
  type PluginTabMenuEntry,
  type PluginTabMenuState,
} from "../../lib/pluginTabMenu";

import { GlancePopover, type GlanceAnchor } from "../Extensions/GlancePopover";
import { subscribePluginBubble, type BubbleData } from "../../lib/pluginBubbleCache";

export type ReceiverTargetMarkSurface = "composer" | "header";

export function receiverTargetEntries(menu: PluginTabMenuState, tabId: string): PluginTabMenuEntry[] {
  const seen = new Set<string>();
  return pluginTabMenuEntries(menu, tabId).filter((entry) => {
    if (entry.action !== "receiverTarget" || !entry.checked || seen.has(entry.pluginId)) return false;
    seen.add(entry.pluginId);
    return true;
  });
}

function ReceiverTargetMarkButton({ entry, tabId, surface }: {
  entry: PluginTabMenuEntry;
  tabId: string;
  surface: ReceiverTargetMarkSurface;
}) {
  const [detaching, setDetaching] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [open, setOpen] = useState<GlanceAnchor | null>(null);
  const [pinned, setPinned] = useState(false);
  const [bubble, setBubble] = useState<BubbleData | null>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const ctx = useMemo(() => ({ tabId }), [tabId]);
  const close = useCallback(() => { setOpen(null); setPinned(false); }, []);
  const receiverName = entry.pluginId || "Unknown receiver";
  useEffect(() => {
    const subscription = subscribePluginBubble(entry.pluginId, null, tabId, true, setBubble);
    return () => subscription.unsubscribe();
  }, [entry.pluginId, tabId]);
  const show = () => {
    const rect = trigger.current?.getBoundingClientRect();
    if (rect) setOpen({ right: Math.max(8, window.innerWidth - rect.right), top: rect.bottom + 6 });
  };
  return (
    <>
      <button
        ref={trigger}
        type="button"
        data-testid={`receiver-target-mark-button-${surface}-${entry.pluginId}`}
        data-receiver-plugin-id={entry.pluginId}
        aria-label={`Show ${receiverName} receiver status`}
        aria-haspopup="dialog"
        aria-expanded={open !== null}
        onPointerEnter={show}
        onClick={() => { show(); setPinned(true); }}
        className="ct-liquid-interactive relative inline-flex h-7 w-7 items-center justify-center rounded-full border"
        style={{ color: entry.iconColor ?? "var(--ct-accent, #58a6ff)", borderColor: "color-mix(in srgb, currentColor 34%, transparent)", background: "color-mix(in srgb, currentColor 10%, transparent)" }}
      >
        <PluginIcon plugin={{ icon_svg: entry.iconSvg, icon_color: entry.iconColor }} size={16} />
        {(bubble?.tone === "warn" || bubble?.tone === "danger") && (
          <span role="status" aria-label={bubble.tooltip ?? "Receiver needs attention"} className="absolute right-0 top-0 h-1.5 w-1.5 rounded-full" style={{ background: "var(--ct-err, #ef4444)", boxShadow: "0 0 0 2px var(--ct-bg-elev)" }} />
        )}
      </button>
      {open && <GlancePopover
        pluginId={entry.pluginId}
        displayName={receiverName}
        anchor={open}
        ctx={ctx}
        trigger={trigger.current}
        hoverOnly={!pinned}
        onClose={close}
        onOpenSettings={(id) => window.dispatchEvent(new CustomEvent("codeterm:open-panel", { detail: { panel: `view:${id}` } }))}
        footer={<div className="mt-3 border-t pt-2" style={{ borderColor: "var(--ct-border-default)" }}>
          {error && <InlineErrorNotice error={error} operation="changing the recipient" blocking />}
          <button type="button" disabled={detaching} onClick={() => {
            setDetaching(true);
            void runPluginTabMenuItem(entry, tabId).then(close).catch((failure) => setError(String(failure))).finally(() => setDetaching(false));
          }} className="text-xs" style={{ color: "var(--ct-muted)" }}>Detach {receiverName} receiver from this tab</button>
        </div>}
      />}
    </>
  );
}

export function ReceiverTargetMarks({
  menu,
  tabId,
  surface,
}: {
  menu: PluginTabMenuState;
  tabId: string;
  surface: ReceiverTargetMarkSurface;
}) {
  const entries = receiverTargetEntries(menu, tabId);
  if (surface !== "header" || entries.length === 0) return null;

  return (
    <div
      data-testid={`receiver-target-marks-${surface}`}
      role="group"
      aria-label="Active receivers"
      className="flex min-w-0 flex-wrap items-center gap-1"
    >
      {entries.map((entry) => (
        <ReceiverTargetMarkButton
          key={`${entry.pluginId}-${entry.itemId}`}
          entry={entry}
          tabId={tabId}
          surface={surface}
        />
      ))}
    </div>
  );
}
