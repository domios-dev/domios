import { useState } from "react";
import type { AppConfig } from "../../lib/types";
import { Group, renderKbd } from "./shared";

export function KeybindGroup({
  title,
  bindings,
  config,
  onUpdate,
}: {
  title: string;
  bindings: [string, string][];
  config: AppConfig;
  onUpdate: (config: AppConfig) => void;
}) {
  const [editingKey, setEditingKey] = useState<string | null>(null);

  return (
    <Group title={title}>
      <div className="space-y-1">
        {bindings.map(([key, label]) => {
          const value = (config.keybindings as unknown as Record<string, string>)[key] || "";
          const isEditing = editingKey === key;
          return (
            <div key={key} className="flex items-center justify-between py-0.5">
              <span className="text-xs text-[var(--ct-muted-fg)]">{label}</span>
              {isEditing ? (
                <input
                  type="text"
                  autoFocus
                  value={value}
                  onChange={(e) => {
                    const next = { ...config, keybindings: { ...config.keybindings, [key]: e.target.value } };
                    onUpdate(next);
                  }}
                  onBlur={() => setEditingKey(null)}
                  onKeyDown={(e) => { if (e.key === "Enter" || e.key === "Escape") setEditingKey(null); }}
                  className="w-36 bg-[var(--ct-bg)] border border-[var(--ct-accent)] rounded px-2 py-0.5 text-[11px] text-[var(--ct-fg)] focus:outline-none text-right font-mono"
                />
              ) : (
                <button
                  onClick={() => setEditingKey(key)}
                  className="flex items-center gap-1 px-1.5 py-0.5 rounded hover:bg-[var(--ct-hover)] transition-colors"
                  title="Click to edit"
                >
                  {renderKbd(value)}
                </button>
              )}
            </div>
          );
        })}
      </div>
    </Group>
  );
}
