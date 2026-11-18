import { useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import { parseDsCanvasFile, reconcileDsCanvas, type DsCanvas, type DsCanvasParse } from "@codeterm/chat-core";
import { loadLocalArtifact } from "../localArtifactLoader";
import { localArtifactMemory, localArtifactMemoryKey } from "../localArtifactMemory";
import { canvasFilePort, CanvasItemRecovery } from "./canvasFilePort";
import { CanvasFileSession, type CanvasSessionState } from "./canvasFileSession";

export type CanvasFileRead = { status: "loading" } | { status: "ready" } | { status: "error"; reason: string };

export interface CanvasFile {
  read: CanvasFileRead;
  state: CanvasSessionState;
  parsed: DsCanvasParse | null;
  /** The parsed board, keeping unchanged item objects across reloads and edits. */
  canvas: DsCanvas | null;
  session: CanvasFileSession;
  writable: boolean;
  /** Why the board shows without live updates, for its Details; `null` while it follows the file. */
  unwatched: string | null;
  itemRecovery: CanvasItemRecovery;
}

/**
 * Reads a `<ds-canvas src>` file and follows it: a change event re-reads it, at most one read
 * runs at a time, and a change during a read queues exactly one more.
 */
export function useCanvasFile(path: string, tabId: string | undefined): CanvasFile {
  const memoryKey = localArtifactMemoryKey(path, "canvas", tabId);
  const reloadRef = useRef<() => void>(() => {});
  const session = useMemo(() => {
    const created = new CanvasFileSession({
      write: (base, next) => {
        const port = canvasFilePort();
        if (!port || !tabId) return Promise.reject(new Error("Saving canvases is unavailable here."));
        return port.write(path, tabId, base, next);
      },
      reload: () => reloadRef.current(),
    });
    const remembered = localArtifactMemory.get(memoryKey);
    if (remembered) created.observe(remembered.data);
    return created;
  }, [path, tabId, memoryKey]);
  const state = useSyncExternalStore(session.subscribe, session.snapshot, session.snapshot);
  const itemRecovery = useMemo(() => new CanvasItemRecovery(), [session]);
  const [read, setRead] = useState<CanvasFileRead>(() => localArtifactMemory.get(memoryKey) ? { status: "ready" } : { status: "loading" });
  const [writable, setWritable] = useState(false);
  const [unwatched, setUnwatched] = useState<string | null>(null);

  useEffect(() => {
    let alive = true;
    let reading = false;
    let again = false;
    const reload = async () => {
      if (reading) { again = true; return; }
      reading = true;
      try {
        do {
          again = false;
          const file = await loadLocalArtifact(path, "canvas", tabId);
          if (!alive) return;
          localArtifactMemory.set(memoryKey, file);
          session.observe(file.data);
          setRead((current) => current.status === "ready" ? current : { status: "ready" });
        } while (again && alive);
      } catch (error) {
        if (alive) setRead({ status: "error", reason: error instanceof Error ? error.message : String(error) });
      } finally {
        reading = false;
      }
    };
    reloadRef.current = () => void reload();
    void reload();
    const port = canvasFilePort();
    let release: (() => void) | null = null;
    if (port && tabId) {
      port.watch(path, tabId, () => { itemRecovery.changed(); void reload(); }).then(
        (watch) => {
          if (!alive) { watch.release(); return; }
          release = watch.release;
          setWritable(watch.writable);
          setUnwatched(watch.watching ? null :"Live updates are off because too many canvases are open. Close one, then reopen this canvas.");
        },
        () => { if (alive) setWritable(false); },
      );
    }
    return () => {
      alive = false;
      reloadRef.current = () => {};
      release?.();
    };
  }, [path, tabId, memoryKey, session, itemRecovery]);

  const parsed = useMemo(() => state.text === null ? null : parseDsCanvasFile(state.text), [state.text]);
  const previous = useRef<DsCanvas | null>(null);
  // A half-written or broken file keeps the last good board on screen beside the error.
  const canvas = useMemo(() => {
    if (!parsed?.ok) return previous.current;
    const next = reconcileDsCanvas(previous.current, parsed.canvas);
    previous.current = next;
    return next;
  }, [parsed]);
  return { read, state, parsed, canvas, session, writable, unwatched, itemRecovery };
}
