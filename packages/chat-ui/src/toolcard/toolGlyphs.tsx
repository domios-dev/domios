import {
  ClipboardList,
  FileEdit,
  FileSearch,
  Globe,
  ListTodo,
  Search,
  Terminal,
  Wrench,
} from "lucide-react";
import type { LucideIcon } from "lucide-react";

export type ToolGlyph = LucideIcon;

export const toolGlyphs = {
  command: Terminal,
  edit: FileEdit,
  read: FileSearch,
  search: Search,
  task: ListTodo,
  fetch: Globe,
  codeterm: ClipboardList,
  generic: Wrench,
} satisfies Record<string, ToolGlyph>;
