import type { ToolEventKind } from "@codeterm/chat-core";
import { createContext, createElement, useContext, type ReactNode } from "react";
import { toolGlyphs, type ToolGlyph } from "./toolGlyphs";

export interface ToolKindVisual {
  glyph: ToolGlyph;
  color: `var(--ct-${string})`;
  label: string;
}

export const toolKindVisual: Record<ToolEventKind, ToolKindVisual> = {
  command: { glyph: toolGlyphs.command, color: "var(--ct-blue)", label: "command" },
  edit: { glyph: toolGlyphs.edit, color: "var(--ct-green)", label: "edit" },
  read: { glyph: toolGlyphs.read, color: "var(--ct-cyan)", label: "read" },
  search: { glyph: toolGlyphs.search, color: "var(--ct-yellow)", label: "search" },
  task: { glyph: toolGlyphs.task, color: "var(--ct-magenta)", label: "task" },
  fetch: { glyph: toolGlyphs.fetch, color: "var(--ct-orange)", label: "fetch" },
  codeterm: { glyph: toolGlyphs.codeterm, color: "var(--ct-accent)", label: "codeterm" },
  generic: { glyph: toolGlyphs.generic, color: "var(--ct-muted-fg)", label: "tool" },
};

export const ShowCommandsContext = createContext(false);

export function useShowCommands(): boolean {
  return useContext(ShowCommandsContext);
}

export function ShowCommandsProvider({ showCommands, children }: { showCommands: boolean; children: ReactNode }) {
  return createElement(ShowCommandsContext.Provider, { value: showCommands }, children);
}
