import { Terminal, type LucideProps } from "lucide-react";

// The reel's shells only use their purpose icons; a custom shell icon falls back to the terminal glyph instead of
// pulling lucide's per-icon dynamic chunks (about 1,700 files) into the embed.
export type IconName = string;

export function DynamicIcon({ name: _name, ...props }: LucideProps & { name: IconName }) {
  return <Terminal {...props} />;
}
