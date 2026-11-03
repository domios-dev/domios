import type { ToolEventKind } from "@codeterm/chat-core";
import { toolGlyphs, type ToolGlyph } from "./toolGlyphs";

export interface ToolKindVisual {
  glyph: ToolGlyph;
  color: `var(--ct-${string})`;
  label: string;
}

export const toolTones = {
  success: "var(--ct-green)",
  info: "var(--ct-blue)",
  warning: "var(--ct-yellow)",
  danger: "var(--ct-red)",
  muted: "var(--ct-muted-fg)",
} as const;

export const toolKindVisual: Record<ToolEventKind, ToolKindVisual> = {
  command: { glyph: toolGlyphs.command, color: toolTones.info, label: "command" },
  edit: { glyph: toolGlyphs.edit, color: toolTones.success, label: "edit" },
  read: { glyph: toolGlyphs.read, color: toolTones.info, label: "read" },
  search: { glyph: toolGlyphs.search, color: toolTones.warning, label: "search" },
  task: { glyph: toolGlyphs.task, color: toolTones.info, label: "task" },
  fetch: { glyph: toolGlyphs.fetch, color: toolTones.info, label: "fetch" },
  codeterm: { glyph: toolGlyphs.codeterm, color: toolTones.info, label: "codeterm" },
  web: { glyph: toolGlyphs.web, color: toolTones.info, label: "web" },
  image: { glyph: toolGlyphs.image, color: toolTones.info, label: "image" },
  agent: { glyph: toolGlyphs.task, color: toolTones.info, label: "agent" },
  generic: { glyph: toolGlyphs.generic, color: toolTones.muted, label: "tool" },
};
