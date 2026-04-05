import { useEffect, useRef, useState, useCallback } from "react";
import type { Terminal } from "@xterm/xterm";
import { invoke } from "@tauri-apps/api/core";
import { InputTracker } from "../../lib/completions/inputTracker";
import { CompletionEngine } from "../../lib/completions/engine";
import { defaultDefinitions } from "../../lib/completions/definitions";
import type { CompletionItem } from "../../lib/completions/types";

export interface UseTabCompletionOptions {
  termRef: React.MutableRefObject<Terminal | null>;
  containerRef: React.RefObject<HTMLDivElement | null>;
  connectedPtyRef: React.MutableRefObject<string | null>;
}

export interface UseTabCompletionReturn {
  completionItems: CompletionItem[];
  completionAnchor: { x: number; y: number };
  /** Returns true if tab was handled by completion engine */
  handleTab: () => boolean;
  dismissCompletions: () => void;
  feedInput: (data: string) => void;
  feedOutput: (text: string) => void;
  acceptCompletion: (item: CompletionItem) => void;
  trackerRef: React.MutableRefObject<InputTracker | null>;
  engineRef: React.MutableRefObject<CompletionEngine | null>;
}

export function useTabCompletion({
  termRef,
  containerRef,
  connectedPtyRef,
}: UseTabCompletionOptions): UseTabCompletionReturn {
  const [completionItems, setCompletionItems] = useState<CompletionItem[]>([]);
  const [completionAnchor, setCompletionAnchor] = useState({ x: 0, y: 0 });
  const trackerRef = useRef<InputTracker | null>(null);
  const engineRef = useRef<CompletionEngine | null>(null);

  // Init completion engine once
  useEffect(() => {
    const tracker = new InputTracker();
    const engine = new CompletionEngine();
    for (const def of defaultDefinitions) engine.register(def);
    trackerRef.current = tracker;
    engineRef.current = engine;
  }, []);

  const handleTab = useCallback((): boolean => {
    const tracker = trackerRef.current;
    const engine = engineRef.current;
    const term = termRef.current;
    if (!tracker || !term) return false;

    const parsed = tracker.getCursorWord();
    if (!parsed) return false;

    if (engine && engine.hasDefinition(parsed.command)) {
      const items = engine.getCompletions(parsed);
      if (items.length === 1) {
        const eraseCount = parsed.currentArg.length;
        const erase = "\x7f".repeat(eraseCount);
        const insert = items[0].text + " ";
        const ptyId = connectedPtyRef.current;
        if (ptyId) invoke("write_pty_string", { ptyId, data: erase + insert });
        tracker.feed(erase + insert);
        return true;
      }
      if (items.length > 1) {
        const rect = containerRef.current?.getBoundingClientRect();
        setCompletionItems(items);
        setCompletionAnchor({
          x: (rect?.left ?? 0) + 100,
          y: (rect?.top ?? 0) + (term.buffer.active.cursorY + 1) * 18,
        });
        return true;
      }
    }
    return false;
  }, [termRef, containerRef, connectedPtyRef]);

  const dismissCompletions = useCallback(() => {
    setCompletionItems([]);
  }, []);

  const feedInput = useCallback((data: string) => {
    trackerRef.current?.feed(data);
  }, []);

  const feedOutput = useCallback((text: string) => {
    trackerRef.current?.feedOutput(text);
  }, []);

  const acceptCompletion = useCallback((item: CompletionItem) => {
    const tracker = trackerRef.current;
    const parsed = tracker?.getCursorWord();
    const eraseCount = parsed?.currentArg.length ?? 0;
    const erase = "\x7f".repeat(eraseCount);
    const insert = item.text + " ";
    const ptyId = connectedPtyRef.current;
    if (ptyId) invoke("write_pty_string", { ptyId, data: erase + insert });
    tracker?.feed(erase + insert);
    setCompletionItems([]);
  }, [connectedPtyRef]);

  return {
    completionItems,
    completionAnchor,
    handleTab,
    dismissCompletions,
    feedInput,
    feedOutput,
    acceptCompletion,
    trackerRef,
    engineRef,
  };
}
