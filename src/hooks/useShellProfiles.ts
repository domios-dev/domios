import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "../lib/bridge";
import type { ShellProfile } from "../lib/types";

export function useShellProfiles() {
  const [profiles, setProfiles] = useState<ShellProfile[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const generation = useRef(0);
  const load = useCallback(async (refresh = false) => {
    const current = ++generation.current;
    setLoading(true);
    try {
      const result = await invoke<ShellProfile[]>("shell_profiles", { refresh });
      if (generation.current === current) {
        setProfiles(result);
        setError(null);
      }
    } catch (reason) {
      if (generation.current === current) setError(String(reason));
    } finally {
      if (generation.current === current) setLoading(false);
    }
  }, []);
  useEffect(() => {
    void load();
    return () => { generation.current++; };
  }, [load]);
  return { profiles, error, loading, refresh: () => load(true) };
}
