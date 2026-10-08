import { useEffect, useState } from "react";
import { loadLocalArtifact } from "./localArtifactLoader";
import { useLocalArtifactTabId } from "./LocalArtifactScope";

const PREVIEW_LINES = 8;
const PREVIEW_CHARS = 420;

/** The head of a text, clipped to a hover card: at most a few lines and a few hundred characters. */
export function textPreviewSnippet(text: string): string {
  const lines = text.replace(/\r\n/g, "\n").replace(/^\s*\n/, "").split("\n");
  let snippet = lines.slice(0, PREVIEW_LINES).join("\n");
  if (snippet.length > PREVIEW_CHARS) snippet = snippet.slice(0, PREVIEW_CHARS);
  const clipped = snippet.length < text.trim().length;
  return clipped ? `${snippet.replace(/\s+$/, "")}…` : snippet.replace(/\s+$/, "");
}

/** Reads the head of a local text file through the host preview loader; undefined until it loads or when it cannot. */
export function useLocalTextPreview(path: string | undefined): string | undefined {
  const tabId = useLocalArtifactTabId();
  const [state, setState] = useState<{ path: string; text?: string } | null>(null);
  useEffect(() => {
    if (!path) return;
    let current = true;
    loadLocalArtifact(path, "document", tabId).then(
      (file) => { if (current) setState({ path, text: textPreviewSnippet(file.data) }); },
      () => { if (current) setState({ path }); },
    );
    return () => { current = false; };
  }, [path, tabId]);
  return path !== undefined && state?.path === path ? state.text : undefined;
}
