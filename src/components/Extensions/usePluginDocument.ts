import { useEffect, useState } from "react";
import { baseDaemonTransport } from "../../lib/daemonBootstrap";
import { loadPluginDocument, type PluginDocument } from "../../lib/pluginDocument";

type DocumentState = Partial<Pick<PluginDocument, "channel" | "html" | "invoke" | "media" | "hostRequest" | "dispose" | "frameToken">> & {error: string | null};

export function usePluginDocument(path: string): DocumentState {
  const [state, setState] = useState<DocumentState & {path: string}>({path, error: null});
  useEffect(() => {
    const controller = new AbortController();
    let dispose: (() => void) | undefined;
    void loadPluginDocument(baseDaemonTransport(), path, controller.signal).then((document) => {
      if (controller.signal.aborted) { document.dispose(); return; }
      dispose = document.dispose;
      setState({ path, channel: document.channel, html: document.html, invoke: document.invoke, media: document.media, hostRequest: document.hostRequest, frameToken: document.frameToken, dispose: document.dispose, error: null });
    }).catch((error: unknown) => {
      if (!controller.signal.aborted) setState({ path, error: String(error) });
    });
    return () => { controller.abort(); dispose?.(); };
  }, [path]);
  return state.path === path ? { channel: state.channel, html: state.html, error: state.error, invoke: state.invoke, media: state.media, hostRequest: state.hostRequest, frameToken: state.frameToken, dispose: state.dispose } : { html: undefined, error: null };
}
