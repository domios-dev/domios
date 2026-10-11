import { useEffect, useState } from "react";
import { baseDaemonTransport } from "../../lib/daemonBootstrap";
import { loadPluginDocument } from "../../lib/pluginDocument";

export function usePluginDocument(path: string): { html: string | undefined; error: string | null } {
  const [state, setState] = useState<{ path: string; html?: string; error: string | null }>({ path, error: null });
  useEffect(() => {
    const controller = new AbortController();
    let dispose: (() => void) | undefined;
    void loadPluginDocument(baseDaemonTransport(), path, controller.signal).then((document) => {
      if (controller.signal.aborted) { document.dispose(); return; }
      dispose = document.dispose;
      setState({ path, html: document.html, error: null });
    }).catch((error: unknown) => {
      if (!controller.signal.aborted) setState({ path, error: String(error) });
    });
    return () => { controller.abort(); dispose?.(); };
  }, [path]);
  return state.path === path ? { html: state.html, error: state.error } : { html: undefined, error: null };
}
