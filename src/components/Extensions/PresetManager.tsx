/**
 * Preset manager for chatBackend plugins (R5b).
 *
 * Master-detail editor over a backend's user-defined presets
 * ({id, name, systemPrompt, model?, params?}). The list is the source of truth
 * the plugin's `listPresets()` reads back, so every mutation persists to
 * `config.plugins.<id>` through the {@link presetManagerApi} seam. Binding a
 * model (R4) is just `preset.model`; the active selection persists as
 * `activePreset` and — when a live `sessionId` is supplied — switches that
 * session onto the preset's bound model. Generalized: any chatBackend with
 * presets renders this; no hardcoded provider id.
 */
import { useCallback, useEffect, useMemo, useState } from "react";
import { Plus, Trash2 } from "lucide-react";
import type { ModelInfo } from "../../lib/types";
import { GroupLabel } from "./shared";
import { getPresetManagerApi, type Preset, type PresetState } from "./presetManagerApi";

function newId(): string {
  const c = (globalThis as { crypto?: { randomUUID?: () => string } }).crypto;
  if (c?.randomUUID) return `preset-${c.randomUUID()}`;
  return `preset-${Math.floor(performance.now())}-${Math.floor(performance.now() * 1000) % 1000}`;
}

function readTemperature(preset: Preset | undefined): string {
  const t = preset?.params?.temperature;
  return typeof t === "number" ? String(t) : "";
}

