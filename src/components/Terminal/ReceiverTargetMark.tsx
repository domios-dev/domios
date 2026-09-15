import { useState } from "react";
import { PluginIcon } from "../Extensions/PluginIcon";
import {
  pluginTabMenuEntries,
  runPluginTabMenuItem,
  type PluginTabMenuEntry,
  type PluginTabMenuState,
} from "../../lib/pluginTabMenu";

export type ReceiverTargetMarkSurface = "composer" | "header";

export function receiverTargetEntries(menu: PluginTabMenuState, tabId: string): PluginTabMenuEntry[] {
  const seen = new Set<string>();
  return pluginTabMenuEntries(menu, tabId).filter((entry) => {
    if (entry.action !== "receiverTarget" || !entry.checked || seen.has(entry.pluginId)) return false;
    seen.add(entry.pluginId);
    return true;
  });
}

function ReceiverTargetMarkButton({
  entry,
  tabId,
  surface,
}: {
  entry: PluginTabMenuEntry;
  tabId: string;
  surface: ReceiverTargetMarkSurface;
}) {
  const [detaching, setDetaching] = useState(false);
  const receiverName = entry.pluginId || "Unknown receiver";
  const detachLabel = `Detach ${receiverName} receiver from this tab`;

  return (
    <button
      type="button"
      data-testid={`receiver-target-mark-button-${surface}-${entry.pluginId}`}
      data-receiver-plugin-id={entry.pluginId}
      aria-label={detachLabel}
      aria-busy={detaching}
      disabled={detaching}
      title={detachLabel}
      onMouseDown={(event) => event.preventDefault()}
      onClick={() => {
        if (detaching) return;
        setDetaching(true);
        void runPluginTabMenuItem(entry, tabId)
          .catch((error) => console.warn("Receiver target detach failed:", error))
          .finally(() => setDetaching(false));
      }}
      className="ct-liquid-interactive inline-flex min-w-0 items-center gap-1.5 rounded-full border px-2 py-1 text-[11px] font-medium"
      style={{
        color: entry.iconColor ?? "var(--ct-accent, #58a6ff)",
        borderColor: "color-mix(in srgb, currentColor 34%, transparent)",
        background: "color-mix(in srgb, currentColor 10%, transparent)",
        opacity: detaching ? 0.65 : 1,
      }}
    >
      <PluginIcon plugin={{ icon_svg: entry.iconSvg, icon_color: entry.iconColor }} size={surface === "header" ? 14 : 16} />
      <span className="max-w-48 truncate">{receiverName}</span>
    </button>
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
  if (entries.length === 0) return null;

  return (
    <div
      data-testid={`receiver-target-marks-${surface}`}
      role="group"
      aria-label="Active receivers"
      className={surface === "composer" ? "mb-2 flex flex-wrap items-center gap-1.5" : "flex min-w-0 flex-wrap items-center gap-1"}
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
