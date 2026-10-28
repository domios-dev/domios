import { useEffect, useState } from "react";
import { baseDaemonTransport } from "../../lib/daemonBootstrap";
import { loadPluginDocument, type PluginDocument } from "../../lib/pluginDocument";

export function usePluginDocument(path: string): { html: string | undefined; error: string | null; media?: PluginDocument["media"]; dispose?: () => void; frameToken?: string | null } {
  const [state, setState] = useState<{ path: string; html?: string; error: string | null; media?: PluginDocument["media"]; dispose?: () => void; frameToken?: string | null }>({ path, error: null });
  useEffect(() => {
    const controller = new AbortController();
    let dispose: (() => void) | undefined;
    void loadPluginDocument(baseDaemonTransport(), path, controller.signal).then((document) => {
      if (controller.signal.aborted) { document.dispose(); return; }
      dispose = document.dispose;
      setState({ path, html: document.html, media: document.media, frameToken: document.frameToken, dispose: document.dispose, error: null });
    }).catch((error: unknown) => {
      if (!controller.signal.aborted) setState({ path, error: String(error) });
    });
    return () => { controller.abort(); dispose?.(); };
  }, [path]);
  return state.path === path ? { html: state.html, error: state.error, media: state.media, frameToken: state.frameToken, dispose: state.dispose } : { html: undefined, error: null };
}