export function PresetManager({
  pluginId,
  models,
  sessionId,
}: {
  pluginId: string;
  models: ModelInfo[];
  sessionId?: string;
}) {
  const [state, setState] = useState<PresetState>({ presets: [], activePreset: null });
  const [selectedId, setSelectedId] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    getPresetManagerApi()
      .load(pluginId)
      .then((loaded) => {
        if (!alive) return;
        setState(loaded);
        setSelectedId(loaded.activePreset ?? loaded.presets[0]?.id ?? null);
      })
      .catch(() => {});
    return () => {
      alive = false;
    };
  }, [pluginId]);

  // Single commit path: optimistic local update + persist. Keeps the plugin's
  // listPresets() in lockstep with what the editor shows.
  const commit = useCallback(
    (next: PresetState) => {
      setState(next);
      void getPresetManagerApi()
        .save(pluginId, next)
        .catch(() => {});
    },
    [pluginId],
  );

  const selected = useMemo(
    () => state.presets.find((p) => p.id === selectedId) ?? null,
    [state.presets, selectedId],
  );

  const patchSelected = useCallback(
    (patch: Partial<Preset>) => {
      if (!selected) return;
      commit({
        ...state,
        presets: state.presets.map((p) => (p.id === selected.id ? { ...p, ...patch } : p)),
      });
    },
    [commit, selected, state],
  );

  const createPreset = useCallback(() => {
    const preset: Preset = { id: newId(), name: "New preset" };
    commit({ ...state, presets: [...state.presets, preset] });
    setSelectedId(preset.id);
  }, [commit, state]);

  const deleteSelected = useCallback(() => {
    if (!selected) return;
    const presets = state.presets.filter((p) => p.id !== selected.id);
    const activePreset = state.activePreset === selected.id ? null : state.activePreset;
    commit({ presets, activePreset });
    setSelectedId(presets[0]?.id ?? null);
  }, [commit, selected, state]);

  const setActive = useCallback(() => {
    if (!selected) return;
    commit({ ...state, activePreset: selected.id });
    if (sessionId && selected.model) {
      void getPresetManagerApi()
        .applyToSession(pluginId, sessionId, selected.model)
        .catch(() => {});
    }
  }, [commit, pluginId, selected, sessionId, state]);

  const setTemperature = useCallback(
    (raw: string) => {
      if (!selected) return;
      const trimmed = raw.trim();
      const params = { ...(selected.params ?? {}) };
      if (trimmed === "" || Number.isNaN(Number(trimmed))) delete params.temperature;
      else params.temperature = Number(trimmed);
      patchSelected({ params });
    },
    [patchSelected, selected],
  );

  return (
    <div style={{ marginBottom: 20 }} data-testid="extensions-presets">
      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", gap: 8 }}>
        <GroupLabel>Presets</GroupLabel>
        <button
          type="button"
          data-testid="preset-new"
          onClick={createPreset}
          style={ghostBtn}
          title="Create a new preset"
        >
          <Plus size={12} />
          New preset
        </button>
      </div>

      <div style={{ display: "grid", gridTemplateColumns: "minmax(140px, 200px) 1fr", gap: 12, marginTop: 6 }}>
        <div style={{ display: "flex", flexDirection: "column", gap: 3, minWidth: 0 }}>
          {state.presets.length === 0 ? (
            <div
              data-testid="preset-empty"
              style={{ fontSize: 12, color: "var(--ct-muted)", padding: "8px 2px", lineHeight: 1.5 }}
            >
              No presets yet. Create one to pair a system prompt with a model.
            </div>
          ) : (
            state.presets.map((p) => {
              const isActive = p.id === state.activePreset;
              const isSelected = p.id === selectedId;
              return (
                <button
                  key={p.id}
                  type="button"
                  data-testid="preset-row"
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
                    borderColor: isSelected ? "var(--ct-accent, #6366f1)" : "var(--ct-border-subtle)",
                    background: isSelected
                      ? "color-mix(in srgb, var(--ct-accent, #6366f1) 12%, transparent)"
                      : "var(--ct-bg-elev)",
                    color: "var(--ct-fg)",
                  }}
                >
                  <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                    {p.name || "Untitled"}
                  </span>
                  {isActive && (
                    <span
                      data-testid="preset-row-active"
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

        <div style={{ minWidth: 0 }}>
          {selected ? (
            <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
              <Field label="Name">
                <input
                  data-testid="preset-name"
                  value={selected.name}
                  placeholder="Preset name"
                  onInput={(e) => patchSelected({ name: e.currentTarget.value })}
                  style={inputStyle}
                />
              </Field>

              <Field label="System prompt">
                <textarea
                  data-testid="preset-system-prompt"
                  value={selected.systemPrompt ?? ""}
                  placeholder="Instructions sent to the model as its system prompt…"
                  rows={4}
                  onInput={(e) => patchSelected({ systemPrompt: e.currentTarget.value })}
                  style={{ ...inputStyle, resize: "vertical", fontFamily: "inherit", lineHeight: 1.5 }}
                />
              </Field>

              <div style={{ display: "grid", gridTemplateColumns: "1fr 120px", gap: 10 }}>
                <Field label="Model">
                  <select
                    data-testid="preset-model"
                    value={selected.model ?? ""}
                    onChange={(e) => patchSelected({ model: e.currentTarget.value || undefined })}
                    style={inputStyle}
                  >
                    <option value="">— Backend default —</option>
                    {models.map((m) => (
                      <option key={m.id} value={m.id}>
                        {m.display_name}
                      </option>
                    ))}
                  </select>
                </Field>
                <Field label="Temperature">
                  <input
                    data-testid="preset-temperature"
                    type="number"
                    step="0.1"
                    min="0"
                    max="2"
                    value={readTemperature(selected)}
                    placeholder="—"
                    onInput={(e) => setTemperature(e.currentTarget.value)}
                    style={inputStyle}
                  />
                </Field>
              </div>

              <div style={{ display: "flex", gap: 8, marginTop: 2 }}>
                <button
                  type="button"
                  data-testid="preset-set-active"
                  onClick={setActive}
                  style={{ ...primaryBtn, cursor: "pointer" }}
                  title={sessionId ? "Use this preset for the current session" : "Make this the active preset"}
                >
                  {selected.id === state.activePreset
                    ? sessionId
                      ? "Apply to session"
                      : "Active"
                    : "Set active"}
                </button>
                <button type="button" data-testid="preset-delete" onClick={deleteSelected} style={dangerBtn}>
                  <Trash2 size={12} />
                  Delete
                </button>
              </div>
            </div>
          ) : (
            <div style={{ fontSize: 12, color: "var(--ct-muted)", padding: "8px 2px" }}>
              Select a preset to edit, or create a new one.
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <label style={{ display: "flex", flexDirection: "column", gap: 4 }}>
      <span style={{ fontSize: 11, color: "var(--ct-muted)", fontWeight: 540 }}>{label}</span>
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
