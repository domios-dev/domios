import { PageDataContext } from "./PageDataContext";
import { useCallback, useEffect, useRef, useState, useContext } from "react";
import { invoke, listen } from "../lib/bridge";
import { DEV_BUILD } from "../lib/devBuild";
import type { ShellProfile } from "../lib/types";

export function useShellProfiles() {
  const page = useContext(PageDataContext);
  const local = useShellProfilesLoader(!page);
  return page?.shells ?? local;
}

export function useShellProfilesLoader(enabled = true) {
  const [profiles, setProfiles] = useState<ShellProfile[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const generation = useRef(0);
  const load = useCallback(async (refresh = false, userInitiated = false) => {
    const current = ++generation.current;
    setLoading(true);
    try {
      const result = await invoke<ShellProfile[]>("shell_profiles", { refresh });
      if (generation.current === current) {
        setProfiles(result);
        setError(null);
      }
    } catch (reason) {
      if (generation.current !== current) return;
      if (userInitiated) setError(String(reason));
      else if (DEV_BUILD) console.debug("Background shell profile refresh failed", reason);
    } finally {
      if (generation.current === current) setLoading(false);
    }
  }, []);
  useEffect(() => {
    if (!enabled) return;
    void load();
    let active = true;
    let stop: (() => void) | undefined;
    void listen("shells-changed", () => { void load(); }).then((off) => {
      if (active) stop = off;
      else off();
    }).catch(() => undefined);
    const rebind = () => { void load(); };
    const reset = () => { generation.current++; setProfiles([]); setError(null); };
    window.addEventListener("codeterm-peer-switch-reset", reset);
    window.addEventListener("codeterm-peer-switch-rebind", rebind);
    return () => {
      active = false; generation.current++; stop?.();
      window.removeEventListener("codeterm-peer-switch-reset", reset);
      window.removeEventListener("codeterm-peer-switch-rebind", rebind);
    };
  }, [load, enabled]);
  return { profiles, error, loading, refresh: () => load(true, true) };
}
