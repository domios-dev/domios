import { mentionMatch, type MentionSource } from "../composerMentions";

export interface MentionFile { name: string; path: string }

export function fileMentionSource(read: (path: string, signal: AbortSignal) => Promise<readonly MentionFile[]>): MentionSource {
  return {
    id: "files", group: "Files",
    async search(query, { scope, signal }) {
      const path = scope.cwd ?? scope.projectPath;
      if (!path || !scope.workspaceId) return [];
      signal.throwIfAborted();
      const files = await read(path, signal);
      signal.throwIfAborted();
      return files.map(file => ({
        kind: "file" as const, id: `files:${file.path}`, label: file.name, group: "Files", description: file.path.replace(/^\\\\\?\\UNC\\/i, "\\\\").replace(/^\\\\\?\\/, ""),
        insertText: JSON.stringify(file.path), match: mentionMatch(query, [file.name]),
      }));
    },
  };
}
