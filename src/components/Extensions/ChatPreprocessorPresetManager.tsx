/**
 * Master-detail preset editor for chatPreprocessor plugins. Presets configure
 * per-turn transformation behavior (name + description + arbitrary config).
 * Mutations persist via {@link chatPreprocessorPresetApi}; no model binding or
 * session apply — preprocessors are stateless per-turn transforms.
 * Generalized — any chatPreprocessor plugin, no hardcoded plugin id.
 */
import { useCallback, useEffect, useMemo, useState } from "react";
import { Copy, Plus, Trash2 } from "lucide-react";
import { GroupLabel } from "./shared";
import {
  getPreprocessorPresetApi,
  type PreprocessorPreset,
  type PreprocessorPresetState,
} from "./chatPreprocessorPresetApi";

const SYSTEM_PRESETS: PreprocessorPreset[] = [
  {
    id: "mem-recall",
    name: "Memory recall",
    kind: "mem-recall",
    description: "Search Domios memory and attach compact similar facts.",
    system: true,
    config: { k: 3, position: "prepend" },
  },
];

function newId(): string {
  const c = (globalThis as { crypto?: { randomUUID?: () => string } }).crypto;
  if (c?.randomUUID) return `ppre-${c.randomUUID()}`;
  return `ppre-${Math.floor(performance.now())}-${Math.floor(performance.now() * 1000) % 1000}`;
}

function serializeConfig(config: Record<string, unknown> | undefined): string {
  if (!config || Object.keys(config).length === 0) return "";
  try {
    return JSON.stringify(config, null, 2);
  } catch {
    return "";
  }
}

function parseConfig(raw: string): Record<string, unknown> | undefined {
  const trimmed = raw.trim();
  if (!trimmed) return undefined;
  try {
    const parsed = JSON.parse(trimmed);
    if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
      return parsed as Record<string, unknown>;
    }
  } catch {
  }
  return undefined;
}

