import {
  Terminal,        // Bash
  FileText,        // Read
  Pencil,          // Write
  Wrench,          // Edit
  Bot,             // Agent / Task
  Search,          // Grep, ToolSearch
  Folder,          // Glob
  Sparkles,        // Skill
  RefreshCw,       // Update
} from "lucide-react";
import type { LucideIcon } from "lucide-react";
import React from "react";

export const TOOL_ICONS: Record<string, LucideIcon> = {
  Bash: Terminal,
  Read: FileText,
  Write: Pencil,
  Edit: Wrench,
  Agent: Bot,
  Task: Bot,
  Grep: Search,
  Glob: Folder,
  Skill: Sparkles,
  Update: RefreshCw,
  ToolSearch: Search,
};

export function ToolIcon({ name }: { name: string }) {
  const Icon = TOOL_ICONS[name] ?? Wrench;
  return React.createElement(Icon, {
    size: 12,
    className: "inline opacity-70",
    style: { verticalAlign: "-2px" },
  });
}
