import { useCallback, useState, type CSSProperties } from "react";
import {
  CANVAS_LAYOUT_SETTING_KEYS,
  DEFAULT_CANVAS_SETTINGS,
  type CanvasLayoutSettings,
  type LayoutStrategy,
} from "./settings";
import { useCanvasSettings } from "./useCanvasSettings";
import { DEV_BUILD } from "../../lib/devBuild";

const PANEL_OPEN_KEY = "codeterm:canvas-debug-panel-open";

const LAYOUT_STRATEGIES: LayoutStrategy[] = ["lanes", "cluster", "radial"];

type NumericSettingKey = {
  [K in keyof CanvasLayoutSettings]: CanvasLayoutSettings[K] extends number ? K : never;
}[keyof CanvasLayoutSettings];

function numericMeta(key: NumericSettingKey): { min: number; max: number; step: number } {
  if (key.includes("Scale") || key.includes("Threshold") || key === "edgeCurve") {
    return { min: 0, max: 3, step: 0.05 };
  }
  if (key.includes("zoom")) {
    return { min: 0.1, max: 3, step: 0.05 };
  }
  return { min: 0, max: 8000, step: 1 };
}

function isDevBuild(): boolean {
  try {
    return DEV_BUILD;
  } catch {
    return false;
  }
}

function panelOpenFromStorage(): boolean {
  if (typeof window === "undefined") return false;
  return window.localStorage.getItem(PANEL_OPEN_KEY) === "1";
}

const panelShellStyle: CSSProperties = {
  position: "absolute",
  top: 8,
  right: 8,
  zIndex: 20,
  width: 280,
  maxHeight: "min(80vh, 640px)",
  overflow: "auto",
  border: "1px solid var(--ct-border-default)",
  borderRadius: "var(--ct-radius-md)",
  background: "var(--ct-bg)",
  color: "var(--ct-fg)",
  fontFamily: "var(--ct-font-ui)",
  fontSize: "var(--ct-font-size-sm)",
  boxShadow: "var(--ct-shadow-md)",
  padding: "10px 12px",
};

const rowStyle: CSSProperties = {
  display: "flex",
  flexDirection: "column",
  gap: 4,
  marginBottom: 10,
};

const labelStyle: CSSProperties = {
  display: "flex",
  justifyContent: "space-between",
  alignItems: "center",
  gap: 8,
};

export interface CanvasDebugPanelProps {
  /** Bypass dev gate (tests). */
  forceVisible?: boolean;
}

export function CanvasDebugPanel({ forceVisible = false }: CanvasDebugPanelProps) {
  const { settings, setField, resetOverride, saveSettings } = useCanvasSettings();
  const [open, setOpen] = useState(panelOpenFromStorage);
  const devGated = forceVisible || isDevBuild();

  const toggleOpen = useCallback(() => {
    setOpen((prev) => {
      const next = !prev;
      if (typeof window !== "undefined") {
        window.localStorage.setItem(PANEL_OPEN_KEY, next ? "1" : "0");
      }
      return next;
    });
  }, []);

  const handleSave = useCallback(async () => {
    await saveSettings();
  }, [saveSettings]);

  if (!devGated) return null;

  return (
    <div data-testid="canvas-debug-panel-root" style={{ position: "absolute", top: 8, right: 8, zIndex: 20 }}>
      <button
        type="button"
        data-testid="canvas-debug-panel-toggle"
        onClick={toggleOpen}
        style={{
          fontSize: "var(--ct-font-size-sm)",
          padding: "4px 10px",
          borderRadius: "var(--ct-radius-sm)",
          border: "1px solid var(--ct-border-default)",
          background: "var(--ct-bg-selected)",
          color: "var(--ct-fg)",
          cursor: "pointer",
        }}
      >
        Canvas debug
      </button>
      {open && (
        <div data-testid="canvas-debug-panel" style={panelShellStyle}>
          <div style={{ fontWeight: 600, marginBottom: 10 }}>Layout settings</div>
          {CANVAS_LAYOUT_SETTING_KEYS.map((key) => {
            if (key === "layoutStrategy") {
              return (
                <div key={key} data-testid={`canvas-debug-field-${key}`} style={rowStyle}>
                  <label style={labelStyle} htmlFor={`canvas-debug-${key}`}>
                    <span>{key}</span>
                  </label>
                  <select
                    id={`canvas-debug-${key}`}
                    data-testid={`canvas-debug-input-${key}`}
                    value={settings.layoutStrategy}
                    onChange={(e) => setField("layoutStrategy", e.target.value as LayoutStrategy)}
                    style={{ width: "100%" }}
                  >
                    {LAYOUT_STRATEGIES.map((strategy) => (
                      <option key={strategy} value={strategy}>
                        {strategy}
                      </option>
                    ))}
                  </select>
                </div>
              );
            }

            const meta = numericMeta(key as NumericSettingKey);
            const value = settings[key] as number;
            const defaultValue = DEFAULT_CANVAS_SETTINGS[key] as number;

            return (
              <div key={key} data-testid={`canvas-debug-field-${key}`} style={rowStyle}>
                <label style={labelStyle} htmlFor={`canvas-debug-${key}`}>
                  <span>{key}</span>
                  <span data-testid={`canvas-debug-value-${key}`}>{value}</span>
                </label>
                <input
                  id={`canvas-debug-${key}`}
                  data-testid={`canvas-debug-slider-${key}`}
                  type="range"
                  min={meta.min}
                  max={meta.max}
                  step={meta.step}
                  value={value}
                  onChange={(e) =>
                    setField(key, Number(e.target.value) as CanvasLayoutSettings[typeof key])
                  }
                />
                <input
                  data-testid={`canvas-debug-input-${key}`}
                  type="number"
                  min={meta.min}
                  max={meta.max}
                  step={meta.step}
                  value={value}
                  onChange={(e) =>
                    setField(key, Number(e.target.value) as CanvasLayoutSettings[typeof key])
                  }
                />
                {value !== defaultValue && (
                  <span style={{ opacity: "var(--ct-text-secondary-opacity)" }}>
                    default: {defaultValue}
                  </span>
                )}
              </div>
            );
          })}
          <div style={{ display: "flex", gap: 8, marginTop: 4 }}>
            <button
              type="button"
              data-testid="canvas-debug-save"
              onClick={() => void handleSave()}
              style={{
                flex: 1,
                padding: "6px 10px",
                borderRadius: "var(--ct-radius-sm)",
                border: "1px solid var(--ct-border-default)",
                background: "var(--ct-accent, rgba(99, 102, 241, 0.9))",
                color: "var(--ct-bg)",
                cursor: "pointer",
              }}
            >
              Save
            </button>
            <button
              type="button"
              data-testid="canvas-debug-reset"
              onClick={resetOverride}
              style={{
                flex: 1,
                padding: "6px 10px",
                borderRadius: "var(--ct-radius-sm)",
                border: "1px solid var(--ct-border-default)",
                background: "var(--ct-bg-selected)",
                color: "var(--ct-fg)",
                cursor: "pointer",
              }}
            >
              Reset
            </button>
          </div>
        </div>
      )}
    </div>
  );
}