export function ChatPreprocessorPresetManager({ pluginId }: { pluginId: string }) {
  const [state, setState] = useState<PreprocessorPresetState>({
    presets: [],
    activePreset: null,
  });
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [configText, setConfigText] = useState("");
  const [configError, setConfigError] = useState(false);

  useEffect(() => {
    let alive = true;
    getPreprocessorPresetApi()
      .load(pluginId)
      .then((loaded) => {
        if (!alive) return;
        const presets = mergeSystemPresets(loaded.presets);
        setState({ ...loaded, presets });
        setSelectedId(loaded.activePreset ?? presets[0]?.id ?? null);
      })
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, [pluginId]);

  const commit = useCallback(
    (next: PreprocessorPresetState) => {
      const withSystem = { ...next, presets: mergeSystemPresets(next.presets) };
      const stored = {
        ...withSystem,
        presets: withSystem.presets.filter((p) => !p.system),
      };
      setState(withSystem);
      void getPreprocessorPresetApi()
        .save(pluginId, stored)
        .catch(() => {});
    },
    [pluginId],
  );

  const selected = useMemo(
    () => state.presets.find((p) => p.id === selectedId) ?? null,
    [state.presets, selectedId],
  );

  // Sync config editor when selection changes.
  useEffect(() => {
    setConfigText(serializeConfig(selected?.config));
    setConfigError(false);
  }, [selectedId, selected?.config]);

  const patchSelected = useCallback(
    (patch: Partial<PreprocessorPreset>) => {
      if (!selected) return;
      commit({
        ...state,
        presets: state.presets.map((p) =>
          p.id === selected.id ? { ...p, ...patch } : p,
        ),
      });
    },
    [commit, selected, state],
  );

  const handleConfigChange = useCallback(
    (raw: string) => {
      setConfigText(raw);
      if (!selected) return;
      const trimmed = raw.trim();
      if (!trimmed) {
        setConfigError(false);
        patchSelected({ config: undefined });
        return;
      }
      const parsed = parseConfig(trimmed);
      if (parsed !== undefined) {
        setConfigError(false);
        patchSelected({ config: parsed });
      } else {
        setConfigError(true);
      }
    },
    [patchSelected, selected],
  );

  const createPreset = useCallback(() => {
    const preset: PreprocessorPreset = {
      id: newId(),
      name: "New preset",
      kind: "template",
      template: "{{text}}",
    };
    commit({ ...state, presets: [...state.presets, preset] });
    setSelectedId(preset.id);
  }, [commit, state]);

  const duplicateSelected = useCallback(() => {
    if (!selected) return;
    const copy: PreprocessorPreset = {
      ...structuredClone(selected),
      system: false,
      id: newId(),
      name: `${selected.name} (copy)`,
    };
    commit({ ...state, presets: [...state.presets, copy] });
    setSelectedId(copy.id);
  }, [commit, selected, state]);

  const deleteSelected = useCallback(() => {
    if (!selected) return;
    if (selected.system) return;
    const presets = state.presets.filter((p) => p.id !== selected.id);
    const activePreset =
      state.activePreset === selected.id ? null : state.activePreset;
    commit({ presets, activePreset });
    setSelectedId(presets[0]?.id ?? null);
  }, [commit, selected, state]);

  const setActive = useCallback(() => {
    if (!selected) return;
    commit({ ...state, activePreset: selected.id });
  }, [commit, selected, state]);

  return (
    <div style={{ marginBottom: 20 }} data-testid="preprocessor-presets">
      <div
        style={{
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          gap: 8,
        }}
      >
        <GroupLabel>Presets</GroupLabel>
        <button
          type="button"
          data-testid="ppre-new"
          onClick={createPreset}
          style={ghostBtn}
          title="Create a new preset"
        >
          <Plus size={12} />
          New preset
        </button>
      </div>

      <div
        style={{
          display: "grid",
          gridTemplateColumns: "minmax(140px, 200px) 1fr",
          gap: 12,
          marginTop: 6,
        }}
      >
        {/* Preset list */}
        <div style={{ display: "flex", flexDirection: "column", gap: 3, minWidth: 0 }}>
          {state.presets.filter((p) => !p.system).length === 0 && (
            <div
              data-testid="ppre-empty"
              style={{
                fontSize: 12,
                color: "var(--ct-muted)",
                padding: "4px 2px 6px",
                lineHeight: 1.5,
              }}
            >
              No user presets yet. Create one to configure how this plugin
              transforms your messages.
            </div>
          )}
          {state.presets.length > 0 && (
            state.presets.map((p) => {
              const isActive = p.id === state.activePreset;
              const isSelected = p.id === selectedId;
              return (
                <button
                  key={p.id}
                  type="button"
                  data-testid="ppre-row"
                  data-preset-id={p.id}
                  data-active={isActive ? "true" : undefined}
                  onClick={() => setSelectedId(p.id)}
                  style={{
                    display: "flex",
                    alignItems: "center",
                    justifyContent: "space-between",
                    gap: 6,
                    textAlign: "left",
                    borderRadius: 6,
                    padding: "6px 8px",
                    fontSize: 12.5,
                    cursor: "pointer",
                    border: "1px solid",
                    borderColor: isSelected
                      ? "var(--ct-accent, #6366f1)"
                      : "var(--ct-border-subtle)",
                    background: isSelected
                      ? "color-mix(in srgb, var(--ct-accent, #6366f1) 12%, transparent)"
                      : "var(--ct-bg-elev)",
                    color: "var(--ct-fg)",
                  }}
                >
                  <span
                    style={{
                      overflow: "hidden",
                      textOverflow: "ellipsis",
                      whiteSpace: "nowrap",
                    }}
                  >
                    {p.name || "Untitled"}
                  </span>
                  {isActive && (
                    <span
                      data-testid="ppre-row-active"
                      data-preset-id={p.id}
                      style={{
                        flex: "none",
                        fontSize: 9.5,
                        fontWeight: 600,
                        letterSpacing: 0.3,
                        textTransform: "uppercase",
                        color: "var(--ct-accent, #818cf8)",
                      }}
                    >
                      active
                    </span>
                  )}
                </button>
              );
            })
          )}
        </div>

        {/* Detail panel */}
        <div style={{ minWidth: 0 }}>
          {selected ? (
            <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
              {selected.system && (
                <div
                  data-testid="ppre-system-note"
                  style={{
                    border: "1px solid var(--ct-border-subtle)",
                    borderRadius: 6,
                    padding: "7px 9px",
                    fontSize: 11.5,
                    color: "var(--ct-muted)",
                    background: "var(--ct-bg-elev)",
                  }}
                >
                  Built-in preset. Duplicate it to make an editable copy.
                </div>
              )}

              <Field label="Name">
                <input
                  data-testid="ppre-name"
                  value={selected.name}
                  placeholder="Preset name"
                  disabled={selected.system}
                  onInput={(e) => patchSelected({ name: e.currentTarget.value })}
                  style={inputStyle}
                />
              </Field>

              <Field label="Kind">
                <select
                  data-testid="ppre-kind"
                  value={selected.kind ?? "template"}
                  disabled={selected.system}
                  onChange={(e) => patchSelected({ kind: e.currentTarget.value })}
                  style={inputStyle}
                >
                  <option value="template">Template</option>
                  <option value="mem-recall">Memory recall</option>
                </select>
              </Field>

              <Field label="Description">
                <input
                  data-testid="ppre-description"
                  value={selected.description ?? ""}
                  placeholder="Short note shown in the preset picker…"
                  disabled={selected.system}
                  onInput={(e) =>
                    patchSelected({
                      description: e.currentTarget.value || undefined,
                    })
                  }
                  style={inputStyle}
                />
              </Field>

              <Field label="Template">
                <textarea
                  data-testid="ppre-template"
                  value={selected.template ?? "{{text}}"}
                  placeholder="{{text}}"
                  rows={4}
                  disabled={selected.system}
                  onInput={(e) =>
                    patchSelected({
                      template: e.currentTarget.value || undefined,
                    })
                  }
                  style={{
                    ...inputStyle,
                    resize: "vertical",
                    fontFamily: '"JetBrains Mono", ui-monospace, monospace',
                    fontSize: 11.5,
                    lineHeight: 1.5,
                  }}
                />
              </Field>

              <Field label={configError ? "Config (invalid JSON)" : "Config (JSON)"}>
                <textarea
                  data-testid="ppre-params"
                  value={configText}
                  placeholder='{"key": "value"}'
                  rows={4}
                  disabled={selected.system}
                  onInput={(e) => handleConfigChange(e.currentTarget.value)}
                  style={{
                    ...inputStyle,
                    resize: "vertical",
                    fontFamily:
                      '"JetBrains Mono", ui-monospace, monospace',
                    fontSize: 11.5,
                    lineHeight: 1.5,
                    borderColor: configError
                      ? "rgba(229,115,115,0.6)"
                      : "var(--ct-border-subtle)",
                  }}
                />
              </Field>

              <div style={{ display: "flex", gap: 8, marginTop: 2 }}>
                <button
                  type="button"
                  data-testid="ppre-set-active"
                  onClick={setActive}
                  style={{ ...primaryBtn, cursor: "pointer" }}
                  title="Make this the active preset"
                >
                  {selected.id === state.activePreset ? "Active" : "Set active"}
                </button>
                <button
                  type="button"
                  data-testid="ppre-duplicate"
                  onClick={duplicateSelected}
                  style={ghostBtn}
                  title="Duplicate this preset"
                >
                  <Copy size={12} />
                  Duplicate
                </button>
                <button
                  type="button"
                  data-testid="ppre-delete"
                  onClick={deleteSelected}
                  disabled={selected.system}
                  style={{
                    ...dangerBtn,
                    opacity: selected.system ? 0.45 : 1,
                    cursor: selected.system ? "not-allowed" : "pointer",
                  }}
                >
                  <Trash2 size={12} />
                  Delete
                </button>
              </div>
            </div>
          ) : (
            <div
              style={{ fontSize: 12, color: "var(--ct-muted)", padding: "8px 2px" }}
            >
              Select a preset to edit, or create a new one.
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

function mergeSystemPresets(userPresets: PreprocessorPreset[]): PreprocessorPreset[] {
  const user = userPresets.filter((p) => !p.system && !SYSTEM_PRESETS.some((s) => s.id === p.id));
  return [...SYSTEM_PRESETS, ...user];
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <label style={{ display: "flex", flexDirection: "column", gap: 4 }}>
      <span
        style={{ fontSize: 11, color: "var(--ct-muted)", fontWeight: 540 }}
      >
        {label}
      </span>
      {children}
    </label>
  );
}

const inputStyle: React.CSSProperties = {
  width: "100%",
  boxSizing: "border-box",
  fontSize: 12.5,
  borderRadius: 6,
  padding: "6px 8px",
  border: "1px solid var(--ct-border-subtle)",
  background: "var(--ct-bg)",
  color: "var(--ct-fg)",
};

const ghostBtn: React.CSSProperties = {
  display: "inline-flex",
  alignItems: "center",
  gap: 4,
  fontSize: 11.5,
  fontWeight: 540,
  borderRadius: 6,
  padding: "4px 9px",
  cursor: "pointer",
  border: "1px solid var(--ct-border-subtle)",
  background: "var(--ct-bg-elev)",
  color: "var(--ct-fg)",
};

const primaryBtn: React.CSSProperties = {
  fontSize: 12,
  fontWeight: 560,
  borderRadius: 6,
  padding: "5px 14px",
  border: "1px solid transparent",
  background: "var(--ct-accent, #6366f1)",
  color: "#fff",
};

const dangerBtn: React.CSSProperties = {
  display: "inline-flex",
  alignItems: "center",
  gap: 4,
  fontSize: 12,
  borderRadius: 6,
  padding: "5px 12px",
  cursor: "pointer",
  border: "1px solid rgba(229,115,115,0.4)",
  background: "transparent",
  color: "#e57373",
};
