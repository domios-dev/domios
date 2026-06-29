import {
  createContext,
  useCallback,
  useContext,
  useMemo,
  useState,
  type ReactNode,
} from "react";
import {
  DEFAULT_CANVAS_SETTINGS,
  type CanvasLayoutSettings,
} from "./settings";

const STORAGE_KEY = "codeterm:canvas-settings-override";

export type CanvasSettingsOverride = Partial<CanvasLayoutSettings>;

export interface CanvasSettingsContextValue {
  settings: CanvasLayoutSettings;
  override: CanvasSettingsOverride;
  setOverride: (patch: CanvasSettingsOverride) => void;
  setField: <K extends keyof CanvasLayoutSettings>(
    key: K,
    value: CanvasLayoutSettings[K],
  ) => void;
  resetOverride: () => void;
  saveSettings: () => Promise<void>;
}

const CanvasSettingsContext = createContext<CanvasSettingsContextValue | null>(null);

function loadStoredOverride(): CanvasSettingsOverride {
  if (typeof window === "undefined") return {};
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw) as CanvasSettingsOverride;
    return parsed && typeof parsed === "object" ? parsed : {};
  } catch {
    return {};
  }
}

function mergeSettings(override: CanvasSettingsOverride): CanvasLayoutSettings {
  return { ...DEFAULT_CANVAS_SETTINGS, ...override };
}

export function CanvasSettingsProvider({ children }: { children: ReactNode }) {
  const [override, setOverrideState] = useState<CanvasSettingsOverride>(loadStoredOverride);

  const settings = useMemo(() => mergeSettings(override), [override]);

  const setOverride = useCallback((patch: CanvasSettingsOverride) => {
    setOverrideState((prev) => ({ ...prev, ...patch }));
  }, []);

  const setField = useCallback(
    <K extends keyof CanvasLayoutSettings>(key: K, value: CanvasLayoutSettings[K]) => {
      setOverrideState((prev) => ({ ...prev, [key]: value }));
    },
    [],
  );

  const resetOverride = useCallback(() => {
    setOverrideState({});
    if (typeof window !== "undefined") {
      window.localStorage.removeItem(STORAGE_KEY);
    }
  }, []);

  const saveSettings = useCallback(async () => {
    if (typeof window === "undefined") return;
    window.localStorage.setItem(STORAGE_KEY, JSON.stringify(override));
  }, [override]);

  const value = useMemo(
    () => ({ settings, override, setOverride, setField, resetOverride, saveSettings }),
    [settings, override, setOverride, setField, resetOverride, saveSettings],
  );

  return (
    <CanvasSettingsContext.Provider value={value}>{children}</CanvasSettingsContext.Provider>
  );
}

export function useCanvasSettings(): CanvasSettingsContextValue {
  const ctx = useContext(CanvasSettingsContext);
  if (!ctx) {
    throw new Error("useCanvasSettings must be used within CanvasSettingsProvider");
  }
  return ctx;
}

export function useCanvasSettingsOptional(): CanvasSettingsContextValue | null {
  return useContext(CanvasSettingsContext);
}
